//! Stopping the dev daemons that keep a listed item in use, from the
//! clean-up window: only those the engine knows by their JVM's main class,
//! politely, and never by force.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use harbormaster_cleaner::Finding;
use harbormaster_engine::live::{self, Daemon, Holding};
use harbormaster_engine::{Member, Protection, Signals, Standing};
use harbormaster_platform::{ProcessKey, Signal};
use serde::Serialize;

use crate::cleaning::servers_running;
use crate::scanning::home;
use crate::stopping::{Reason, Stopped, Stopping, failed};

/// How long to wait for a daemon to exit before saying it is still running.
const GRACE: Duration = Duration::from_secs(5);
/// An approval left unused this long lapses, and the window asks again.
const APPROVAL_LIFETIME: Duration = Duration::from_secs(120);

/// What stopping the daemons that keep an item in use would do, for the
/// window to ask about.
#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub(crate) enum DaemonPreview {
    Ready { approval: u64, stops: Vec<Stopping> },
    Refused { reason: Reason },
}

/// What the user saw and agreed to stop.
#[derive(Debug)]
struct Approval {
    /// The list the item was shown in.
    token: u64,
    path: PathBuf,
    daemons: Vec<(ProcessKey, Daemon)>,
    at: Instant,
}

/// Stops of daemons the user agreed to, by the approval the window holds.
#[derive(Debug, Default)]
pub(crate) struct Daemons(Mutex<Ledger>);

#[derive(Debug, Default)]
struct Ledger {
    last: u64,
    open: HashMap<u64, Approval>,
}

/// What keeps items in use right now, and what a stop may reach.
struct Users {
    /// Whether this system tells when a process execs another program: every
    /// check relies on keys that change when it does.
    exec_tracked: bool,
    holdings: Vec<Holding>,
    running: Vec<(PathBuf, String)>,
    /// This program's own file, which nothing may take from it.
    own: PathBuf,
    /// This process and everything above it, if all of that could be traced.
    lineage: Option<Vec<u32>>,
    /// How the checks a stop runs find each daemon.
    standings: HashMap<ProcessKey, Standing>,
}

impl Users {
    fn now() -> Result<Self, Reason> {
        let holdings = live::holdings().map_err(failed)?;
        let daemons: Vec<ProcessKey> = holdings
            .iter()
            .filter(|holding| holding.daemon.is_some())
            .map(|holding| holding.key)
            .collect();
        Ok(Self {
            exec_tracked: harbormaster_platform::tracks_exec(),
            standings: live::guards(&home(), &daemons).map_err(failed)?,
            lineage: live::lineage().map_err(failed)?,
            running: servers_running().map_err(failed)?,
            own: std::env::current_exe()
                .and_then(std::fs::canonicalize)
                .map_err(|error| failed(error.into()))?,
            holdings,
        })
    }

    fn of(&self, finding: &Finding) -> Option<Vec<(ProcessKey, Daemon)>> {
        only_daemons(finding, &self.holdings, &self.running, &self.own)
    }

    /// The daemons that alone keep `finding` in use, if a stop would reach
    /// every one of them.
    fn to_stop(&self, finding: &Finding) -> Result<Vec<(ProcessKey, Daemon)>, Reason> {
        let daemons = self.of(finding).ok_or(Reason::NotFound)?;
        self.may_stop(&daemons)?;
        Ok(daemons)
    }

    /// Of the `listed` items in use, those whose daemons a stop would reach.
    fn stoppable(&self, listed: &[Finding]) -> Vec<PathBuf> {
        listed
            .iter()
            .filter(|finding| finding.in_use.is_some() && self.to_stop(finding).is_ok())
            .map(|finding| finding.path.clone())
            .collect()
    }

