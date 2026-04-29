use crate::daemon::{Daemon, ToggleResult, TranscriptionJob};
use log::debug;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex;
use wayvoice::config::Config;
use wayvoice::transcription::transcribe_audio;

pub fn socket_path() -> PathBuf {
    std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
        .join("wayvoice.sock")
}

#[derive(Debug, Deserialize, Default)]
struct IpcCommand {
    #[serde(default)]
    cmd: String,
    #[serde(default)]
    overrides: RequestOverrides,
}

#[derive(Debug, Deserialize, Default)]
struct RequestOverrides {
    prompt: Option<String>,
    extra_keywords: Option<Vec<String>>,
    replacements: Option<HashMap<String, String>>,
    inject_mode: Option<String>,
    use_default_keywords: Option<bool>,
}

impl RequestOverrides {
    fn summary(&self) -> String {
        format!(
            "inject_mode={:?} prompt_chars={} extra_keywords={} keywords_sample={:?} replacements={} use_default_keywords={:?}",
            self.inject_mode,
            self.prompt.as_ref().map_or(0, String::len),
            self.extra_keywords.as_ref().map_or(0, Vec::len),
            self.extra_keywords
                .as_ref()
                .map(|keywords| keywords.iter().take(8).cloned().collect::<Vec<_>>())
                .unwrap_or_default(),
            self.replacements.as_ref().map_or(0, HashMap::len),
            self.use_default_keywords,
        )
    }

    fn apply(self, config: &mut Config) {
        if let Some(prompt) = self.prompt {
            config.prompt = prompt;
        }
        if let Some(extra_keywords) = self.extra_keywords {
            config.extra_keywords.extend(extra_keywords);
        }
        if let Some(replacements) = self.replacements {
            config.replacements.extend(replacements);
        }
        if let Some(inject_mode) = self.inject_mode {
            config.inject_mode = inject_mode;
        }
        if let Some(use_default_keywords) = self.use_default_keywords {
            config.use_default_keywords = use_default_keywords;
        }
    }
}

impl IpcCommand {
    fn summary(&self) -> String {
        format!(
            "cmd={:?} overrides={{ {} }}",
            self.cmd,
            self.overrides.summary()
        )
    }
}

fn summarize_incoming_line(line: &str) -> String {
    if let Some(json) = line.strip_prefix("start-json ") {
        return format!("start-json {}", summarize_json_payload(json));
    }
    if line.starts_with('{') {
        return summarize_json_payload(line);
    }
    line.to_string()
}

fn summarize_json_payload(json: &str) -> String {
    match serde_json::from_str::<IpcCommand>(json) {
        Ok(command) => command.summary(),
        Err(_) => format!("invalid-json chars={}", json.len()),
    }
}

pub async fn run_server(daemon: Arc<Mutex<Daemon>>) -> Result<(), Box<dyn std::error::Error>> {
    let path = socket_path();
    let _ = tokio::fs::remove_file(&path).await;

    let listener = UnixListener::bind(&path)?;
    println!("Listening on {path:?}");

    loop {
        let (stream, _) = listener.accept().await?;
        let daemon = daemon.clone();
        tokio::spawn(handle_client(stream, daemon));
    }
}

async fn handle_client(stream: UnixStream, daemon: Arc<Mutex<Daemon>>) {
    let (reader, mut writer) = stream.into_split();
    let mut reader = BufReader::new(reader);
    let mut line = String::new();

    if reader.read_line(&mut line).await.is_ok() {
        let line = line.trim();
        if line != "status" {
            debug!("ipc incoming raw: {line}");
            debug!("ipc incoming: {}", summarize_incoming_line(line));
        }
        let response = handle_command(line, daemon).await;
        let _ = writer.write_all(response.as_bytes()).await;
        let _ = writer.write_all(b"\n").await;
    }
}

async fn handle_command(line: &str, daemon: Arc<Mutex<Daemon>>) -> String {
    if let Some(json) = line.strip_prefix("start-json ") {
        return handle_json_command(json, "toggle", daemon).await;
    }
    if line == "stop-json" {
        return handle_toggle(daemon, None).await;
    }
    if line.starts_with('{') {
        return handle_json_command(line, "", daemon).await;
    }

    match line {
        "toggle" => handle_toggle(daemon, None).await,
        "cancel" => {
            let mut d = daemon.lock().await;
            d.cancel().await.to_string()
        }
        "status" => {
            let d = daemon.lock().await;
            let status = d.status();
            if status == "recording" {
                format!("{status} {:.3}", d.audio_level())
            } else {
                status.to_string()
            }
        }
        _ => "unknown".to_string(),
    }
}

async fn handle_json_command(line: &str, default_cmd: &str, daemon: Arc<Mutex<Daemon>>) -> String {
    let mut command = match serde_json::from_str::<IpcCommand>(line) {
        Ok(command) => command,
        Err(e) => return format!("error invalid-json: {e}"),
    };
    if command.cmd.is_empty() {
        command.cmd = default_cmd.to_string();
    }

    debug!("ipc parsed: {}", command.summary());

    match command.cmd.as_str() {
        "toggle" | "start" | "stop" => handle_toggle(daemon, Some(command.overrides)).await,
        "cancel" => {
            let mut d = daemon.lock().await;
            d.cancel().await.to_string()
        }
        "status" => {
            let d = daemon.lock().await;
            let status = d.status();
            if status == "recording" {
                format!("{status} {:.3}", d.audio_level())
            } else {
                status.to_string()
            }
        }
        _ => "unknown".to_string(),
    }
}

async fn handle_toggle(daemon: Arc<Mutex<Daemon>>, overrides: Option<RequestOverrides>) -> String {
    let mut d = daemon.lock().await;
    let result = match overrides {
        Some(overrides) => {
            d.toggle_with_overrides(|config| overrides.apply(config))
                .await
        }
        None => d.toggle().await,
    };
    match result {
        ToggleResult::Started => "recording".to_string(),
        ToggleResult::Stopped => "idle".to_string(),
        ToggleResult::Busy => "busy".to_string(),
        ToggleResult::Transcribing(job) => {
            drop(d);
            transcribe_for_ipc(daemon.clone(), job).await
        }
    }
}

async fn transcribe_for_ipc(daemon: Arc<Mutex<Daemon>>, job: Box<TranscriptionJob>) -> String {
    if job.config.inject_mode != "stdout" {
        tokio::spawn(async move {
            let result = transcribe_audio(job.audio_data.clone(), &job.config).await;
            let mut d = daemon.lock().await;
            let _ = d.finish_transcription(result, &job).await;
        });
        return "transcribing".to_string();
    }

    let result = transcribe_audio(job.audio_data.clone(), &job.config).await;
    let mut d = daemon.lock().await;
    d.finish_transcription(result, &job)
        .await
        .unwrap_or_default()
}

pub async fn send_command(cmd: &str) -> Result<String, Box<dyn std::error::Error>> {
    let path = socket_path();
    let mut stream = UnixStream::connect(&path).await?;

    stream.write_all(cmd.as_bytes()).await?;
    stream.write_all(b"\n").await?;

    let mut reader = BufReader::new(stream);
    let mut response = String::new();
    reader.read_line(&mut response).await?;

    Ok(response.trim().to_string())
}
