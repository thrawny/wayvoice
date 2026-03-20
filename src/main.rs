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
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;
use tokio::sync::{Mutex, mpsc};
use wayvoice::config::{
    append_keyword, config_path, load_config, try_load_config, upsert_replacement,
};

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
    /// Manage prompt keywords in the user config
    Keyword {
        #[command(subcommand)]
        command: KeywordCommands,
    },
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
enum KeywordCommands {
    /// Add a keyword to the transcription prompt in ~/.config/wayvoice/config.toml
    Add {
        /// Keyword or phrase to bias transcription toward
        keyword: String,
    },
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

const CONFIG_RELOAD_DEBOUNCE_MS: u64 = 100;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let cli = Cli::parse();

    match cli.command {
        Commands::Serve => {
            let config = load_config();
            let keywords = config
                .keywords
                .iter()
                .chain(config.extra_keywords.iter())
                .cloned()
                .collect::<Vec<_>>();
            debug!("merged keywords: {:?}", keywords);
            debug!("merged replacements: {:?}", config.replacements);

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
        Commands::Keyword { command } => match command {
            KeywordCommands::Add { keyword } => match append_keyword(&keyword) {
                Ok(path) => {
                    println!("Saved keyword \"{keyword}\" in {}", path.display());
                }
                Err(err) => {
                    eprintln!("Failed to update keyword: {err}");
                    std::process::exit(1);
                }
            },
        },
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
        let watch_state = Arc::new(StdMutex::new(ConfigWatchState::discover(&path)));
        let initial_state = clone_watch_state(&watch_state);

        if let Err(e) = std::fs::create_dir_all(config_watch_root(&path)) {
            log::warn!(
                "Failed to create config watch directory {}: {e}",
                config_watch_root(&path).display()
            );
            return;
        }

        let (tx, mut rx) = mpsc::unbounded_channel();
        let log_path = path.clone();
        let callback_watch_state = watch_state.clone();
        let mut watcher =
            match notify::recommended_watcher(move |result: notify::Result<Event>| match result {
                Ok(event) => {
                    if clone_watch_state(&callback_watch_state).matches_event(&event) {
                        let _ = tx.send(());
                    }
                }
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

        if !watch_config_roots(&mut watcher, &initial_state.roots) {
            return;
        }

        debug!("Watching config changes at {}", path.display());

        while rx.recv().await.is_some() {
            // Coalesce truncate/write bursts so reload sees the settled file contents.
            tokio::time::sleep(Duration::from_millis(CONFIG_RELOAD_DEBOUNCE_MS)).await;
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

            sync_config_watch_state(&mut watcher, &watch_state);
        }
    });
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ConfigWatchState {
    configured_path: PathBuf,
    resolved_path: Option<PathBuf>,
    roots: Vec<PathBuf>,
}

impl ConfigWatchState {
    fn discover(path: &Path) -> Self {
        let configured_path = path.to_path_buf();
        let resolved_path = resolved_config_path(path);
        let mut roots = vec![config_watch_root(path).to_path_buf()];

        if let Some(resolved_path) = &resolved_path {
            push_unique_path(&mut roots, config_watch_root(resolved_path).to_path_buf());
        }

        Self {
            configured_path,
            resolved_path,
            roots,
        }
    }

    fn matches_event(&self, event: &Event) -> bool {
        matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
        ) && event.paths.iter().any(|path| self.matches_path(path))
    }

    fn matches_path(&self, path: &Path) -> bool {
        is_config_path(path, &self.configured_path)
            || self
                .resolved_path
                .as_deref()
                .is_some_and(|resolved_path| is_config_path(path, resolved_path))
    }
}

fn config_watch_root(path: &Path) -> PathBuf {
    path.parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

#[cfg(test)]
fn is_config_reload_event(event: &Event, target: &Path) -> bool {
    matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    ) && event.paths.iter().any(|path| is_config_path(path, target))
}

fn is_config_path(path: &Path, target: &Path) -> bool {
    path == target || (path.file_name() == target.file_name() && path.parent() == target.parent())
}

fn resolved_config_path(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path)
        .ok()
        .filter(|resolved| resolved != path)
}

fn push_unique_path(paths: &mut Vec<PathBuf>, candidate: PathBuf) {
    if !paths.iter().any(|path| path == &candidate) {
        paths.push(candidate);
    }
}

fn clone_watch_state(watch_state: &StdMutex<ConfigWatchState>) -> ConfigWatchState {
    watch_state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner())
        .clone()
}

