use std::path::PathBuf;
use wayvoice::audio_guard::{analyze_audio, reject_before_transcribe, reject_transcript};
use wayvoice::config::{Config, Provider};
use wayvoice::transcription::transcribe_audio;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn default_test_config() -> Config {
    Config {
        provider: Provider::Groq,
        ..Config::default()
    }
}

// ── Offline tests (no API calls) ──────────────────────────────────

#[test]
fn silence_rejected_by_audio_guard() {
    let audio = std::fs::read(fixture("silence.wav")).unwrap();
    let metrics = analyze_audio(&audio);

    assert!(
        metrics.likely_silent,
        "silence.wav should be detected as silent: mean_abs={:.1}, max_abs={}",
        metrics.mean_abs, metrics.max_abs
    );

    let result = reject_before_transcribe(Provider::Groq, metrics);
    assert_eq!(result, Some("No microphone input detected"));
}

// ── Integration tests (require GROQ_API_KEY) ──────────────────────

fn has_groq_key() -> bool {
    std::env::var("GROQ_API_KEY").is_ok()
}

#[tokio::test]
async fn silence_e2e_rejected() {
    if !has_groq_key() {
        eprintln!("skipping: GROQ_API_KEY not set");
        return;
    }

    let audio = std::fs::read(fixture("silence.wav")).unwrap();
    let config = default_test_config();
    let metrics = analyze_audio(&audio);

    // The audio guard should catch this before we even call the API
    let guard = reject_before_transcribe(config.provider, metrics);
    assert_eq!(guard, Some("No microphone input detected"));

    // But also verify: if we bypass the guard and send to Groq,
    // the transcript rejection catches the hallucination
    let transcript = transcribe_audio(audio, &config).await.unwrap();
    let post_guard = reject_transcript(config.provider, &transcript, metrics);
    assert!(
        post_guard.is_some(),
        "Groq hallucinated on silence and we didn't catch it: {transcript:?}"
    );
}
