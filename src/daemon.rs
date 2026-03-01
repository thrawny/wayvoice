use crate::debug_recordings::save_wav_data_for_debug;
use crate::inject::{inject_text, notify};
use log::{debug, warn};
use std::process::Stdio;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;
use wayvoice::audio_guard::{
    AudioMetrics, analyze_audio, reject_before_transcribe, reject_transcript, wrap_pcm_as_wav,
};
use wayvoice::audio_level::rms_level;
use wayvoice::config::Config;
use wayvoice::text::apply_replacements;

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
    Transcribing(Box<TranscriptionJob>),
    Busy,
}

pub struct Daemon {
    state: State,
    config: Config,
    recorder: Option<Child>,
    pcm_reader: Option<JoinHandle<()>>,
    pcm_buffer: Arc<Mutex<Vec<u8>>>,
    audio_level: Arc<AtomicU32>,
}

impl Daemon {
    pub fn new(config: Config) -> Self {
        Self {
            state: State::Idle,
            config,
            recorder: None,
            pcm_reader: None,
            pcm_buffer: Arc::new(Mutex::new(Vec::new())),
            audio_level: Arc::new(AtomicU32::new(0.0f32.to_bits())),
        }
    }

    pub fn status(&self) -> &'static str {
        self.state.as_str()
    }

    pub fn audio_level(&self) -> f32 {
        f32::from_bits(self.audio_level.load(Ordering::Relaxed))
    }

    pub async fn toggle(&mut self) -> ToggleResult {
        match self.state {
            State::Idle => {
                self.start_recording().await;
                ToggleResult::Started
            }
            State::Recording => match self.prepare_transcription().await {
                Some(job) => ToggleResult::Transcribing(Box::new(job)),
                None => ToggleResult::Started,
            },
            State::Transcribing => ToggleResult::Busy,
        }
    }

    pub async fn cancel(&mut self) -> &'static str {
        if let Some(mut child) = self.recorder.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        if let Some(reader) = self.pcm_reader.take()
            && let Err(e) = reader.await
        {
            warn!("PCM reader task failed during cancel: {e}");
        }
        self.pcm_buffer.lock().unwrap().clear();
        self.audio_level.store(0.0f32.to_bits(), Ordering::Relaxed);
        self.state = State::Idle;
        notify("Cancelled", &self.config).await;
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
                    notify(reason, &job.config).await;
                } else {
                    let text = apply_replacements(&text, &job.config.replacements);
                    debug!("replaced: {text}");
                    if !text.is_empty() {
                        let inject_start = std::time::Instant::now();
                        inject_text(&text, &job.config).await;
                        debug!("inject: {:?}", inject_start.elapsed());
                    }
                }
            }
            Err(e) => {
                eprintln!("Transcription failed: {e}");
                notify(&format!("Error: {e}"), &job.config).await;
            }
        }

        debug!("total: {:?}", job.total_start.elapsed());
        self.state = State::Idle;
    }

    async fn start_recording(&mut self) {
        if let Some(reader) = self.pcm_reader.take()
            && let Err(e) = reader.await
        {
            warn!("PCM reader task failed before start: {e}");
        }
        self.pcm_buffer.lock().unwrap().clear();
        self.audio_level.store(0.0f32.to_bits(), Ordering::Relaxed);

        let child = Command::new("pw-record")
            .args([
                "--format",
                "s16",
                "--rate",
                "16000",
                "--channels",
                "1",
                "--raw",
                "-",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();

        match child {
            Ok(mut child) => {
                let stdout = child.stdout.take().expect("stdout piped");
                self.recorder = Some(child);
                self.state = State::Recording;

                let buffer = self.pcm_buffer.clone();
                let level = self.audio_level.clone();
                self.pcm_reader = Some(tokio::spawn(read_pcm_stream(stdout, buffer, level)));

                notify("Recording...", &self.config).await;
            }
            Err(e) => {
                eprintln!("Failed to start pw-record: {e}");
                notify("Failed to start recording", &self.config).await;
            }
        }
    }

    /// Stop recording, validate audio, set state to Transcribing.
    /// Returns a job for the caller to run outside the lock, or None if
    /// the audio was rejected early.
    async fn prepare_transcription(&mut self) -> Option<TranscriptionJob> {
        let total_start = std::time::Instant::now();

        let stop_start = std::time::Instant::now();
        if let Some(mut child) = self.recorder.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        if let Some(reader) = self.pcm_reader.take()
            && let Err(e) = reader.await
        {
            warn!("PCM reader task failed during stop: {e}");
        }
        self.audio_level.store(0.0f32.to_bits(), Ordering::Relaxed);
        debug!("stop_recording: {:?}", stop_start.elapsed());

        let pcm_data = std::mem::take(&mut *self.pcm_buffer.lock().unwrap());

        if pcm_data.is_empty() {
            eprintln!("No audio recorded");
            notify("No audio recorded", &self.config).await;
            self.state = State::Idle;
            return None;
        }

        debug!("pcm bytes: {}", pcm_data.len());

        let audio_data = wrap_pcm_as_wav(&pcm_data);
        save_wav_data_for_debug(&audio_data, &self.config).await;

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
            notify(reason, &self.config).await;
            self.state = State::Idle;
            return None;
        }

        self.state = State::Transcribing;
        notify("Transcribing...", &self.config).await;

        Some(TranscriptionJob {
            audio_data,
            config: self.config.clone(),
            metrics,
            total_start,
        })
    }
}

/// Read raw s16le PCM from pw-record stdout, accumulate in buffer,
/// and update audio level every ~50ms window.
async fn read_pcm_stream(
    mut stdout: tokio::process::ChildStdout,
    buffer: Arc<Mutex<Vec<u8>>>,
    level: Arc<AtomicU32>,
) {
    // ~50ms of audio at 16kHz mono s16 = 1600 samples × 2 bytes
    const WINDOW_BYTES: usize = 3200;
    let mut window = vec![0u8; WINDOW_BYTES];
    let mut window_pos = 0;
    let mut read_buf = [0u8; 4096];

    loop {
        let n = match stdout.read(&mut read_buf).await {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };

        buffer.lock().unwrap().extend_from_slice(&read_buf[..n]);

        // Fill rolling window for RMS computation
        let mut consumed = 0;
        while consumed < n {
            let to_copy = (WINDOW_BYTES - window_pos).min(n - consumed);
            window[window_pos..window_pos + to_copy]
                .copy_from_slice(&read_buf[consumed..consumed + to_copy]);
            window_pos += to_copy;
            consumed += to_copy;

            if window_pos == WINDOW_BYTES {
                let rms = rms_level(&window);
                level.store(rms.to_bits(), Ordering::Relaxed);
                window_pos = 0;
            }
        }
    }

    level.store(0.0f32.to_bits(), Ordering::Relaxed);
}
