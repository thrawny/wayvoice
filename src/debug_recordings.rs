use log::{debug, warn};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use wayvoice::config::Config;

const KEEP_LAST_RECORDINGS: usize = 10;

pub async fn save_recording_for_debug(audio_file: &Path, config: &Config) {
    if !config.debug_recordings {
        return;
    }

    let Ok(meta) = tokio::fs::metadata(audio_file).await else {
        return;
    };

    if meta.len() == 0 {
        return;
    }

    let dir = recordings_dir(config);
    if let Err(e) = tokio::fs::create_dir_all(&dir).await {
        warn!("Failed to create debug recordings dir {dir:?}: {e}");
        return;
    }

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let dest = dir.join(format!("{timestamp:020}.wav"));

    match tokio::fs::copy(audio_file, &dest).await {
        Ok(_) => {
            debug!("Saved debug recording: {dest:?}");
            prune_old_recordings(&dir).await;
        }
        Err(e) => warn!("Failed to save debug recording {dest:?}: {e}"),
    }
}

fn recordings_dir(config: &Config) -> PathBuf {
    if let Some(dir) = &config.debug_recordings_dir {
        return PathBuf::from(dir);
    }

    dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("wayvoice")
        .join("recordings")
}

async fn prune_old_recordings(dir: &Path) {
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(e) => {
            warn!("Failed to list debug recordings in {dir:?}: {e}");
            return;
        }
    };

    let mut files = Vec::new();
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "wav") {
            files.push(path);
        }
    }

    if files.len() <= KEEP_LAST_RECORDINGS {
        return;
    }

    files.sort();
    let remove_count = files.len() - KEEP_LAST_RECORDINGS;
    for path in files.into_iter().take(remove_count) {
        if let Err(e) = tokio::fs::remove_file(&path).await {
            warn!("Failed to prune debug recording {path:?}: {e}");
        }
    }
}
