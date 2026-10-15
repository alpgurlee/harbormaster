mod format;

use std::path::Path;
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::Context;
use clap::{Parser, Subcommand};
use harbormaster_engine::{Classification, Input, Scanner, Service, Snapshot};

/// How long to wait between the two samples a CPU reading needs.
const CPU_SAMPLE_GAP: Duration = Duration::from_millis(400);
const COMMAND_WIDTH: usize = 60;

#[derive(Debug, Parser)]
#[command(
    name = "harbormaster",
    version,
    about = "See the dev servers running on this machine."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// List dev servers (the default)
    Ls {
        /// Include listeners that are not dev servers
        #[arg(long)]
        all: bool,
        /// Print machine-readable JSON
        #[arg(long)]
        json: bool,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command.unwrap_or(Command::Ls {
        all: false,
        json: false,
    }) {
        Command::Ls { all, json } => list(all, json),
    }
}

fn list(all: bool, json: bool) -> anyhow::Result<()> {
    let snapshot = sample()?;
    let services: Vec<&Service> = snapshot
        .services
        .iter()
        .filter(|service| all || service.classification == Classification::Dev)
        .collect();

    if json {
        let document = serde_json::json!({ "schema": "harbormaster/v1", "services": services });
        println!("{}", serde_json::to_string_pretty(&document)?);
        return Ok(());
    }
    if services.is_empty() {
        println!("No dev servers are listening.");
        return Ok(());
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
    Ok(())
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
