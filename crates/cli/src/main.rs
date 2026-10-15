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
    for service in services {
        let mut row = vec![
            ports(service),
            service
                .project
                .as_ref()
                .map_or("-".into(), |project| project.name.clone()),
            service
                .project
                .as_ref()
                .and_then(|project| project.branch.clone())
                .unwrap_or_default(),
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
        rows.push(row);
    }
    print!("{}", format::table(&rows));
    Ok(0)
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

    use super::{Cli, Command};

    #[test]
    fn takes_the_list_flags_without_naming_the_list_command() {
        let cli = Cli::try_parse_from(["harbormaster", "--all", "--json"]).unwrap();

        assert!(cli.command.is_none());
        assert!(cli.list.all && cli.list.json);
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
