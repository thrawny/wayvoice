use crate::config::load_config;
use crate::text::apply_replacements;
use crate::transcription::transcribe_audio;
use log::debug;
use std::process::Stdio;
use tokio::process::Command;

pub async fn run_once() {
    let config = load_config();
    let audio_file = std::env::temp_dir().join("voice-recording.wav");
    let _ = tokio::fs::remove_file(&audio_file).await;

    // Start recording
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

    // Wait for Enter or Ctrl+C
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);

    // Stop recording
    let _ = child.kill().await;
    let _ = child.wait().await;

    // Check if we got any audio
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

    eprintln!("Transcribing...");

    // Transcribe
    let text = match transcribe_audio(audio_data, &config).await {
        Ok(text) => text,
        Err(e) => {
            eprintln!("Transcription failed: {e}");
            std::process::exit(1);
        }
    };

    // Apply replacements and print
    debug!("raw: {text}");
    let text = apply_replacements(&text, &config.replacements);
    debug!("replaced: {text}");
    println!("{text}");
}
