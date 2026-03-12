use std::path::PathBuf;
use wayvoice::audio_guard::{
    analyze_audio, reject_before_transcribe, reject_transcript, wrap_pcm_as_wav,
};
use wayvoice::audio_level::{display_level, rms_level};
use wayvoice::audio_signal::pcm_payload;
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
        language: "en".to_string(),
        ..Config::default()
    }
}

macro_rules! require_groq {
    () => {
        if std::env::var("GROQ_API_KEY").is_err() {
            eprintln!("skipping: GROQ_API_KEY not set");
            return;
        }
    };
}

fn assert_ryzen_silence_rejected(name: &str) {
    let audio = std::fs::read(fixture(name)).unwrap();
    let metrics = analyze_audio(&audio);

    assert!(
        metrics.likely_silent,
        "{name} should be detected as silent: dc_offset={:.1} centered_p90_abs={:.1} zc/s={:.1}",
        metrics.dc_offset, metrics.centered_p90_abs, metrics.centered_zero_crossings_per_sec
    );
    assert_eq!(
        reject_before_transcribe(Provider::Groq, metrics),
        Some("No microphone input detected")
    );
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

#[test]
fn rms_level_on_silence_fixture() {
    let wav = std::fs::read(fixture("silence.wav")).unwrap();
    let pcm = &wav[44..];
    let level = rms_level(pcm);
    assert!(
        level < 0.01,
        "silence fixture should have near-zero RMS, got {level}"
    );
}

#[test]
fn wrap_pcm_as_wav_roundtrips_through_analyze() {
    let samples: Vec<i16> = (0..16000)
        .map(|i| if i % 20 < 10 { 1500 } else { -1500 })
        .collect();
    let pcm: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    let wav = wrap_pcm_as_wav(&pcm);

    let metrics = analyze_audio(&wav);
    assert_eq!(metrics.sample_count, 16000);
    assert!(!metrics.too_short);
    assert!(!metrics.likely_silent);
    assert_eq!(reject_before_transcribe(Provider::Groq, metrics), None);
}

#[test]
fn ryzen_silence_rejected_by_audio_guard() {
    assert_ryzen_silence_rejected("ryzen_silence_gibberish.wav");
}

#[test]
fn ryzen_silence_edge_rejected_by_audio_guard() {
    assert_ryzen_silence_rejected("ryzen_silence_edge.wav");
}

#[test]
fn ryzen_silence_latest_rejected_by_audio_guard() {
    assert_ryzen_silence_rejected("ryzen_silence_rejected.wav");
}

#[test]
fn ryzen_speech_passes_audio_guard() {
    let audio = std::fs::read(fixture("ryzen_speech.wav")).unwrap();
    let metrics = analyze_audio(&audio);

    assert!(!metrics.too_short, "ryzen speech flagged as too short");
    assert!(
        !metrics.likely_silent,
        "ryzen speech flagged as silent: dc_offset={:.1} centered_p90_abs={:.1} zc/s={:.1}",
        metrics.dc_offset, metrics.centered_p90_abs, metrics.centered_zero_crossings_per_sec
    );
    assert_eq!(reject_before_transcribe(Provider::Groq, metrics), None);
}

#[test]
fn display_level_handles_ryzen_offset_better_than_raw_rms() {
    let ryzen_silence = std::fs::read(fixture("ryzen_silence_gibberish.wav")).unwrap();
    let ryzen_silence_edge = std::fs::read(fixture("ryzen_silence_edge.wav")).unwrap();
    let ryzen_silence_latest = std::fs::read(fixture("ryzen_silence_rejected.wav")).unwrap();
    let ryzen_speech = std::fs::read(fixture("ryzen_speech.wav")).unwrap();

    let silence_pcm = pcm_payload(&ryzen_silence);
    let silence_edge_pcm = pcm_payload(&ryzen_silence_edge);
    let silence_latest_pcm = pcm_payload(&ryzen_silence_latest);
    let speech_pcm = pcm_payload(&ryzen_speech);

    let silence_display = display_level(silence_pcm);
    let silence_edge_display = display_level(silence_edge_pcm);
    let silence_latest_display = display_level(silence_latest_pcm);
    let speech_display = display_level(speech_pcm);
    let silence_rms = rms_level(silence_pcm);

    assert!(
        silence_display < 0.02,
        "ryzen silence display level should stay low, got {silence_display}"
    );
    assert!(
        silence_edge_display < 0.02,
        "ryzen silence edge display level should stay low, got {silence_edge_display}"
    );
    assert!(
        silence_latest_display < 0.02,
        "ryzen latest silence display level should stay low, got {silence_latest_display}"
    );
    assert!(
        speech_display > silence_display * 2.0,
        "speech display level should exceed silence: speech={speech_display} silence={silence_display}"
    );
    assert!(
        speech_display > silence_edge_display * 2.0,
        "speech display level should exceed edge silence: speech={speech_display} silence={silence_edge_display}"
    );
    assert!(
        speech_display > silence_latest_display * 2.0,
        "speech display level should exceed latest silence: speech={speech_display} silence={silence_latest_display}"
    );
    assert!(
        silence_rms > silence_display * 10.0,
        "raw RMS should remain much higher than the display level on biased silence: rms={silence_rms} display={silence_display}"
    );
}

// ── Happy path: real speech must not be rejected ─────────────────

#[test]
fn real_speech_passes_audio_guard() {
    let audio = std::fs::read(fixture("keywords_test.wav")).unwrap();
    let metrics = analyze_audio(&audio);

    assert!(!metrics.too_short, "real speech flagged as too short");
    assert!(
        !metrics.likely_silent,
        "real speech flagged as silent: mean_abs={:.1} max_abs={}",
        metrics.mean_abs, metrics.max_abs
    );
    assert_eq!(reject_before_transcribe(Provider::Groq, metrics), None);
}

#[tokio::test]
async fn real_speech_transcribes_successfully() {
    require_groq!();

    let audio = std::fs::read(fixture("keywords_test.wav")).unwrap();
    let config = default_test_config();
    let metrics = analyze_audio(&audio);

    assert_eq!(reject_before_transcribe(config.provider, metrics), None);

    let transcript = transcribe_audio(audio, &config).await.unwrap();
    assert!(!transcript.is_empty(), "transcript should not be empty");

    let rejection = reject_transcript(&config, &transcript, metrics);
    assert_eq!(
        rejection, None,
        "real speech was rejected: {transcript:?} reason={rejection:?}"
    );

    let lower = transcript.to_lowercase();
    assert!(
        lower.contains("update") || lower.contains("config") || lower.contains("flake"),
        "transcript doesn't contain expected words: {transcript:?}"
    );
}

// ── Rejection tests (require GROQ_API_KEY) ───────────────────────

#[tokio::test]
async fn silence_e2e_rejected() {
    require_groq!();

    let audio = std::fs::read(fixture("silence.wav")).unwrap();
    let config = default_test_config();
    let metrics = analyze_audio(&audio);

    let guard = reject_before_transcribe(config.provider, metrics);
    assert_eq!(guard, Some("No microphone input detected"));

    let transcript = transcribe_audio(audio, &config).await.unwrap();
    let post_guard = reject_transcript(&config, &transcript, metrics);
    assert!(
        post_guard.is_some(),
        "Groq hallucinated on silence and we didn't catch it: {transcript:?}"
    );
}

// ── Eval tests (require GROQ_API_KEY, #[ignore]) ────────────────

#[tokio::test]
#[ignore]
async fn silence_gibberish_transcription() {
    require_groq!();

    let audio = std::fs::read(fixture("silence_gibberish.wav")).unwrap();
    let metrics = analyze_audio(&audio);
    eprintln!(
        "metrics: mean_abs={:.1} max_abs={} samples={} too_short={} likely_silent={}",
        metrics.mean_abs,
        metrics.max_abs,
        metrics.sample_count,
        metrics.too_short,
        metrics.likely_silent
    );

    let config = default_test_config();
    let transcript = transcribe_audio(audio, &config).await.unwrap();
    eprintln!("transcript: {transcript}");

    let ascii_count = transcript.chars().filter(|c| c.is_ascii()).count();
    let total = transcript.len().max(1);
    eprintln!(
        "ascii ratio: {ascii_count}/{total} = {:.2}",
        ascii_count as f64 / total as f64
    );

    let rejection = reject_transcript(&config, &transcript, metrics);
    eprintln!("rejection: {rejection:?}");
}

const EXPECTED_TERMS: &[&str] = &[
    "Hyprland",
    "Wisprflow",
    "NixOS",
    "LazyVim",
    "Claude Code",
    "wayvoice",
    "TOML",
    "Groq",
];

fn count_matched_terms(transcript: &str) -> Vec<&'static str> {
    EXPECTED_TERMS
        .iter()
        .filter(|term| transcript.contains(**term))
        .copied()
        .collect()
}

