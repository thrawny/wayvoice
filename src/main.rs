mod daemon;
mod debug_recordings;
mod hud;
mod inject;
mod ipc;
mod oneshot;

use clap::{Parser, Subcommand};
use daemon::Daemon;
use ipc::{run_server, send_command};
use log::debug;
use notify::{Event, EventKind, RecursiveMode, Watcher};
use oneshot::run_once;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use wayvoice::config::{config_path, load_config, try_load_config, upsert_replacement};

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
    /// Manage text replacements in the user config
    Replace {
        #[command(subcommand)]
        command: ReplaceCommands,
    },
    /// HUD popup for recording/transcribing state
    Hud,
    /// Show HUD in preview mode without daemon state
    HudPreview,
}

#[derive(Subcommand)]
enum ReplaceCommands {
    /// Add or update a replacement in ~/.config/wayvoice/config.toml
    Add {
        /// Match inside words by storing the rule as a substring replacement
        #[arg(long)]
        substring: bool,
        /// Text to replace
        from: String,
        /// Replacement text
        to: String,
    },
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
            start_config_watcher(daemon.clone());

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
        Commands::Replace { command } => match command {
            ReplaceCommands::Add {
                substring,
                from,
                to,
            } => {
                let key = if substring && !from.starts_with('~') {
                    format!("~{from}")
                } else {
                    from
                };

                match upsert_replacement(&key, &to) {
                    Ok(path) => {
                        println!(
                            "Saved replacement \"{key}\" = \"{to}\" in {}",
                            path.display()
                        );
                    }
                    Err(err) => {
                        eprintln!("Failed to update replacement: {err}");
                        std::process::exit(1);
                    }
                }
            }
        },
        Commands::Hud => {
            hud::run_hud();
        }
        Commands::HudPreview => {
            hud::run_hud_preview();
        }
    }
}

fn spawn_hud() {
    let exe = match std::env::current_exe() {
        Ok(path) => path,
        Err(e) => {
            log::warn!("Failed to resolve current executable for HUD spawn: {e}");
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
        Err(e) => log::warn!("Failed to spawn HUD process: {e}"),
    }
}

fn start_config_watcher(daemon: Arc<Mutex<Daemon>>) {
    tokio::spawn(async move {
        let path = config_path();
        let watch_root = config_watch_root(&path);

        if let Err(e) = std::fs::create_dir_all(&watch_root) {
            log::warn!(
                "Failed to create config watch directory {}: {e}",
                watch_root.display()
            );
            return;
        }

        let (tx, mut rx) = mpsc::unbounded_channel();
        let watched_path = path.clone();
        let log_path = path.clone();
        let mut watcher =
            match notify::recommended_watcher(move |result: notify::Result<Event>| match result {
                Ok(event) if is_config_reload_event(&event, &watched_path) => {
                    let _ = tx.send(());
                }
                Ok(_) => {}
                Err(e) => {
                    log::warn!("Config watcher error for {}: {e}", log_path.display());
                }
            }) {
                Ok(watcher) => watcher,
                Err(e) => {
                    log::warn!("Failed to start config watcher for {}: {e}", path.display());
                    return;
                }
            };

        if let Err(e) = watcher.watch(&watch_root, RecursiveMode::NonRecursive) {
            log::warn!(
                "Failed to watch config directory {}: {e}",
                watch_root.display()
            );
            return;
        }

        debug!("Watching config changes at {}", path.display());

        while rx.recv().await.is_some() {
            while rx.try_recv().is_ok() {}

            match try_load_config() {
                Ok(config) => {
                    let should_spawn_hud = {
                        let mut daemon = daemon.lock().await;
                        daemon.reload_config(config)
                    };

                    if should_spawn_hud {
                        spawn_hud();
                    }
                }
                Err(e) => {
                    log::warn!(
                        "Ignoring config reload for {} because parsing failed: {e}",
                        path.display()
                    );
                }
            }
        }
    });
}

fn config_watch_root(path: &Path) -> PathBuf {
    path.parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

fn is_config_reload_event(event: &Event, target: &Path) -> bool {
    matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    ) && event.paths.iter().any(|path| is_config_path(path, target))
}

fn is_config_path(path: &Path, target: &Path) -> bool {
    path == target || (path.file_name() == target.file_name() && path.parent() == target.parent())
}

#[cfg(test)]
mod tests {
    use super::{config_watch_root, is_config_path, is_config_reload_event};
    use notify::event::{CreateKind, ModifyKind, RemoveKind};
    use notify::{Event, EventKind};
    use std::path::Path;

    #[test]
    fn matches_config_paths_by_location() {
        let target = Path::new("/tmp/wayvoice/config.toml");

        assert!(is_config_path(
            Path::new("/tmp/wayvoice/config.toml"),
            target
        ));
        assert!(!is_config_path(Path::new("/tmp/other/config.toml"), target));
    }

    #[test]
    fn config_reload_events_ignore_unrelated_paths() {
        let target = Path::new("/tmp/wayvoice/config.toml");

        let related = Event {
            kind: EventKind::Modify(ModifyKind::Any),
            paths: vec![target.to_path_buf()],
            attrs: Default::default(),
        };
        let unrelated = Event {
            kind: EventKind::Create(CreateKind::Any),
            paths: vec![Path::new("/tmp/other/config.toml").to_path_buf()],
            attrs: Default::default(),
        };
        let access = Event {
            kind: EventKind::Remove(RemoveKind::Any),
            paths: vec![target.to_path_buf()],
            attrs: Default::default(),
        };

        assert!(is_config_reload_event(&related, target));
        assert!(!is_config_reload_event(&unrelated, target));
        assert!(is_config_reload_event(&access, target));
    }

    #[test]
    fn watch_root_uses_parent_directory() {
        let root = config_watch_root(Path::new("/tmp/nested/wayvoice/config.toml"));
        assert_eq!(root, Path::new("/tmp/nested/wayvoice"));
    }
}