    /// Whether a stop may reach every one of `daemons`: none runs
    /// Harbormaster or anything above it, and the checks find each clear.
    fn may_stop(&self, daemons: &[(ProcessKey, Daemon)]) -> Result<(), Reason> {
        if !self.exec_tracked {
            return Err(Reason::UntrackedExec);
        }
        let lineage = self.lineage.as_deref().ok_or(Reason::UnknownLineage)?;
        if daemons.iter().any(|(key, _)| lineage.contains(&key.pid.0)) {
            return Err(Reason::RunsHarbormaster);
        }
        for (key, _) in daemons {
            match self.standings.get(key) {
                Some(Standing::Clear) => {}
                Some(Standing::Gone | Standing::Replaced) => return Err(Reason::Changed),
                Some(Standing::Protected(protection)) => {
                    return Err(Reason::Protected {
                        protection: *protection,
                    });
                }
                Some(Standing::Unknown) | None => {
                    return Err(Reason::Protected {
                        protection: Protection::Unverified,
                    });
                }
            }
        }
        Ok(())
    }
}

impl Daemons {
    /// Of the `listed` items in use, those only known daemons keep so, which
    /// the window may offer to stop.
    pub(crate) fn stoppable(&self, listed: &[Finding]) -> Result<Vec<PathBuf>, Reason> {
        Ok(Users::now()?.stoppable(listed))
    }

    /// What stopping the daemons that keep the item at `path` in use would
    /// do; it must be among those `listed` under `token`, byte for byte.
    pub(crate) fn preview(&self, token: u64, path: &Path, listed: &[Finding]) -> DaemonPreview {
        self.prepare(token, path, listed)
            .unwrap_or_else(|reason| DaemonPreview::Refused { reason })
    }

    fn prepare(
        &self,
        token: u64,
        path: &Path,
        listed: &[Finding],
    ) -> Result<DaemonPreview, Reason> {
        let finding = find(listed, path).ok_or(Reason::NotFound)?;
        let daemons = Users::now()?.to_stop(finding)?;
        let keys: Vec<ProcessKey> = daemons.iter().map(|(key, _)| *key).collect();
        let members = live::members(&keys).map_err(failed)?;
        let stops = shown(&daemons, &members).ok_or(Reason::Changed)?;
        let approval = self.approve(Approval {
            token,
            path: finding.path.clone(),
            daemons,
            at: Instant::now(),
        });
        Ok(DaemonPreview::Ready { approval, stops })
    }

    /// Asks the approved daemons to exit, after checking again that the item
    /// is still listed and that they are still what keeps it in use, and
    /// waits for them to go. `listed` hands back what is listed under a
    /// token, if that is still the list shown. Nothing is ever forced.
    pub(crate) fn stop(
        &self,
        approval: u64,
        listed: impl FnOnce(u64) -> Option<Vec<Finding>>,
    ) -> Result<Stopped, Reason> {
        let approval = self.take(approval, Instant::now())?;
        let listed = listed(approval.token).ok_or(Reason::Expired)?;
        let finding = find(&listed, &approval.path).ok_or(Reason::NotFound)?;
        // The checks before the signal, again.
        let keys = still_fits(&approval.daemons, finding, &Users::now()?)?;
        let outcomes = harbormaster_engine::stop(
            &keys,
            Signal::Terminate,
            GRACE,
            &Signals {
                send: &harbormaster_platform::signal,
                presence: &harbormaster_platform::presence,
                sleep: &std::thread::sleep,
            },
        );
        Ok(Stopped::new(outcomes, |key| {
            approval
                .daemons
                .iter()
                .find(|(approved, _)| *approved == key)
                .map_or_else(String::new, |(_, daemon)| daemon.name().to_owned())
        }))
    }

    fn approve(&self, approval: Approval) -> u64 {
        let mut ledger = self.lock();
        let now = approval.at;
        ledger
            .open
            .retain(|_, open| now.duration_since(open.at) < APPROVAL_LIFETIME);
        ledger.last += 1;
        let token = ledger.last;
        ledger.open.insert(token, approval);
        token
    }

