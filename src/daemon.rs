use crate::audio_guard::{
    AudioMetrics, analyze_audio, reject_before_transcribe, reject_transcript,
};
use crate::config::{Config, load_config};
use crate::debug_recordings::save_recording_for_debug;
use crate::inject::{inject_text, notify};
use crate::text::apply_replacements;
use log::{debug, warn};
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

pub struct TranscriptionJob {
    pub audio_data: Vec<u8>,
    pub config: Config,
    pub metrics: AudioMetrics,
    pub total_start: std::time::Instant,
}

pub enum ToggleResult {
    Started,
    Transcribing(TranscriptionJob),
    Busy,
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

    pub async fn toggle(&mut self) -> ToggleResult {
        match self.state {
            State::Idle => {
                self.start_recording().await;
                ToggleResult::Started
            }
            State::Recording => match self.prepare_transcription().await {
                Some(job) => ToggleResult::Transcribing(job),
                None => ToggleResult::Started, // fell back to idle
            },
            State::Transcribing => ToggleResult::Busy,
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

    pub async fn finish_transcription(
        &mut self,
        result: Result<String, Box<dyn std::error::Error + Send + Sync>>,
        job: &TranscriptionJob,
    ) {
        match result {
            Ok(text) => {
                debug!("raw: {text}");

                if let Some(reason) = reject_transcript(job.config.provider, &text, job.metrics) {
                    warn!("Discarded suspicious transcript: {text:?} ({reason})");
                    notify(reason).await;
                } else {
                    let text = apply_replacements(&text, &job.config.replacements);
                    debug!("replaced: {text}");
                    if !text.is_empty() {
                        let inject_start = std::time::Instant::now();
                        inject_text(&text).await;
                        debug!("inject: {:?}", inject_start.elapsed());
                    }
                }
            }
            Err(e) => {
                eprintln!("Transcription failed: {e}");
                notify(&format!("Error: {e}")).await;
            }
        }

        debug!("total: {:?}", job.total_start.elapsed());
        self.state = State::Idle;
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

    /// Stop recording, read & validate audio, set state to Transcribing.
    /// Returns a job for the caller to run outside the lock, or None if
    /// the audio was rejected early.
    async fn prepare_transcription(&mut self) -> Option<TranscriptionJob> {
        let total_start = std::time::Instant::now();

        let stop_start = std::time::Instant::now();
        if let Some(mut child) = self.recorder.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        save_recording_for_debug(&self.audio_file).await;
        debug!("stop_recording: {:?}", stop_start.elapsed());

        match tokio::fs::metadata(&self.audio_file).await {
            Ok(meta) if meta.len() < 1000 => {
                eprintln!("No audio recorded");
                notify("No audio recorded").await;
                self.state = State::Idle;
                return None;
            }
            Err(_) => {
                eprintln!("No audio file");
                notify("Recording failed").await;
                self.state = State::Idle;
                return None;
            }
            Ok(meta) => {
                debug!("audio bytes: {}", meta.len());
            }
        }

        let read_start = std::time::Instant::now();
        let audio_data = match tokio::fs::read(&self.audio_file).await {
            Ok(data) => data,
            Err(e) => {
                eprintln!("Failed to read audio file: {e}");
                notify(&format!("Error: {e}")).await;
                self.state = State::Idle;
                return None;
            }
        };
        debug!("file_read: {:?}", read_start.elapsed());

        let metrics = analyze_audio(&audio_data);
        debug!(
            "audio signal: payload_bytes={} samples={} mean_abs={:.2} max_abs={} too_short={} likely_silent={}",
            metrics.payload_bytes,
            metrics.sample_count,
            metrics.mean_abs,
            metrics.max_abs,
            metrics.too_short,
            metrics.likely_silent
        );

        if let Some(reason) = reject_before_transcribe(self.config.provider, metrics) {
            warn!("{reason}");
            notify(reason).await;
            self.state = State::Idle;
            return None;
        }

        self.state = State::Transcribing;
        notify("Transcribing...").await;

        Some(TranscriptionJob {
            audio_data,
            config: self.config.clone(),
            metrics,
            total_start,
        })
    }
}
