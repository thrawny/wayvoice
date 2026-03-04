use crate::debug_recordings::save_recording_for_debug;
use log::debug;
use std::process::Stdio;
use tokio::process::Command;
use wayvoice::audio_guard::{analyze_audio, reject_before_transcribe, reject_transcript};
use wayvoice::config::Config;
use wayvoice::text::apply_replacements;
use wayvoice::transcription::transcribe_audio;

pub async fn run_once(config: Config) {
    let audio_file = std::env::temp_dir().join("voice-recording.wav");
    let _ = tokio::fs::remove_file(&audio_file).await;

    let mut child = match Command::new("pw-record")
        .args([
            "--format",
            "s16",
            "--rate",
            "16000",
            "--channels",
            "1",
            audio_file.to_str().unwrap(),
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => {
            eprintln!("Failed to start pw-record: {e}");
            std::process::exit(1);
        }
    };

    eprintln!("Recording... (press Enter to stop)");

    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);

    let _ = child.kill().await;
    let _ = child.wait().await;
    save_recording_for_debug(&audio_file, &config).await;

    let audio_data = match tokio::fs::metadata(&audio_file).await {
        Ok(meta) if meta.len() < 1000 => {
            eprintln!("No audio recorded (file too small)");
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("No audio file created: {e}");
            std::process::exit(1);
        }
        Ok(meta) => {
            debug!("audio bytes: {}", meta.len());
            match tokio::fs::read(&audio_file).await {
                Ok(data) => data,
                Err(e) => {
                    eprintln!("Failed to read audio file: {e}");
                    std::process::exit(1);
                }
            }
        }
    };

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

    if let Some(reason) = reject_before_transcribe(config.provider, metrics) {
        eprintln!("{reason}");
        std::process::exit(1);
    }

    eprintln!("Transcribing...");

    let text = match transcribe_audio(audio_data, &config).await {
        Ok(text) => text,
        Err(e) => {
            eprintln!("Transcription failed: {e}");
            std::process::exit(1);
        }
    };

    debug!("raw: {text}");
    if let Some(reason) = reject_transcript(&config, &text, metrics) {
        eprintln!("{reason}");
        std::process::exit(1);
    }

    let text = apply_replacements(&text, &config.replacements);
    debug!("replaced: {text}");
    println!("{text}");
}
