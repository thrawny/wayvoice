use crate::config::{Config, load_config};
use crate::inject::{inject_text, notify};
use crate::text::apply_replacements;
use crate::transcription::transcribe_audio;
use log::debug;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::{Child, Command};

#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Idle,
    Recording,
    Transcribing,
}

impl State {
    fn as_str(&self) -> &'static str {
        match self {
            State::Idle => "idle",
            State::Recording => "recording",
            State::Transcribing => "transcribing",
        }
    }
}

pub struct Daemon {
    state: State,
    config: Config,
    recorder: Option<Child>,
    audio_file: PathBuf,
}

impl Daemon {
    pub fn new() -> Self {
        let audio_file = std::env::temp_dir().join("voice-recording.wav");
        Self {
            state: State::Idle,
            config: load_config(),
            recorder: None,
            audio_file,
        }
    }

    pub fn status(&self) -> &'static str {
        self.state.as_str()
    }

    pub async fn toggle(&mut self) -> &'static str {
        match self.state {
            State::Idle => {
                self.start_recording().await;
                "recording"
            }
            State::Recording => {
                self.stop_and_transcribe().await;
                "transcribing"
            }
            State::Transcribing => "busy",
        }
    }

    pub async fn cancel(&mut self) -> &'static str {
        if let Some(mut child) = self.recorder.take() {
            let _ = child.kill().await;
        }
        self.state = State::Idle;
        notify("Cancelled").await;
        "cancelled"
    }

    async fn start_recording(&mut self) {
        let _ = tokio::fs::remove_file(&self.audio_file).await;

        let child = Command::new("pw-record")
            .args([
                "--format",
                "s16",
                "--rate",
                "16000",
                "--channels",
                "1",
                self.audio_file.to_str().unwrap(),
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();

        match child {
            Ok(child) => {
                self.recorder = Some(child);
                self.state = State::Recording;
                notify("Recording...").await;
            }
            Err(e) => {
                eprintln!("Failed to start pw-record: {e}");
                notify("Failed to start recording").await;
            }
        }
    }

    async fn stop_and_transcribe(&mut self) {
        let total_start = std::time::Instant::now();

        let stop_start = std::time::Instant::now();
        if let Some(mut child) = self.recorder.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        debug!("stop_recording: {:?}", stop_start.elapsed());

        // Check if we got any audio
        match tokio::fs::metadata(&self.audio_file).await {
            Ok(meta) if meta.len() < 1000 => {
                eprintln!("No audio recorded");
                notify("No audio recorded").await;
                self.state = State::Idle;
                return;
            }
            Err(_) => {
                eprintln!("No audio file");
                notify("Recording failed").await;
                self.state = State::Idle;
                return;
            }
            Ok(meta) => {
                debug!("audio bytes: {}", meta.len());
            }
        }

        self.state = State::Transcribing;
        notify("Transcribing...").await;

        let read_start = std::time::Instant::now();
        let audio_data = match tokio::fs::read(&self.audio_file).await {
            Ok(data) => data,
            Err(e) => {
                eprintln!("Failed to read audio file: {e}");
                notify(&format!("Error: {e}")).await;
                self.state = State::Idle;
                return;
            }
        };
        debug!("file_read: {:?}", read_start.elapsed());

        match transcribe_audio(audio_data, &self.config).await {
            Ok(text) => {
                debug!("raw: {text}");
                let text = apply_replacements(&text, &self.config.replacements);
                debug!("replaced: {text}");
                if !text.is_empty() {
                    let inject_start = std::time::Instant::now();
                    inject_text(&text).await;
                    debug!("inject: {:?}", inject_start.elapsed());
                }
            }
            Err(e) => {
                eprintln!("Transcription failed: {e}");
                notify(&format!("Error: {e}")).await;
            }
        }

        debug!("total: {:?}", total_start.elapsed());
        self.state = State::Idle;
    }
}
