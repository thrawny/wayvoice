mod daemon;
mod debug_recordings;
mod hud;
mod inject;
mod ipc;
mod oneshot;

use clap::{Parser, Subcommand};
use daemon::Daemon;
use ipc::{run_server, send_command};
use log::{debug, warn};
use oneshot::run_once;
use std::process::Stdio;
use std::sync::Arc;
use tokio::sync::Mutex;
use wayvoice::config::load_config;

#[derive(Parser)]
#[command(name = "wayvoice", about = "Voice-to-text for Wayland")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the daemon
    Serve,
    /// Toggle recording on/off
    Toggle,
    /// Cancel current operation
    Cancel,
    /// Get current status
    Status,
    /// One-shot: record until Enter, transcribe, print to stdout
    Once,
    /// HUD popup for recording/transcribing state
    Hud,
    /// Show HUD in preview mode without daemon state
    HudPreview,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let cli = Cli::parse();

    match cli.command {
        Commands::Serve => {
            let config = load_config();

            if config.hud {
                spawn_hud();
            }

            let daemon = Arc::new(Mutex::new(Daemon::new(config)));

            let daemon_for_signal = daemon.clone();
            tokio::spawn(async move {
                let _ = tokio::signal::ctrl_c().await;
                let mut d = daemon_for_signal.lock().await;
                let _ = d.cancel().await;
                std::process::exit(0);
            });

            if let Err(e) = run_server(daemon).await {
                eprintln!("Server error: {e}");
                std::process::exit(1);
            }
        }
        Commands::Toggle => match send_command("toggle").await {
            Ok(response) => println!("{response}"),
            Err(e) => {
                eprintln!("Failed to connect: {e} (is daemon running?)");
                std::process::exit(1);
            }
        },
        Commands::Cancel => match send_command("cancel").await {
            Ok(response) => println!("{response}"),
            Err(e) => {
                eprintln!("Failed to connect: {e}");
                std::process::exit(1);
            }
        },
        Commands::Status => match send_command("status").await {
            Ok(response) => println!("{response}"),
            Err(e) => {
                eprintln!("Failed to connect: {e}");
                std::process::exit(1);
            }
        },
        Commands::Once => {
            let config = load_config();
            run_once(config).await;
        }
        Commands::Hud => {
            hud::run_hud();
        }
        Commands::HudPreview => {
            hud::run_hud_preview();
        }
    }
}

fn spawn_hud() {
    if !hud::is_supported() {
        warn!("HUD disabled: binary built without hud-ui feature");
        return;
    }

    let exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(e) => {
            warn!("Failed to resolve current executable for HUD spawn: {e}");
            return;
        }
    };

    match std::process::Command::new(exe)
        .arg("hud")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_) => debug!("HUD process spawned"),
        Err(e) => warn!("Failed to spawn HUD process: {e}"),
    }
}
