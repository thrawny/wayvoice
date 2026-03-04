use std::path::PathBuf;
use wayvoice::audio_guard::{
    analyze_audio, reject_before_transcribe, reject_transcript, wrap_pcm_as_wav,
};
use wayvoice::audio_level::rms_level;
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
    // Skip WAV header (44 bytes) to get raw PCM
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
    let post_guard = reject_transcript(&config, &transcript, metrics);
    assert!(
        post_guard.is_some(),
        "Groq hallucinated on silence and we didn't catch it: {transcript:?}"
    );
}

#[tokio::test]
#[ignore]
async fn silence_gibberish_transcription() {
    if !has_groq_key() {
        eprintln!("skipping: GROQ_API_KEY not set");
        return;
    }

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

// ── Keyword eval tests (require GROQ_API_KEY, #[ignore]) ────────

/// Expected domain terms in the keywords_test fixture.
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
    if !has_groq_key() {
        eprintln!("skipping: GROQ_API_KEY not set");
        return;
    }

    let audio = std::fs::read(fixture("keywords_test.wav")).unwrap();

    // Without keywords — no prompt at all
    let config_bare = Config {
        provider: Provider::Groq,
        prompt: String::new(),
        keywords: vec![],
        ..Config::default()
    };
    let without = transcribe_audio(audio.clone(), &config_bare).await.unwrap();
    let matched_without = count_matched_terms(&without);
    print_eval("No keywords", &without, &matched_without);

    // With keywords — comma-separated list as prompt
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