    /// An approval serves one stop.
    fn take(&self, token: u64, now: Instant) -> Result<Approval, Reason> {
        let approval = self.lock().open.remove(&token).ok_or(Reason::Expired)?;
        if now.duration_since(approval.at) >= APPROVAL_LIFETIME {
            return Err(Reason::Expired);
        }
        Ok(approval)
    }

    fn lock(&self) -> MutexGuard<'_, Ledger> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn find<'a>(listed: &'a [Finding], path: &Path) -> Option<&'a Finding> {
    listed
        .iter()
        .find(|finding| finding.path.as_os_str() == path.as_os_str())
}

/// The daemons that keep `finding` in use, when nothing else does: none if
/// nothing holds it, a dev server runs in its project, or anything but a
/// known daemon holds something in it or its project, this program's own
/// file `own` included.
fn only_daemons(
    finding: &Finding,
    holdings: &[Holding],
    running: &[(PathBuf, String)],
    own: &Path,
) -> Option<Vec<(ProcessKey, Daemon)>> {
    let project = finding.project.as_deref();
    let keeps = |held: &Path| harbormaster_cleaner::holds(held, &finding.path, project);
    if keeps(own) || harbormaster_cleaner::in_use(&finding.path, project, &[], running).is_some() {
        return None;
    }
    let daemons = holdings
        .iter()
        .filter(|holding| holding.paths.iter().any(|held| keeps(held)))
        .map(|holding| Some((holding.key, holding.daemon?)))
        .collect::<Option<Vec<_>>>()?;
    (!daemons.is_empty()).then_some(daemons)
}

/// How the `daemons` are shown before they are stopped, from `members`:
/// not at all unless every one of them is there with its command.
fn shown(daemons: &[(ProcessKey, Daemon)], members: &[Member]) -> Option<Vec<Stopping>> {
    daemons
        .iter()
        .map(|(key, daemon)| {
            let member = members.iter().find(|member| member.key == *key)?;
            let command = member.command.clone()?;
            Some(Stopping {
                pid: member.pid,
                name: daemon.name().to_owned(),
                command: Some(command),
            })
        })
        .collect()
}

/// Whom to signal for the `approved` daemons, if they are still among what
/// alone keeps `finding` in use and a stop may still reach every one.
fn still_fits(
    approved: &[(ProcessKey, Daemon)],
    finding: &Finding,
    users: &Users,
) -> Result<Vec<ProcessKey>, Reason> {
    let now = users.of(finding).ok_or(Reason::Changed)?;
    if !approved.iter().all(|daemon| now.contains(daemon)) {
        return Err(Reason::Changed);
    }
    users.may_stop(approved)?;
    Ok(approved.iter().map(|(key, _)| *key).collect())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::process::{Child, Command};
    use std::time::{Duration, Instant};

    use harbormaster_cleaner::{Finding, Group, Identity, Kind};
    use harbormaster_engine::live::{Daemon, Holding};
    use harbormaster_engine::{Member, Outcome, Protection, Standing};
    use harbormaster_platform::{Pid, ProcessKey, StartTime};

    use super::{
        APPROVAL_LIFETIME, Approval, DaemonPreview, Daemons, Users, only_daemons, shown, still_fits,
    };
    use crate::stopping::{Reason, Stopping};

    const PROJECT: &str = "/Users/dev/code/app";

    fn key(pid: u32) -> ProcessKey {
        ProcessKey {
            pid: Pid(pid),
            start: StartTime(1),
            exec: 0,
        }
    }

    fn holding(pid: u32, daemon: Option<Daemon>, held: &str) -> Holding {
        Holding {
            key: key(pid),
            name: daemon.map_or("java".into(), |daemon| daemon.name().into()),
            daemon,
            paths: vec![PathBuf::from(held)],
        }
    }

    fn finding(path: &Path, project: &Path) -> Finding {
        Finding {
            group: Group::Projects,
            kind: Kind::Rebuildable,
            name: "build".into(),
            path: path.to_path_buf(),
            bytes: 1,
            picked: false,
            in_use: Some("Gradle daemon".into()),
            touched: None,
            project: Some(project.to_path_buf()),
            emptied: false,
            identity: Identity::default(),
        }
    }

    fn build() -> Finding {
        finding(&Path::new(PROJECT).join("build"), Path::new(PROJECT))
    }

    const OWN: &str = "/Applications/Harbormaster.app/Contents/MacOS/harbormaster-app";

    #[test]
    fn names_the_daemons_when_only_daemons_keep_an_item_in_use() {
        let holdings = [
            holding(10, Some(Daemon::Gradle), PROJECT),
            holding(
                11,
                Some(Daemon::Kotlin),
                &format!("{PROJECT}/build/classes"),
            ),
            holding(12, None, "/Users/dev/elsewhere"),
        ];

        let daemons = only_daemons(&build(), &holdings, &[], Path::new(OWN));

        assert_eq!(
            daemons,
            Some(vec![(key(10), Daemon::Gradle), (key(11), Daemon::Kotlin)])
        );
    }

    #[test]
    fn names_none_when_anything_else_keeps_the_item_in_use_too() {
        let gradle = holding(10, Some(Daemon::Gradle), PROJECT);
        let server = [(PathBuf::from(PROJECT), "localhost:3000".to_string())];
        let own = format!("{PROJECT}/build/harbormaster-app");

        // Another program, a dev server, this app's own file, or nothing at all.
        let other = [gradle.clone(), holding(11, None, PROJECT)];
        assert_eq!(only_daemons(&build(), &other, &[], Path::new(OWN)), None);
        assert_eq!(
            only_daemons(
                &build(),
                std::slice::from_ref(&gradle),
                &server,
                Path::new(OWN)
            ),
            None
        );
        assert_eq!(
            only_daemons(&build(), &[gradle], &[], Path::new(&own)),
            None
        );
        assert_eq!(only_daemons(&build(), &[], &[], Path::new(OWN)), None);
    }

    /// What keeps items in use, each daemon as a stop finds it clear, on a
    /// system that tracks exec, with Harbormaster run from far away.
    fn users(holdings: Vec<Holding>) -> Users {
        Users {
            exec_tracked: true,
            standings: holdings
                .iter()
                .map(|holding| (holding.key, Standing::Clear))
                .collect(),
            holdings,
            running: Vec::new(),
            own: PathBuf::from(OWN),
            lineage: Some(vec![900, 1]),
        }
    }

    fn judged(mut users: Users, pid: u32, standing: Standing) -> Users {
        users.standings.insert(key(pid), standing);
        users
    }

    #[test]
    fn offers_a_stop_only_where_one_would_go_through() {
        let lib = Path::new("/Users/dev/code/lib");
        let users = judged(
            users(vec![
                holding(10, Some(Daemon::Gradle), PROJECT),
                holding(11, Some(Daemon::Kotlin), "/Users/dev/code/lib"),
            ]),
            11,
            Standing::Protected(Protection::Session),
        );
        let idle = Finding {
            in_use: None,
            ..finding(&Path::new(PROJECT).join(".gradle"), Path::new(PROJECT))
        };

        let listed = [build(), finding(&lib.join("build"), lib), idle];

        assert_eq!(users.stoppable(&listed), [build().path]);
        // Where exec isn't tracked, every stop is refused.
        let untracked = Users {
            exec_tracked: false,
            ..users
        };
        assert_eq!(untracked.stoppable(&listed), Vec::<PathBuf>::new());
    }

    fn member(pid: u32) -> Member {
        Member {
            key: key(pid),
            pid,
            name: "java".into(),
            command: Some(format!("java -cp daemon.jar {pid}")),
            memory_bytes: None,
            cpu_percent: None,
        }
    }

    #[test]
    fn shows_every_daemon_it_would_stop_or_offers_no_stop() {
        let daemons = [(key(10), Daemon::Gradle), (key(11), Daemon::Kotlin)];
        let named = |stops: Vec<Stopping>| {
            stops
                .into_iter()
                .map(|stop| (stop.pid, stop.name, stop.command))
                .collect::<Vec<_>>()
        };

        let both = shown(&daemons, &[member(11), member(10)]).map(named);
        // One exited, or couldn't be read, once it was found keeping the item in use.
        let one = shown(&daemons, &[member(11)]).map(named);
        let unread = shown(
            &daemons,
            &[
                member(10),
                Member {
                    command: None,
                    ..member(11)
                },
            ],
        );

        assert_eq!(
            both,
            Some(vec![
                (10, "Gradle daemon".into(), member(10).command),
                (11, "Kotlin daemon".into(), member(11).command),
            ])
        );
        assert_eq!(one, None);
        assert!(unread.is_none());
    }

    #[test]
    fn signals_the_approved_daemons_while_they_alone_keep_the_item_in_use() {
        let approved = [(key(10), Daemon::Gradle)];
        let users = users(vec![
            holding(10, Some(Daemon::Gradle), PROJECT),
            holding(12, None, "/Users/dev/elsewhere"),
        ]);

        assert_eq!(still_fits(&approved, &build(), &users), Ok(vec![key(10)]));
    }

    #[test]
    fn signals_nothing_once_the_daemons_are_not_what_was_approved() {
        let approved = [(key(10), Daemon::Gradle), (key(11), Daemon::Kotlin)];
        let gradle = || holding(10, Some(Daemon::Gradle), PROJECT);
        let kotlin = || holding(11, Some(Daemon::Kotlin), PROJECT);
        let ran_another = Holding {
            key: ProcessKey { exec: 1, ..key(10) },
            ..gradle()
        };

        for holdings in [
            // Something else keeps the item in use as well now.
            vec![gradle(), kotlin(), holding(12, None, PROJECT)],
            // One of them exited, or both did.
            vec![kotlin()],
            vec![],
            // One runs another program now, under the same pid.
            vec![ran_another, kotlin()],
        ] {
            assert_eq!(
                still_fits(&approved, &build(), &users(holdings)),
                Err(Reason::Changed)
            );
        }
    }

    #[test]
    fn signals_nothing_a_stop_may_not_reach() {
        let approved = [(key(10), Daemon::Gradle)];
        let gradle = || users(vec![holding(10, Some(Daemon::Gradle), PROJECT)]);
        let fits = |users: Users| still_fits(&approved, &build(), &users);
        let unverified = Err(Reason::Protected {
            protection: Protection::Unverified,
        });

        assert_eq!(
            fits(Users {
                exec_tracked: false,
                ..gradle()
            }),
            Err(Reason::UntrackedExec)
        );
        assert_eq!(
            fits(Users {
                lineage: Some(vec![900, 10, 1]),
                ..gradle()
            }),
            Err(Reason::RunsHarbormaster)
        );
        assert_eq!(
            fits(Users {
                lineage: None,
                ..gradle()
            }),
            Err(Reason::UnknownLineage)
        );
        assert_eq!(
            fits(judged(
                gradle(),
                10,
                Standing::Protected(Protection::Session)
            )),
            Err(Reason::Protected {
                protection: Protection::Session
            })
        );
        assert_eq!(fits(judged(gradle(), 10, Standing::Unknown)), unverified);
        assert_eq!(
            fits(Users {
                standings: HashMap::new(),
                ..gradle()
            }),
            unverified
        );
        assert_eq!(
            fits(judged(gradle(), 10, Standing::Replaced)),
            Err(Reason::Changed)
        );
    }

    #[test]
    fn serves_one_stop_per_approval_and_only_while_it_lasts() {
        let daemons = Daemons::default();
        let now = Instant::now();
        let approval = || Approval {
            token: 7,
            path: PathBuf::from(PROJECT),
            daemons: vec![(key(10), Daemon::Gradle)],
            at: now,
        };

        let once = daemons.approve(approval());
        assert!(daemons.take(once, now).is_ok());
        assert_eq!(daemons.take(once, now).err(), Some(Reason::Expired));

        let late = daemons.approve(approval());
        assert_eq!(
            daemons
                .take(late, now + APPROVAL_LIFETIME + Duration::from_secs(1))
                .err(),
            Some(Reason::Expired)
        );
    }

    /// A program called `java` that only waits, started as a Gradle daemon's
    /// JVM would be, in `folder`.
    fn fake_gradle_daemon(folder: &Path) -> Child {
        let source = folder.join("main.c");
        std::fs::write(
            &source,
            "#include <unistd.h>\nint main(void){sleep(30);return 0;}\n",
        )
        .expect("the program's source should be written");
        let java = folder.join("java");
        let built = Command::new("cc")
            .arg("-o")
            .arg(&java)
            .arg(&source)
            .status()
            .expect("cc should run");
        assert!(built.success());
        Command::new(&java)
            .args([
                "-Xmx64m",
                "-cp",
                "gradle-daemon-main-8.14.jar",
                "org.gradle.launcher.daemon.bootstrap.GradleDaemon",
                "8.14",
            ])
            .env_remove("JDK_JAVA_OPTIONS")
            .current_dir(folder)
            .spawn()
            .expect("the program should start")
    }

    #[test]
    fn stops_the_gradle_daemon_that_keeps_a_listed_folder_in_use() {
        let folder = tempfile::tempdir().unwrap();
        let project = folder.path().canonicalize().unwrap();
        std::fs::create_dir(project.join("build")).unwrap();
        let listed = vec![finding(&project.join("build"), &project)];
        let mut daemon = fake_gradle_daemon(&project);
        let pid = daemon.id();
        let daemons = Daemons::default();

        let stoppable = daemons.stoppable(&listed);
        let preview = daemons.preview(7, &project.join("build"), &listed);
        let DaemonPreview::Ready { approval, stops } = preview else {
            daemon.kill().unwrap();
            panic!("no stop offered: {preview:?}");
        };
        let stopped = daemons.stop(approval, |token| (token == 7).then(|| listed.clone()));
        // A daemon seen exiting may not be ready to be waited for yet.
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut exited = daemon.try_wait().unwrap().is_some();
        while !exited && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
            exited = daemon.try_wait().unwrap().is_some();
        }
        if !exited {
            daemon.kill().unwrap();
        }
        daemon.wait().unwrap();

        assert_eq!(stoppable.unwrap(), [project.join("build")]);
        assert_eq!(
            stops
                .iter()
                .map(|stop| (stop.pid, stop.name.as_str()))
                .collect::<Vec<_>>(),
            [(pid, "Gradle daemon")]
        );
        let stopped = serde_json::to_value(stopped.unwrap()).unwrap();
        assert_eq!(
            stopped["outcomes"][0]["outcome"],
            serde_json::to_value(Outcome::Stopped).unwrap()
        );
        assert!(exited);
    }

    #[test]
    fn stops_nothing_once_the_list_it_was_offered_from_is_gone() {
        let folder = tempfile::tempdir().unwrap();
        let project = folder.path().canonicalize().unwrap();
        std::fs::create_dir(project.join("build")).unwrap();
        let listed = vec![finding(&project.join("build"), &project)];
        let mut daemon = fake_gradle_daemon(&project);
        let daemons = Daemons::default();

        let preview = daemons.preview(7, &project.join("build"), &listed);
        let stopped = match preview {
            DaemonPreview::Ready { approval, .. } => daemons.stop(approval, |_| None).err(),
            DaemonPreview::Refused { reason } => Some(reason),
        };
        let running = daemon.try_wait().unwrap().is_none();
        daemon.kill().unwrap();
        daemon.wait().unwrap();

        assert_eq!(stopped, Some(Reason::Expired));
        assert!(running);
    }
}
