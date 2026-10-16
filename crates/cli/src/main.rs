mod clean;
mod format;
mod stop;
mod why;

use std::io::{BufRead, IsTerminal, Write};
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;
use std::thread::sleep;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use clap::{Args, Parser, Subcommand};
use harbormaster_cleaner::Survey;
use harbormaster_engine::{
    Classification, Outcome, Scanner, Service, Signals, Snapshot, Standing, Target, live,
};
use harbormaster_platform::{ProcessKey, Signal};

/// How long to wait between the two samples a CPU reading needs.
const CPU_SAMPLE_GAP: Duration = Duration::from_millis(400);
/// How long a forced process gets to disappear.
const KILL_GRACE: Duration = Duration::from_secs(2);
/// How long a project sits untouched before its build outputs are picked.
const IDLE_AFTER: Duration = Duration::from_secs(30 * 86_400);
const COMMAND_WIDTH: usize = 60;
const NAME_WIDTH: usize = 32;

#[derive(Debug, Parser)]
#[command(
    name = "harbormaster",
    version,
    about = "See the dev servers running on this machine.",
    args_conflicts_with_subcommands = true
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    /// Listing is the default, so its flags work on their own too.
    #[command(flatten)]
    list: ListArgs,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List dev servers (the default)
    Ls(ListArgs),
    /// Explain who holds a port, or why it can't be used
    Why {
        /// The port to check
        #[arg(value_parser = clap::value_parser!(u16).range(1..))]
        port: u16,
    },
    /// Stop the server on a port, after showing what will stop
    Stop {
        /// The port the server listens on
        #[arg(
            value_parser = clap::value_parser!(u16).range(1..),
            required_unless_present = "pid"
        )]
        port: Option<u16>,
        /// Stop the server this process belongs to instead
        #[arg(long, conflicts_with_all = ["port", "address"])]
        pid: Option<u32>,
        /// Pick the server on this address when several share the port
        #[arg(long)]
        address: Option<IpAddr>,
        /// Stop without asking
        #[arg(long, short)]
        yes: bool,
        /// End whatever is still running once the wait is over
        #[arg(long)]
        force: bool,
        /// Seconds to wait for the server to exit after asking it to
        #[arg(long, default_value_t = 5, value_name = "SECONDS")]
        wait: u64,
    },
    /// Show the caches, logs and other leftovers that could go; removes nothing yet
    Clean {
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Debug, Clone, Copy, Args)]
struct ListArgs {
    /// Include listeners that are not dev servers
    #[arg(long)]
    all: bool,
    /// Print machine-readable JSON
    #[arg(long)]
    json: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let outcome = match cli.command {
        None => list(cli.list),
        Some(Command::Ls(args)) => list(args),
        Some(Command::Why { port }) => explain(port),
        Some(Command::Clean { json }) => survey(json),
        Some(Command::Stop {
            port,
            pid,
            address,
            yes,
            force,
            wait,
        }) => {
            let grace = Duration::from_secs(wait);
            match (port, pid) {
                (Some(port), _) => halt(Target::Port { port, address }, yes, force, grace),
                (None, Some(pid)) => halt(Target::Pid(pid), yes, force, grace),
                (None, None) => Ok(2),
            }
        }
    };
    match outcome {
        Ok(status) => ExitCode::from(status),
        Err(error) => {
            eprintln!("harbormaster: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn list(ListArgs { all, json }: ListArgs) -> anyhow::Result<u8> {
    let snapshot = sample()?;
    let services: Vec<&Service> = snapshot
        .services
        .iter()
        .filter(|service| all || service.classification == Classification::Dev)
        .collect();

    if json {
        let document = serde_json::json!({ "schema": "harbormaster/v1", "services": services });
        println!("{}", serde_json::to_string_pretty(&document)?);
        return Ok(0);
    }
    if services.is_empty() {
        println!("No dev servers are listening.");
        return Ok(0);
    }

    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let mut rows = vec![
        ["PORT", "PROJECT", "BRANCH", "MEM", "CPU", "UP", "COMMAND"]
            .map(String::from)
            .to_vec(),
    ];
    if all {
        rows[0].insert(0, "KIND".into());
    }
    rows.extend(services.into_iter().map(|service| row(service, now, all)));
    print!("{}", format::table(&rows));
    Ok(0)
}

fn row(service: &Service, now: u64, all: bool) -> Vec<String> {
    let project = service.project.as_ref();
    let mut row = vec![
        format::ports(service),
        format::truncate(project.map_or("-", |project| &project.name), NAME_WIDTH),
        format::truncate(
            project
                .and_then(|project| project.branch.as_deref())
                .unwrap_or_default(),
            NAME_WIDTH,
        ),
        service.memory_bytes.map(format::bytes).unwrap_or_default(),
        service
            .cpu_percent
            .map(|cpu| format!("{cpu:.1}%"))
            .unwrap_or_default(),
        format::uptime(Duration::from_secs(now.saturating_sub(service.started_at))),
        format::truncate(
            service.command.as_deref().unwrap_or_default(),
            COMMAND_WIDTH,
        ),
    ];
    if all {
        row.insert(0, format!("{:?}", service.classification).to_lowercase());
    }
    row
}

fn survey(json: bool) -> anyhow::Result<u8> {
    let home = harbormaster_cleaner::account_home().context("could not find the home folder")?;
    let roots = harbormaster_cleaner::usual_roots(&home);
    let busy = busy_paths().context("could not tell which folders are in use")?;
    let running = running_projects(&home)?;
    let now = SystemTime::now();
    let survey = Survey {
        home: &home,
        roots: &roots,
        busy: &busy,
        running: &running,
        idle_after: IDLE_AFTER,
        now,
    };
    let findings = harbormaster_cleaner::scan(&survey, &AtomicBool::new(false))
        .with_context(|| format!("could not look through {}", home.display()))?;
    if json {
        println!("{}", clean::document(&findings)?);
        return Ok(0);
    }
    let now = now.duration_since(UNIX_EPOCH)?.as_secs();
    for line in clean::lines(&findings, &home, now) {
        println!("{line}");
    }
    Ok(0)
}

/// Every path this user's other processes hold, each with its process's name.
fn busy_paths() -> anyhow::Result<Vec<(PathBuf, String)>> {
    let own = std::process::id();
    Ok(harbormaster_platform::whereabouts()?
        .into_iter()
        .filter(|process| process.pid.0 != own)
        .flat_map(|process| {
            let name = process.name;
            process
                .paths
                .into_iter()
                .map(move |path| (path, name.clone()))
        })
        .collect())
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

fn explain(port: u16) -> anyhow::Result<u8> {
    let home = std::env::home_dir().context("could not find the home folder")?;
    let snapshot = live::scan(&mut Scanner::default(), &home)?;
    let report =
        harbormaster_engine::explain_port(port, &snapshot, &harbormaster_platform::probe_bind);
    for line in why::lines(port, &report) {
        println!("{line}");
    }
    Ok(why::status(&report))
}

fn halt(target: Target, yes: bool, force: bool, grace: Duration) -> anyhow::Result<u8> {
    when_exec_tracked(harbormaster_platform::tracks_exec(), || {
        halt_tracked(target, yes, force, grace)
    })
}

/// Every check before a signal relies on keys that change on exec. Without
/// them, nothing is scanned, asked or signalled.
fn when_exec_tracked(
    tracked: bool,
    proceed: impl FnOnce() -> anyhow::Result<u8>,
) -> anyhow::Result<u8> {
    if !tracked {
        return Ok(refuse(stop::untracked_exec()));
    }
    proceed()
}

fn halt_tracked(target: Target, yes: bool, force: bool, grace: Duration) -> anyhow::Result<u8> {
    let home = std::env::home_dir().context("could not find the home folder")?;
    let snapshot = live::scan(&mut Scanner::default(), &home)?;
    let own = live::lineage()?.context("could not trace which processes started Harbormaster")?;
    let service = match harbormaster_engine::choose(&snapshot, &target, &own) {
        Ok(service) => service,
        Err(refused) => return Ok(refuse(stop::refusal(&refused, &target))),
    };
    for line in stop::preview(service) {
        println!("{line}");
    }
    let count = service.processes.len();
    let noun = if count == 1 { "process" } else { "processes" };
    if !yes && !ask(&format!("Stop {count} {noun}? [y/N] "))? {
        println!("Nothing was stopped.");
        return Ok(stop::CANCELLED);
    }

    // What was approved is checked against a fresh look right before any signal.
    let approved: Vec<ProcessKey> = service.processes.iter().map(|member| member.key).collect();
    let current = live::scan(&mut Scanner::default(), &home)?;
    let guard = live::guards(&home, &approved)?;
    let targets = match harbormaster_engine::recheck(service, &current, &|key| {
        guard.get(&key).copied().unwrap_or(Standing::Unknown)
    }) {
        Ok(targets) => targets,
        Err(refused) => return Ok(refuse(stop::refusal(&refused, &target))),
    };
    let signals = Signals {
        send: &harbormaster_platform::signal,
        presence: &harbormaster_platform::presence,
        sleep: &sleep,
    };
    let mut outcomes = harbormaster_engine::stop(&targets, Signal::Terminate, grace, &signals);
    let left: Vec<ProcessKey> = outcomes
        .iter()
        .filter(|(_, outcome)| *outcome == Outcome::StillRunning)
        .map(|(key, _)| *key)
        .collect();
    let forced = !left.is_empty()
        && (force
            || (!yes
                && std::io::stdin().is_terminal()
                && ask(&format!(
                    "{} still running after {} s. End them now? [y/N] ",
                    left.len(),
                    grace.as_secs()
                ))?));
    if forced {
        // The wait, and the question, gave the server time to change.
        let current = live::scan(&mut Scanner::default(), &home)?;
        let guard = live::guards(&home, &approved)?;
        if let Err(refused) = harbormaster_engine::recheck(service, &current, &|key| {
            guard.get(&key).copied().unwrap_or(Standing::Unknown)
        }) {
            for line in stop::report(service, &outcomes).0 {
                println!("{line}");
            }
            return Ok(refuse(stop::refusal(&refused, &target)));
        }
        for (key, outcome) in harbormaster_engine::stop(&left, Signal::Kill, KILL_GRACE, &signals) {
            if let Some(slot) = outcomes.iter_mut().find(|(target, _)| *target == key) {
                slot.1 = outcome;
            }
        }
    }

    let (lines, status) = stop::report(service, &outcomes);
    for line in lines {
        println!("{line}");
    }
    if status == stop::PARTIAL && !forced && !left.is_empty() {
        println!("Run it again with --force to end what is left.");
    }
    if status == 0 {
        let listening = harbormaster_platform::listeners()?;
        let free = service
            .ports
            .iter()
            .all(|port| !listening.iter().any(|listener| listener.port == port.port));
        if free {
            println!("{} is free now.", format::ports(service));
        }
    }
    Ok(status)
}

fn refuse((lines, status): (Vec<String>, u8)) -> u8 {
    for line in lines {
        eprintln!("{line}");
    }
    status
}

/// Asks on the terminal; anything but a clear yes, or no answer at all, is a no.
fn ask(question: &str) -> anyhow::Result<bool> {
    print!("{question}");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    if std::io::stdin().lock().read_line(&mut answer)? == 0 {
        // No answer is coming; end the prompt's line.
        println!();
    }
    Ok(stop::confirmed(&answer))
}

/// Two scans a moment apart, so CPU use can be measured.
fn sample() -> anyhow::Result<Snapshot> {
    let home = std::env::home_dir().context("could not find the home folder")?;
    let mut scanner = Scanner::default();
    live::scan(&mut scanner, &home)?;
    sleep(CPU_SAMPLE_GAP);
    Ok(live::scan(&mut scanner, &home)?)
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use harbormaster_engine::{Classification, Project, Service};

    use super::{Cli, Command, NAME_WIDTH, row, stop, when_exec_tracked};

    #[test]
    fn refuses_before_anything_is_scanned_when_keys_cant_follow_an_exec() {
        let refused = when_exec_tracked(false, || panic!("nothing may be scanned or asked"));
        assert_eq!(refused.unwrap(), stop::REFUSED);
        assert_eq!(when_exec_tracked(true, || Ok(17)).unwrap(), 17);
    }

    #[test]
    fn takes_the_list_flags_without_naming_the_list_command() {
        let cli = Cli::try_parse_from(["harbormaster", "--all", "--json"]).unwrap();

        assert!(cli.command.is_none());
        assert!(cli.list.all && cli.list.json);
    }

    #[test]
    fn keeps_long_project_and_branch_names_from_widening_the_table() {
        let service = Service {
            id: "1-1".into(),
            classification: Classification::Dev,
            protected: None,
            ports: Vec::new(),
            project: Some(Project {
                name: "n".repeat(100),
                root: "/x".into(),
                repo_root: None,
                framework: None,
                branch: Some("b".repeat(255)),
                worktree: false,
            }),
            command: None,
            started_at: 0,
            memory_bytes: None,
            cpu_percent: None,
            processes: Vec::new(),
            holders: Vec::new(),
            ancestors: Vec::new(),
        };

        let row = row(&service, 0, false);

        assert_eq!(row[1].chars().count(), NAME_WIDTH);
        assert_eq!(row[2].chars().count(), NAME_WIDTH);
    }

    #[test]
    fn rejects_port_zero() {
        assert!(Cli::try_parse_from(["harbormaster", "why", "0"]).is_err());
    }

    #[test]
    fn still_takes_them_after_ls() {
        let cli = Cli::try_parse_from(["harbormaster", "ls", "--all"]).unwrap();

        assert!(matches!(cli.command, Some(Command::Ls(args)) if args.all && !args.json));
    }
}
