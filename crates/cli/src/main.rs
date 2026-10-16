mod format;
mod stop;
mod why;

use std::collections::HashMap;
use std::io::{BufRead, IsTerminal, Write};
use std::net::IpAddr;
use std::path::Path;
use std::process::ExitCode;
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use clap::{Args, Parser, Subcommand};
use harbormaster_engine::{
    Classification, Input, Outcome, Scanner, Service, Signals, Snapshot, Standing, Target,
};
use harbormaster_platform::{ProcessKey, Signal};

/// How long to wait between the two samples a CPU reading needs.
const CPU_SAMPLE_GAP: Duration = Duration::from_millis(400);
/// How long a forced process gets to disappear.
const KILL_GRACE: Duration = Duration::from_secs(2);
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

fn explain(port: u16) -> anyhow::Result<u8> {
    let home = std::env::home_dir().context("could not find the home folder")?;
    let snapshot = scan(&mut Scanner::default(), &home)?;
    let report =
        harbormaster_engine::explain_port(port, &snapshot, &harbormaster_platform::probe_bind);
    for line in why::lines(port, &report) {
        println!("{line}");
    }
    Ok(why::status(&report))
}

fn halt(target: Target, yes: bool, force: bool, grace: Duration) -> anyhow::Result<u8> {
    let home = std::env::home_dir().context("could not find the home folder")?;
    let snapshot = scan(&mut Scanner::default(), &home)?;
    let service = match harbormaster_engine::choose(&snapshot, &target, &lineage()?) {
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
    let current = scan(&mut Scanner::default(), &home)?;
    let guard = guards(&home, &approved)?;
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
        let current = scan(&mut Scanner::default(), &home)?;
        let guard = guards(&home, &approved)?;
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

/// Where each approved process stands right now.
fn guards(home: &Path, keys: &[ProcessKey]) -> anyhow::Result<HashMap<ProcessKey, Standing>> {
    let processes = harbormaster_platform::processes()?;
    let details = harbormaster_platform::details;
    let input = Input {
        listeners: &[],
        processes: &processes,
        details: &details,
        home,
        now: Instant::now(),
    };
    Ok(
        harbormaster_engine::standings(input, keys, &harbormaster_platform::presence)
            .into_iter()
            .collect(),
    )
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

/// This process and everything above it, which a stop must never reach.
fn lineage() -> anyhow::Result<Vec<u32>> {
    let parents: HashMap<u32, u32> = harbormaster_platform::processes()?
        .iter()
        .filter_map(|entry| entry.parent.map(|parent| (entry.key.pid.0, parent.0)))
        .collect();
    lineage_from(std::process::id(), &parents)
        .context("could not trace which processes started Harbormaster")
}

/// The chain from `pid` up to launchd. A gap could hide an ancestor, so a
/// chain that doesn't get there is no answer.
fn lineage_from(pid: u32, parents: &HashMap<u32, u32>) -> Option<Vec<u32>> {
    let mut lineage = vec![pid];
    let mut current = pid;
    while current > 1 {
        let parent = *parents.get(&current)?;
        if lineage.contains(&parent) {
            return None;
        }
        lineage.push(parent);
        current = parent;
    }
    Some(lineage)
}

/// Two scans a moment apart, so CPU use can be measured.
fn sample() -> anyhow::Result<Snapshot> {
    let home = std::env::home_dir().context("could not find the home folder")?;
    let mut scanner = Scanner::default();
    scan(&mut scanner, &home)?;
    sleep(CPU_SAMPLE_GAP);
    scan(&mut scanner, &home)
}

fn scan(scanner: &mut Scanner, home: &Path) -> anyhow::Result<Snapshot> {
    let processes = harbormaster_platform::processes()?;
    let listeners = harbormaster_platform::listeners()?;
    let details = harbormaster_platform::details;
    Ok(scanner.scan(Input {
        listeners: &listeners,
        processes: &processes,
        details: &details,
        home,
        now: Instant::now(),
    }))
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use harbormaster_engine::{Classification, Project, Service};

    use std::collections::HashMap;

    use super::{Cli, Command, NAME_WIDTH, lineage_from, row};

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
    fn traces_harbormaster_up_to_launchd_or_not_at_all() {
        let parents = HashMap::from([(77, 40), (40, 20), (20, 1)]);
        assert_eq!(lineage_from(77, &parents), Some(vec![77, 40, 20, 1]));

        let gap = HashMap::from([(77, 40), (40, 20)]);
        assert_eq!(lineage_from(77, &gap), None);

        let cycle = HashMap::from([(77, 40), (40, 77)]);
        assert_eq!(lineage_from(77, &cycle), None);
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