fn sync_config_watch_state<W: Watcher>(watcher: &mut W, watch_state: &StdMutex<ConfigWatchState>) {
    let current = clone_watch_state(watch_state);
    let next = ConfigWatchState::discover(&current.configured_path);
    if current == next {
        return;
    }

    let added_roots = next
        .roots
        .iter()
        .filter(|root| !current.roots.iter().any(|candidate| candidate == *root))
        .cloned()
        .collect::<Vec<_>>();
    if !watch_config_roots(watcher, &added_roots) {
        return;
    }

    for root in &current.roots {
        if next.roots.iter().any(|candidate| candidate == root) {
            continue;
        }

        if let Err(e) = watcher.unwatch(root) {
            log::warn!(
                "Failed to stop watching config directory {}: {e}",
                root.display()
            );
        }
    }

    *watch_state
        .lock()
        .unwrap_or_else(|poison| poison.into_inner()) = next;
}

fn watch_config_roots<W: Watcher>(watcher: &mut W, roots: &[PathBuf]) -> bool {
    for root in roots {
        if let Err(e) = watcher.watch(root, RecursiveMode::NonRecursive) {
            log::warn!("Failed to watch config directory {}: {e}", root.display());
            return false;
        }
    }

    true
}

#[cfg(test)]
mod tests {
    use super::{
        ConfigWatchState, config_watch_root, is_config_path, is_config_reload_event,
        resolved_config_path,
    };
    use notify::event::{CreateKind, ModifyKind, RemoveKind};
    use notify::{Event, EventKind};
    use std::path::{Path, PathBuf};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_config_path(test_name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir()
            .join(format!(
                "wayvoice-main-{test_name}-{}-{unique}",
                std::process::id()
            ))
            .join("wayvoice")
            .join("config.toml")
    }

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

    #[test]
    fn resolved_config_path_follows_symlinks() {
        let path = temp_config_path("resolve");
        let target = path
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("source")
            .join("config.toml");

        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, "hud = false\n").unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();

        assert_eq!(resolved_config_path(&path), Some(target.clone()));

        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn config_watch_state_includes_symlink_target_root() {
        let path = temp_config_path("roots");
        let configured_root = path.parent().unwrap().to_path_buf();
        let target = path
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("source")
            .join("config.toml");

        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, "hud = false\n").unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();

        let state = ConfigWatchState::discover(&path);
        let target_root = target.parent().unwrap().to_path_buf();

        assert_eq!(state.configured_path, path);
        assert_eq!(state.resolved_path, Some(target.clone()));
        assert!(state.roots.contains(&configured_root));
        assert!(state.roots.contains(&target_root));
        assert_eq!(state.roots.len(), 2);

        let _ = std::fs::remove_dir_all(state.configured_path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn config_watch_state_matches_resolved_target_events() {
        let path = temp_config_path("match-target");
        let target = path
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .join("source")
            .join("config.toml");

        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(&target, "hud = false\n").unwrap();
        std::os::unix::fs::symlink(&target, &path).unwrap();

        let state = ConfigWatchState::discover(&path);
        let event = Event {
            kind: EventKind::Modify(ModifyKind::Any),
            paths: vec![target.clone()],
            attrs: Default::default(),
        };

        assert!(state.matches_event(&event));

        let _ = std::fs::remove_dir_all(state.configured_path.parent().unwrap().parent().unwrap());
    }
}
