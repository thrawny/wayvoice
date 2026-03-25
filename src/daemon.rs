use crate::debug_recordings::save_wav_data_for_debug;
use crate::inject::{inject_text, notify};
use log::{debug, warn};
use std::io::Write;
use std::process::Stdio;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;
use wayvoice::audio_guard::{
    AudioMetrics, analyze_audio, reject_before_transcribe, reject_transcript, wrap_pcm_as_wav,
};
use wayvoice::audio_level::display_level;
use wayvoice::config::Config;
use wayvoice::post_process::run_post_command;
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

    pub fn reload_config(&mut self, config: Config) -> bool {
        if self.config == config {
            return false;
        }

        let should_spawn_hud = !self.config.hud && config.hud;
        self.config = config;
        debug!("runtime config reloaded");
        should_spawn_hud
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

                if let Some(reason) = reject_transcript(&job.config, &text, job.metrics) {
                    warn!("Discarded suspicious transcript: {text:?} ({reason})");
                    notify(reason, &job.config).await;
                } else {
                    let replace_start = std::time::Instant::now();
                    let replaced = apply_replacements(&text, &job.config.replacements);
                    let replace_ms = replace_start.elapsed().as_secs_f64() * 1000.0;
                    debug!("replaced: {replaced}");

                    let post_start = std::time::Instant::now();
                    let final_text = match run_post_command(&replaced, &job.config).await {
                        Ok(t) => t,
                        Err(e) => {
                            warn!("post_command failed: {e}");
                            notify(&format!("Post-command error: {e}"), &job.config).await;
                            replaced.clone()
                        }
                    };
                    let post_ms = post_start.elapsed().as_secs_f64() * 1000.0;

                    let transcribe_ms =
                        job.total_start.elapsed().as_secs_f64() * 1000.0 - post_ms - replace_ms;
                    log_transcription_stages(
                        &text,
                        &replaced,
                        &final_text,
                        &job.config,
                        transcribe_ms,
                        replace_ms,
                        post_ms,
                    );
                    let text = final_text;
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
            "audio signal: payload_bytes={} samples={} mean_abs={:.2} dc_offset={:.2} max_abs={} centered_mean_abs={:.2} centered_rms={:.2} centered_p90_abs={:.2} centered_zc_per_sec={:.2} too_short={} likely_silent={}",
            metrics.payload_bytes,
            metrics.sample_count,
            metrics.mean_abs,
            metrics.dc_offset,
            metrics.max_abs,
            metrics.centered_mean_abs,
            metrics.centered_rms,
            metrics.centered_p90_abs,
            metrics.centered_zero_crossings_per_sec,
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
                let current_level = display_level(&window);
                level.store(current_level.to_bits(), Ordering::Relaxed);
                window_pos = 0;
            }
        }
    }

    level.store(0.0f32.to_bits(), Ordering::Relaxed);
}

fn log_transcription_stages(
    raw: &str,
    replaced: &str,
    post_processed: &str,
    config: &Config,
    transcribe_ms: f64,
    replace_ms: f64,
    post_process_ms: f64,
) {
    let log_path = wayvoice::config::config_path()
        .parent()
        .unwrap()
        .join("transcription_log.jsonl");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path);
    match file {
        Ok(mut f) => {
            let ts = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string();
            let model = if config.model.is_empty() {
                match config.provider {
                    wayvoice::config::Provider::Openai => "whisper-1",
                    wayvoice::config::Provider::Groq => "whisper-large-v3-turbo",
                }
            } else {
                &config.model
            };
            let entry = serde_json::json!({
                "ts": ts,
                "raw": raw,
                "replaced": replaced,
                "post_processed": post_processed,
                "provider": format!("{:?}", config.provider),
                "transcription_model": model,
                "post_process": config.post_process,
                "post_process_model": config.post_process_model,
                "transcribe_ms": (transcribe_ms * 10.0).round() / 10.0,
                "replace_ms": (replace_ms * 10.0).round() / 10.0,
                "post_process_ms": (post_process_ms * 10.0).round() / 10.0,
            });
            let _ = writeln!(f, "{entry}");
        }
        Err(e) => warn!("failed to write transcription log: {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::Daemon;
    use wayvoice::config::Config;

    #[test]
    fn reload_config_updates_runtime_settings() {
        let mut initial = Config::default();
        initial.hud = false;

        let mut daemon = Daemon::new(initial.clone());
        let mut updated = initial.clone();
        updated.notify_send = true;
        updated
            .replacements
            .insert("hyperland".to_string(), "Hyprland".to_string());

        assert!(!daemon.reload_config(updated.clone()));
        assert_eq!(daemon.config, updated);
    }

    #[test]
    fn reload_config_requests_hud_spawn_when_hud_becomes_enabled() {
        let mut initial = Config::default();
        initial.hud = false;

        let mut daemon = Daemon::new(initial.clone());
        let mut updated = initial;
        updated.hud = true;

        assert!(daemon.reload_config(updated));
    }
}
