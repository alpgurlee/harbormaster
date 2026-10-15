mod format;
mod why;

use std::path::Path;
use std::process::ExitCode;
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use clap::{Args, Parser, Subcommand};
use harbormaster_engine::{Classification, Input, Scanner, Service, Snapshot};

/// How long to wait between the two samples a CPU reading needs.
const CPU_SAMPLE_GAP: Duration = Duration::from_millis(400);
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
        ports(service),
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

fn ports(service: &Service) -> String {
    let mut ports: Vec<u16> = service.ports.iter().map(|port| port.port).collect();
    ports.dedup();
    ports
        .iter()
        .map(|port| format!(":{port}"))
        .collect::<Vec<_>>()
        .join(", ")
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

    use super::{Cli, Command, NAME_WIDTH, row};

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
