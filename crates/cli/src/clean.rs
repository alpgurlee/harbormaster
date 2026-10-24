//! The cleaner in the terminal: what could go, how much room it would make,
//! and, with `--yes`, what removing what is picked did.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use harbormaster_cleaner::{Cleaned, Finding, Group, Kind, Outcome, Removal, Survey, Usage};
use harbormaster_engine::{Classification, Scanner, live};

use crate::{FAILED, format};

const NAME_WIDTH: usize = 32;
/// How long a project sits untouched before its build outputs are picked.
const IDLE_AFTER: Duration = Duration::from_secs(30 * 86_400);

/// `harbormaster clean`: what could go, and with `yes`, removing what is picked.
pub(crate) fn run(json: bool, yes: bool) -> anyhow::Result<u8> {
    let home = harbormaster_cleaner::account_home().context("could not find the home folder")?;
    let roots = harbormaster_cleaner::usual_roots(&home);
    let usage = in_use_now(&home)?;
    let now = SystemTime::now();
    let temp = harbormaster_cleaner::temp_folder(&home);
    let survey = Survey {
        home: &home,
        roots: &roots,
        busy: &usage.busy,
        running: &usage.running,
        idle_after: IDLE_AFTER,
        now,
        temp: temp.as_deref(),
    };
    let findings = harbormaster_cleaner::scan(&survey, &AtomicBool::new(false))
        .with_context(|| format!("could not look through {}", home.display()))?;
    let mut out = io::stdout().lock();
    if !json {
        let now = now.duration_since(UNIX_EPOCH)?.as_secs();
        for line in lines(&findings, &home, now, yes) {
            writeln!(out, "{line}")?;
        }
    }
    if !yes {
        if json {
            writeln!(out, "{}", document(&findings, None)?)?;
        }
        return Ok(0);
    }
    remove_picked(&findings, json, &mut out, |picked, stop, told| {
        let reach = harbormaster_cleaner::Reach {
            home: &home,
            shared: harbormaster_cleaner::SHARED_APPS.map(Path::new),
        };
        harbormaster_cleaner::remove(
            &reach,
            picked,
            &|| in_use_now(&home).map_err(io::Error::other),
            &|path| harbormaster_platform::trash(path).map_err(io::Error::other),
            stop,
            told,
        )
        .with_context(|| format!("could not remove anything under {}", home.display()))
    })
}

/// The paths this user's other processes hold, and the projects with a dev
/// server running, as they are now.
fn in_use_now(home: &Path) -> anyhow::Result<Usage> {
    Ok(Usage {
        busy: live::held().context("could not tell which folders are in use")?,
        running: running_projects(home)?,
    })
}

/// The projects with a dev server running, each with what to call the server.
fn running_projects(home: &Path) -> anyhow::Result<Vec<(PathBuf, String)>> {
    let snapshot = live::scan(&mut Scanner::default(), home)?;
    Ok(snapshot
        .services
        .iter()
        .filter(|service| service.classification == Classification::Dev)
        .filter_map(|service| {
            let root = service.project.as_ref()?.root.clone();
            Some((root, format!("the server on {}", format::ports(service))))
        })
        .collect())
}

/// The findings, and what removing did when it ran, as the JSON a script
/// reads; fails on a path JSON can't hold.
pub(crate) fn document(
    findings: &[Finding],
    cleaned: Option<&Cleaned>,
) -> serde_json::Result<String> {
    let mut document = serde_json::json!({
        "schema": "harbormaster/v1",
        "findings": serde_json::to_value(findings)?,
    });
    if let Some(cleaned) = cleaned {
        document["cleaned"] = serde_json::to_value(cleaned)?;
    }
    serde_json::to_string_pretty(&document)
}