fn print_eval(label: &str, transcript: &str, matched: &[&str]) {
    eprintln!("── {label} ──");
    eprintln!("  transcript: {transcript}");
    eprintln!(
        "  matched:    {}/{} {:?}",
        matched.len(),
        EXPECTED_TERMS.len(),
        matched
    );
    eprintln!();
}

#[tokio::test]
#[ignore]
async fn keywords_eval() {
    require_groq!();

    let audio = std::fs::read(fixture("keywords_test.wav")).unwrap();

    let config_bare = Config {
        provider: Provider::Groq,
        prompt: String::new(),
        keywords: vec![],
        ..Config::default()
    };
    let without = transcribe_audio(audio.clone(), &config_bare).await.unwrap();
    let matched_without = count_matched_terms(&without);
    print_eval("No keywords", &without, &matched_without);

    let config_kw = {
        let mut c = default_test_config();
        c.prompt = c.keywords.join(", ");
        c
    };
    let with = transcribe_audio(audio, &config_kw).await.unwrap();
    let matched_with = count_matched_terms(&with);
    print_eval("With keywords", &with, &matched_with);

    eprintln!("── Summary ──");
    eprintln!(
        "  {}/{} No keywords",
        matched_without.len(),
        EXPECTED_TERMS.len()
    );
    eprintln!(
        "  {}/{} With keywords",
        matched_with.len(),
        EXPECTED_TERMS.len()
    );

    assert!(
        matched_with.len() >= matched_without.len(),
        "Keywords should not make things worse: with={} without={}",
        matched_with.len(),
        matched_without.len(),
    );
}
