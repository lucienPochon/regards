//! `regards`: the Regards command-line tool.

use clap::{Parser, Subcommand};
use regards_core::build_info::{BUILD_ID, VERSION};
use regards_core::paths::DataDirs;
use regards_daemon::{Daemon, DaemonError};
use regards_ipc::protocol::ClientKind;
use regards_ipc::{Client, ClientError, Endpoint};
use serde_json::json;
use std::process::ExitCode;

/// Exit code when the daemon is not running (for `daemon status`).
const EXIT_NOT_RUNNING: u8 = 3;
/// Exit code when another daemon already owns the data directory.
const EXIT_ALREADY_RUNNING: u8 = 4;

#[derive(Parser)]
#[command(name = "regards", version, about = "Regards: a local-first email client")]
struct Cli {
    /// Print machine-readable JSON instead of text.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Control the background service that owns your mail data.
    #[command(subcommand)]
    Daemon(DaemonCommand),
    /// Show the version and build identity.
    Version,
}

#[derive(Subcommand)]
enum DaemonCommand {
    /// Run the daemon in the foreground (Ctrl+C to stop).
    Run,
    /// Report whether the daemon is running.
    Status,
    /// Ask the running daemon to stop.
    Stop,
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli).await {
        Ok(code) => code,
        Err(error) => {
            eprintln!("error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    match cli.command {
        Command::Version => {
            if cli.json {
                println!("{}", json!({ "version": VERSION, "build_id": BUILD_ID }));
            } else {
                println!("regards {VERSION} (build {BUILD_ID})");
            }
            Ok(ExitCode::SUCCESS)
        }
        Command::Daemon(command) => {
            let dirs = DataDirs::resolve()?;
            match command {
                DaemonCommand::Run => daemon_run(dirs).await,
                DaemonCommand::Status => daemon_status(&dirs, cli.json).await,
                DaemonCommand::Stop => daemon_stop(&dirs, cli.json).await,
            }
        }
    }
}

async fn daemon_run(dirs: DataDirs) -> anyhow::Result<ExitCode> {
    init_logging();
    let daemon = match Daemon::bind(dirs).await {
        Ok(daemon) => daemon,
        Err(error @ DaemonError::AlreadyRunning(_)) => {
            eprintln!("{error}");
            return Ok(ExitCode::from(EXIT_ALREADY_RUNNING));
        }
        Err(error) => return Err(error.into()),
    };
    eprintln!(
        "Regards daemon {VERSION} serving {} on {}",
        daemon.dirs().data().display(),
        daemon.endpoint()
    );
    daemon
        .serve(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
    Ok(ExitCode::SUCCESS)
}

async fn daemon_status(dirs: &DataDirs, as_json: bool) -> anyhow::Result<ExitCode> {
    let endpoint = Endpoint::for_dirs(dirs);
    let mut client = match Client::connect(&endpoint, ClientKind::Cli).await {
        Ok(client) => client,
        Err(ClientError::NotRunning { .. }) => {
            if as_json {
                println!("{}", json!({ "running": false, "endpoint": endpoint.to_string() }));
            } else {
                println!("The Regards daemon is not running.");
            }
            return Ok(ExitCode::from(EXIT_NOT_RUNNING));
        }
        Err(error) => return Err(error.into()),
    };
    let health = client.health().await?;
    let same_build = client.is_same_build();
    if as_json {
        println!(
            "{}",
            json!({
                "running": true,
                "endpoint": endpoint.to_string(),
                "version": health.version,
                "build_id": health.build_id,
                "same_build_as_cli": same_build,
                "pid": health.pid,
                "uptime_secs": health.uptime_secs,
            })
        );
    } else {
        println!(
            "The Regards daemon is running (version {}, pid {}, up {}s).",
            health.version, health.pid, health.uptime_secs
        );
        if !same_build {
            println!(
                "Note: it was built from a different build ({}) than this CLI ({BUILD_ID}). \
                 Run `regards daemon stop` and start it again to update it.",
                health.build_id
            );
        }
    }
    Ok(ExitCode::SUCCESS)
}

async fn daemon_stop(dirs: &DataDirs, as_json: bool) -> anyhow::Result<ExitCode> {
    let endpoint = Endpoint::for_dirs(dirs);
    let stopped = match Client::connect(&endpoint, ClientKind::Cli).await {
        Ok(mut client) => {
            client.shutdown().await?;
            true
        }
        Err(ClientError::NotRunning { .. }) => false,
        Err(error) => return Err(error.into()),
    };
    if as_json {
        println!("{}", json!({ "stopped": stopped, "was_running": stopped }));
    } else if stopped {
        println!("The Regards daemon is stopping.");
    } else {
        println!("The Regards daemon was not running.");
    }
    Ok(ExitCode::SUCCESS)
}

fn init_logging() {
    let filter = tracing_subscriber::EnvFilter::try_from_env("REGARDS_LOG")
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .init();
}