/// The table of findings by group, then what picking would free, or, when
/// `removing`, that it is being freed. `now` is in seconds since the Unix
/// epoch, for saying how long a project has sat.
pub(crate) fn lines(findings: &[Finding], home: &Path, now: u64, removing: bool) -> Vec<String> {
    if findings.is_empty() {
        return vec!["Nothing to clean up.".into()];
    }
    let mut rows = vec![["WHAT", "SIZE", "WHERE", ""].map(String::from).to_vec()];
    let mut group = None;
    for finding in findings {
        if group != Some(finding.group) {
            group = Some(finding.group);
            rows.push(vec![title(finding.group).into()]);
        }
        rows.push(vec![
            format!("  {}", format::truncate(&finding.name, NAME_WIDTH)),
            format::bytes(finding.bytes),
            shown(&finding.path, home),
            note(finding, now),
        ]);
    }
    let picked: u64 = findings
        .iter()
        .filter(|finding| finding.picked)
        .map(|finding| finding.bytes)
        .sum();
    let all: u64 = findings.iter().map(|finding| finding.bytes).sum();
    let (picked, all) = (format::bytes(picked), format::bytes(all));
    let mut lines: Vec<String> = format::table(&rows).lines().map(String::from).collect();
    lines.push(String::new());
    if removing {
        lines.push(format!("Removing what is picked: {picked} of {all}."));
    } else {
        lines.push(format!(
            "Would free {picked} of {all}. Nothing was removed: this is a dry run."
        ));
        lines.push("Run it with --yes to remove what is picked.".into());
    }
    lines
}

/// Removes what is picked among `findings` through `remove`, writing each
/// removal to `out` as it happens and then the room freed or, with `json`,
/// the whole document. Nothing is removed when the document couldn't be
/// written, and once `out` can't be written to, the rest is stopped rather
/// than removed unseen. Returns the exit status.
pub(crate) fn remove_picked(
    findings: &[Finding],
    json: bool,
    out: &mut dyn Write,
    remove: impl FnOnce(&[Finding], &AtomicBool, &mut dyn FnMut(&Removal)) -> anyhow::Result<Cleaned>,
) -> anyhow::Result<u8> {
    if json {
        document(findings, None)?;
    }
    let picked: Vec<Finding> = findings
        .iter()
        .filter(|finding| finding.picked)
        .cloned()
        .collect();
    let stop = AtomicBool::new(false);
    let mut unwritten = None;
    let cleaned = remove(&picked, &stop, &mut |done| {
        if json || unwritten.is_some() {
            return;
        }
        if let Err(error) = writeln!(out, "{}", removal(done, &picked)) {
            unwritten = Some(error);
            stop.store(true, Ordering::Relaxed);
        }
    });
    // Why removing stopped comes first, whatever went wrong after.
    if let Some(error) = unwritten {
        return Err(
            anyhow::Error::new(error).context("stopped removing: its progress couldn't be written")
        );
    }
    let cleaned = cleaned?;
    if json {
        writeln!(out, "{}", document(findings, Some(&cleaned))?)?;
    } else {
        writeln!(out, "{}", freed(&cleaned))?;
    }
    Ok(status(&cleaned))
}

/// What happened to one finding, named as the table names it.
pub(crate) fn removal(removal: &Removal, findings: &[Finding]) -> String {
    let name = findings
        .iter()
        .find(|finding| finding.path == removal.path)
        .map_or_else(
            || removal.path.display().to_string(),
            |finding| finding.name.clone(),
        );
    let (done, why) = match &removal.outcome {
        Outcome::Deleted => ("deleted", None),
        Outcome::Partly { left } => (
            "deleted",
            Some(format!("all but {left}, which programs hold")),
        ),
        Outcome::Trashed => ("trashed", None),
        Outcome::Refused { why } => ("skipped", Some(why.to_string())),
        Outcome::Failed { why } => ("failed", Some(why.clone())),
        Outcome::Stopped => ("stopped", Some("before its turn".into())),
        Outcome::Interrupted => (
            "stopped",
            Some("during cleanup; some contents may have been deleted".into()),
        ),
    };
    let why = why.map(|why| format!(": {why}")).unwrap_or_default();
    format::printable(&format!("  {done:<8} {name}{why}"))
}

/// The room the disk got back.
pub(crate) fn freed(cleaned: &Cleaned) -> String {
    if cleaned.removals.is_empty() {
        return "Nothing is picked, so nothing was removed.".into();
    }
    format!("Freed {}.", format::bytes(cleaned.freed_bytes))
}

/// The exit status: 1 when anything failed. A skip is the safety rules at
/// work, not a failure.
pub(crate) fn status(cleaned: &Cleaned) -> u8 {
    let failed = cleaned
        .removals
        .iter()
        .any(|removal| matches!(removal.outcome, Outcome::Failed { .. }));
    if failed { FAILED } else { 0 }
}

fn title(group: Group) -> &'static str {
    match group {
        Group::Projects => "Projects",
        Group::Packages => "Packages",
        Group::Xcode => "Xcode",
        Group::Apps => "App caches",
        Group::Temp => "Temporary files",
        Group::Logs => "Logs",
        Group::Trash if cfg!(windows) => "Recycle Bin",
        Group::Trash => "Trash",
        Group::Videos => "Videos",
        Group::Archives => "Archives and disk images",
        Group::Installers => "Installers",
        Group::Audio => "Audio",
        Group::Images => "Images",
        Group::Documents => "Documents",
        Group::OtherFiles => "Other large files",
        Group::Applications => "Apps",
    }
}

/// Why a finding isn't picked, or how long its project has sat.
fn note(finding: &Finding, now: u64) -> String {
    if let Some(who) = &finding.in_use {
        return format!("in use by {who}");
    }
    match (finding.kind, finding.touched) {
        (Kind::Rebuildable, Some(touched)) => {
            let idle = Duration::from_secs(now.saturating_sub(touched));
            format!("last touched {}", ago(idle))
        }
        (Kind::Rebuildable, None) if finding.picked => String::new(),
        (Kind::Rebuildable, None) => "used last".into(),
        (Kind::Personal, _) => "your own work".into(),
        (Kind::Discarded, _) => "empties for good".into(),
    }
}

/// `~/.npm/_cacache` for a path in the home folder.
fn shown(path: &Path, home: &Path) -> String {
    path.strip_prefix(home).map_or_else(
        |_| path.display().to_string(),
        |inside| format!("~/{}", inside.display()),
    )
}

/// How long ago, in days or months: `today`, `3 days ago`, `2 months ago`.
pub(crate) fn ago(elapsed: Duration) -> String {
    match elapsed.as_secs() / 86_400 {
        0 => "today".into(),
        1 => "yesterday".into(),
        days @ 2..60 => format!("{days} days ago"),
        days => format!("{} months ago", days / 30),
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use std::io::{self, Write};
    use std::sync::atomic::{AtomicBool, Ordering};

    use harbormaster_cleaner::{Cleaned, Finding, Group, Identity, Kind, Outcome, Removal, Why};

    use super::{ago, lines};

    #[test]
    fn says_how_long_ago_in_days_then_months() {
        use std::time::Duration;

        assert_eq!(ago(Duration::from_secs(3600)), "today");
        assert_eq!(ago(Duration::from_secs(86_400)), "yesterday");
        assert_eq!(ago(Duration::from_secs(12 * 86_400)), "12 days ago");
        assert_eq!(ago(Duration::from_secs(61 * 86_400)), "2 months ago");
    }

    const GIB: u64 = 1024 * 1024 * 1024;
    const DAY: u64 = 86_400;

    fn finding(
        group: Group,
        kind: Kind,
        name: &str,
        path: &str,
        bytes: u64,
        picked: bool,
    ) -> Finding {
        Finding {
            group,
            kind,
            name: name.into(),
            path: PathBuf::from(path),
            bytes,
            picked,
            in_use: None,
            touched: None,
            project: None,
            emptied: false,
            identity: Identity::default(),
        }
    }

    #[test]
    fn lists_each_group_and_what_picking_would_free() {
        let now = 1000 * DAY;
        let mut shop = finding(
            Group::Projects,
            Kind::Rebuildable,
            "shop · node_modules",
            "/Users/dev/code/shop/node_modules",
            GIB,
            true,
        );
        shop.touched = Some(now - 61 * DAY);
        let mut tool = finding(
            Group::Projects,
            Kind::Rebuildable,
            "tool · target",
            "/Users/dev/code/tool/target",
            GIB,
            false,
        );
        tool.in_use = Some("cargo".into());
        let findings = [
            shop,
            tool,
            finding(
                Group::Packages,
                Kind::Rebuildable,
                "npm",
                "/Users/dev/.npm/_cacache",
                2 * GIB,
                true,
            ),
            finding(
                Group::Xcode,
                Kind::Rebuildable,
                "17.5 (21F79)",
                "/Users/dev/Library/Developer/Xcode/iOS DeviceSupport/17.5 (21F79)",
                GIB,
                false,
            ),
            finding(
                Group::Trash,
                Kind::Discarded,
                "Trash",
                "/Users/dev/.Trash",
                GIB,
                false,
            ),
        ];

        let text = lines(&findings, Path::new("/Users/dev"), now, false).join("\n");

        assert!(
            text.contains("Projects") && text.contains("Packages"),
            "{text}"
        );
        assert!(
            text.contains("npm") && text.contains("2.0 GB") && text.contains("~/.npm/_cacache"),
            "{text}"
        );
        assert!(text.contains("last touched 2 months ago"), "{text}");
        assert!(text.contains("in use by cargo"), "{text}");
        assert!(text.contains("used last"), "{text}");
        assert!(text.contains("empties for good"), "{text}");
        assert!(text.contains("Would free 3.0 GB of 6.0 GB"), "{text}");
        assert!(text.contains("dry run") && text.contains("--yes"), "{text}");

        let removing = lines(&findings, Path::new("/Users/dev"), now, true).join("\n");
        assert!(
            removing.contains("Removing what is picked: 3.0 GB of 6.0 GB."),
            "{removing}"
        );
        assert!(!removing.contains("dry run"), "{removing}");
    }

    #[test]
    fn says_what_removing_did_with_each_and_the_room_freed() {
        let findings = [
            finding(
                Group::Projects,
                Kind::Rebuildable,
                "shop · node_modules",
                "/Users/dev/code/shop/node_modules",
                GIB,
                true,
            ),
            finding(
                Group::Packages,
                Kind::Rebuildable,
                "npm",
                "/Users/dev/.npm/_cacache",
                GIB,
                true,
            ),
            finding(
                Group::Xcode,
                Kind::Personal,
                "2026-09-20",
                "/Users/dev/Library/Developer/Xcode/Archives/2026-09-20",
                GIB,
                true,
            ),
            finding(
                Group::Apps,
                Kind::Rebuildable,
                "com.odd\u{1b}]52;c;eA==\u{7}.app",
                "/Users/dev/Library/Caches/com.odd.app",
                GIB,
                true,
            ),
        ];
        let removal = |at: usize, outcome| Removal {
            path: findings[at].path.clone(),
            outcome,
        };
        let cleaned = Cleaned {
            removals: vec![
                removal(0, Outcome::Deleted),
                removal(
                    1,
                    Outcome::Failed {
                        why: "Permission denied (os error 13)".into(),
                    },
                ),
                removal(2, Outcome::Trashed),
                removal(
                    3,
                    Outcome::Refused {
                        why: Why::InUse {
                            by: "\u{1b}[2Jx".into(),
                        },
                    },
                ),
            ],
            freed_bytes: GIB,
        };

        let said: Vec<String> = cleaned
            .removals
            .iter()
            .map(|removal| super::removal(removal, &findings))
            .collect();

        assert_eq!(said[0], "  deleted  shop · node_modules");
        assert_eq!(said[1], "  failed   npm: Permission denied (os error 13)");
        assert_eq!(said[2], "  trashed  2026-09-20");
        assert!(
            said[3].starts_with("  skipped  com.odd") && !said[3].contains('\u{1b}'),
            "{said:?}"
        );
        assert_eq!(super::freed(&cleaned), "Freed 1.0 GB.");
        assert_eq!(super::status(&cleaned), crate::FAILED);
    }

    #[test]
    fn counts_a_skip_as_the_safety_rules_working_not_a_failure() {
        let cleaned = Cleaned {
            removals: vec![Removal {
                path: PathBuf::from("/Users/dev/.npm/_cacache"),
                outcome: Outcome::Refused {
                    why: Why::InUse { by: "node".into() },
                },
            }],
            freed_bytes: 0,
        };

        assert_eq!(super::status(&cleaned), 0);
        assert_eq!(
            super::freed(&Cleaned {
                removals: Vec::new(),
                freed_bytes: 0
            }),
            "Nothing is picked, so nothing was removed."
        );
    }

    /// Removes as `remove` does: each picked finding while not stopped,
    /// telling of each, and noting in `gone` what it took.
    fn remove_like_the_cleaner(
        gone: &mut Vec<PathBuf>,
        picked: &[Finding],
        stop: &AtomicBool,
        told: &mut dyn FnMut(&Removal),
    ) -> anyhow::Result<Cleaned> {
        let mut removals = Vec::new();
        for finding in picked {
            let outcome = if stop.load(Ordering::Relaxed) {
                Outcome::Stopped
            } else {
                gone.push(finding.path.clone());
                Outcome::Deleted
            };
            let removal = Removal {
                path: finding.path.clone(),
                outcome,
            };
            told(&removal);
            removals.push(removal);
        }
        Ok(Cleaned {
            removals,
            freed_bytes: GIB,
        })
    }

    fn three() -> [Finding; 3] {
        ["a", "b", "c"].map(|name| {
            finding(
                Group::Packages,
                Kind::Rebuildable,
                name,
                &format!("/Users/dev/{name}"),
                GIB,
                true,
            )
        })
    }

    #[test]
    fn removes_what_is_picked_and_tells_of_each() {
        let mut findings = three();
        findings[1].picked = false;
        let mut gone = Vec::new();
        let mut out = Vec::new();

        let status = super::remove_picked(&findings, false, &mut out, |picked, stop, told| {
            remove_like_the_cleaner(&mut gone, picked, stop, told)
        })
        .unwrap();

        let text = String::from_utf8(out).unwrap();
        assert_eq!(status, 0);
        assert_eq!(gone, [findings[0].path.clone(), findings[2].path.clone()]);
        assert_eq!(text, "  deleted  a\n  deleted  c\nFreed 1.0 GB.\n");
    }

    /// `path` and then a unit that isn't text, as a disk may hold in a name.
    fn untextual(path: &str) -> PathBuf {
        #[cfg(unix)]
        let path = {
            use std::os::unix::ffi::OsStrExt;
            std::ffi::OsStr::from_bytes(&[path.as_bytes(), b"\xff"].concat()).to_owned()
        };
        #[cfg(windows)]
        let path = {
            use std::os::windows::ffi::OsStringExt;
            let lone_surrogate = 0xD800;
            let units: Vec<u16> = path.encode_utf16().chain([lone_surrogate]).collect();
            std::ffi::OsString::from_wide(&units)
        };
        PathBuf::from(path)
    }

    #[test]
    fn removes_nothing_when_the_json_could_not_be_written() {
        let mut findings = three();
        findings[2].picked = false;
        findings[2].path = untextual("/Users/");
        let mut out = Vec::new();

        let outcome = super::remove_picked(&findings, true, &mut out, |_, _, _| {
            panic!("nothing may be removed")
        });

        assert!(outcome.is_err());
        assert!(out.is_empty());
    }

    #[test]
    fn stops_removing_once_what_it_did_cant_be_written() {
        struct Closed;
        impl Write for Closed {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let findings = three();
        let mut gone = Vec::new();

        let outcome = super::remove_picked(&findings, false, &mut Closed, |picked, stop, told| {
            remove_like_the_cleaner(&mut gone, picked, stop, told)
        });

        assert!(outcome.is_err());
        assert_eq!(gone, [findings[0].path.clone()]);
    }

    #[test]
    fn says_it_stopped_for_the_output_even_when_removing_then_fails() {
        struct Closed;
        impl Write for Closed {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let findings = three();
        let mut gone = Vec::new();

        let outcome = super::remove_picked(&findings, false, &mut Closed, |picked, stop, told| {
            remove_like_the_cleaner(&mut gone, picked, stop, told)?;
            anyhow::bail!("the disk's free space couldn't be read")
        });

        let error = outcome.unwrap_err();
        assert_eq!(
            error.to_string(),
            "stopped removing: its progress couldn't be written"
        );
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            io::ErrorKind::BrokenPipe
        );
        assert_eq!(gone, [findings[0].path.clone()]);
    }

    #[test]
    fn puts_what_removing_did_in_the_json() {
        let npm = finding(
            Group::Packages,
            Kind::Rebuildable,
            "npm",
            "/Users/dev/.npm/_cacache",
            GIB,
            true,
        );
        let cleaned = Cleaned {
            removals: vec![Removal {
                path: npm.path.clone(),
                outcome: Outcome::Refused {
                    why: Why::InUse { by: "node".into() },
                },
            }],
            freed_bytes: 0,
        };

        let text = super::document(&[npm], Some(&cleaned)).unwrap();
        let json: serde_json::Value = serde_json::from_str(&text).unwrap();

        assert_eq!(json["cleaned"]["freedBytes"], 0);
        assert_eq!(
            json["cleaned"]["removals"][0],
            serde_json::json!({
                "path": "/Users/dev/.npm/_cacache",
                "outcome": "refused",
                "why": {"code": "inUse", "by": "node"},
            })
        );
        assert!(json["findings"][0].get("identity").is_none(), "{text}");
    }

    #[test]
    fn reports_a_path_json_cant_hold_instead_of_panicking() {
        let mut odd = finding(Group::Logs, Kind::Rebuildable, "Logs", "/", 1, true);
        odd.path = untextual("/Users/");

        assert!(super::document(&[odd], None).is_err());
    }

    #[test]
    fn says_so_when_there_is_nothing() {
        assert_eq!(
            lines(&[], Path::new("/Users/dev"), 0, false),
            ["Nothing to clean up."]
        );
    }
}
