use crate::audio_signal::{analyze_pcm_s16le, pcm_payload};
use crate::config::{Config, Provider};

/// ~0.5 sec at 16 kHz mono 16-bit — anything shorter is not intentional speech.
const MIN_PCM_BYTES: usize = 16000;
/// Need at least this many samples before we trust the silence heuristic.
const MIN_SILENCE_SAMPLES: u64 = 4000;
/// Background noise without speech: mean_abs typically 100–1000.
/// Quiet speech starts around mean_abs 1300.
const SILENCE_MEAN_ABS_MAX: f64 = 1000.0;
/// Background noise peaks under ~6000; speech peaks above 10000.
const SILENCE_MAX_ABS_MAX: i32 = 6000;
/// A large DC offset indicates a biased capture path rather than ambient room noise.
const BIASED_CAPTURE_OFFSET_MIN: f64 = 1500.0;
/// Flat offset-heavy junk stays low even at the 90th percentile after centering.
const BIASED_CAPTURE_CENTERED_P90_MAX: f64 = 400.0;
/// Speech crosses the centered zero line often; flat junk barely does.
const BIASED_CAPTURE_CENTERED_ZC_PER_SEC_MAX: f64 = 400.0;
/// Check hallucination patterns for recordings up to ~1.5 sec.
const SHORT_HALLUCINATION_PCM_BYTES: usize = 48000;

#[derive(Debug, Clone, Copy)]
pub struct AudioMetrics {
    pub payload_bytes: usize,
    pub sample_count: u64,
    pub mean_abs: f64,
    pub dc_offset: f64,
    pub max_abs: i32,
    pub centered_mean_abs: f64,
    pub centered_rms: f64,
    pub centered_p90_abs: f64,
    pub centered_zero_crossings_per_sec: f64,
    pub too_short: bool,
    pub likely_silent: bool,
}

pub fn analyze_audio(audio_data: &[u8]) -> AudioMetrics {
    let payload = pcm_payload(audio_data);
    let signal = analyze_pcm_s16le(payload);

    let too_short = payload.len() < MIN_PCM_BYTES;
    let legacy_silence = signal.sample_count >= MIN_SILENCE_SAMPLES
        && signal.mean_abs <= SILENCE_MEAN_ABS_MAX
        && signal.max_abs <= SILENCE_MAX_ABS_MAX;
    let biased_capture_silence = signal.sample_count >= MIN_SILENCE_SAMPLES
        && signal.mean.abs() >= BIASED_CAPTURE_OFFSET_MIN
        && signal.centered_p90_abs <= BIASED_CAPTURE_CENTERED_P90_MAX
        && signal.centered_zero_crossings_per_sec() <= BIASED_CAPTURE_CENTERED_ZC_PER_SEC_MAX;
    let likely_silent = legacy_silence || biased_capture_silence;

    AudioMetrics {
        payload_bytes: payload.len(),
        sample_count: signal.sample_count,
        mean_abs: signal.mean_abs,
        dc_offset: signal.mean,
        max_abs: signal.max_abs,
        centered_mean_abs: signal.centered_mean_abs,
        centered_rms: signal.centered_rms,
        centered_p90_abs: signal.centered_p90_abs,
        centered_zero_crossings_per_sec: signal.centered_zero_crossings_per_sec(),
        too_short,
        likely_silent,
    }
}

/// Wrap raw s16le PCM data in a valid WAV container (16 kHz, mono, 16-bit).
pub fn wrap_pcm_as_wav(pcm: &[u8]) -> Vec<u8> {
    let data_len = pcm.len() as u32;
    let mut wav = Vec::with_capacity(44 + pcm.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_len).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes()); // chunk size
    wav.extend_from_slice(&1u16.to_le_bytes()); // PCM format
    wav.extend_from_slice(&1u16.to_le_bytes()); // mono
    wav.extend_from_slice(&16000u32.to_le_bytes()); // sample rate
    wav.extend_from_slice(&32000u32.to_le_bytes()); // byte rate
    wav.extend_from_slice(&2u16.to_le_bytes()); // block align
    wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_len.to_le_bytes());
    wav.extend_from_slice(pcm);
    wav
}

pub fn reject_before_transcribe(
    _provider: Provider,
    metrics: AudioMetrics,
) -> Option<&'static str> {
    if metrics.too_short {
        return Some("Recording too short");
    }

    if metrics.likely_silent {
        return Some("No microphone input detected");
    }

    None
}

pub fn reject_transcript(
    config: &Config,
    transcript: &str,
    metrics: AudioMetrics,
) -> Option<&'static str> {
    let word_count = transcript.split_whitespace().count();
    if config.min_words > 0 && word_count < config.min_words {
        return Some("Transcript too short");
    }

    if !config.language.is_empty() {
        let non_ascii = transcript.chars().filter(|c| !c.is_ascii()).count();
        let total = transcript.chars().count();
        if total > 0 && non_ascii * 10 > total {
            return Some("Transcript language mismatch");
        }
    }

    if config.provider != Provider::Groq {
        return None;
    }

    let normalized = normalize(transcript);
    if normalized.is_empty() {
        return None;
    }

    if metrics.likely_silent && normalized.len() <= 48 {
        return Some("No microphone input detected");
    }

    if metrics.payload_bytes <= SHORT_HALLUCINATION_PCM_BYTES
        && is_common_hallucination(&normalized)
    {
        return Some("No microphone input detected");
    }

    None
}

fn normalize(text: &str) -> String {
    let cleaned: String = text
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c.is_ascii_whitespace() {
                c
            } else {
                ' '
            }
        })
        .collect();

    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

const HALLUCINATION_PHRASES: &[&str] = &[
    "thank you",
    "thanks for watching",
    "thanks for listening",
    "please subscribe",
    "like and subscribe",
    "see you next time",
    "see you in the next",
    "goodbye",
    "bye bye",
    "you",
];

fn is_common_hallucination(normalized: &str) -> bool {
    HALLUCINATION_PHRASES
        .iter()
        .any(|phrase| normalized == *phrase || normalized.starts_with(phrase))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn groq_config() -> Config {
        Config {
            provider: Provider::Groq,
            language: "en".to_string(),
            min_words: 0,
            ..Config::default()
        }
    }

    fn wav_from_samples(samples: &[i16]) -> Vec<u8> {
        let data_len = (samples.len() * 2) as u32;
        let riff_size = 36 + data_len;

        let mut out = Vec::new();
        out.extend_from_slice(b"RIFF");
        out.extend_from_slice(&riff_size.to_le_bytes());
        out.extend_from_slice(b"WAVE");

        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&16u32.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&1u16.to_le_bytes());
        out.extend_from_slice(&16000u32.to_le_bytes());
        out.extend_from_slice(&32000u32.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend_from_slice(&16u16.to_le_bytes());

        out.extend_from_slice(b"data");
        out.extend_from_slice(&data_len.to_le_bytes());
        for sample in samples {
            out.extend_from_slice(&sample.to_le_bytes());
        }

        out
    }

    fn make_unfinalized(mut wav: Vec<u8>) -> Vec<u8> {
        // Simulate pw-record being killed before fixing RIFF/data sizes.
        wav[4..8].copy_from_slice(&8u32.to_le_bytes());
        wav[40..44].copy_from_slice(&0u32.to_le_bytes());
        wav
    }

    #[test]
    fn detects_too_short_payload() {
        // 4000 samples = 8000 bytes < MIN_PCM_BYTES (16000)
        let wav = wav_from_samples(&vec![0i16; 4000]);
        let m = analyze_audio(&wav);
        assert!(m.too_short);
        assert_eq!(
            reject_before_transcribe(Provider::Groq, m),
            Some("Recording too short")
        );
    }

    #[test]
    fn detects_likely_silence() {
        let wav = wav_from_samples(&vec![0i16; 16000]);
        let m = analyze_audio(&wav);
        assert!(m.likely_silent);
        assert_eq!(
            reject_before_transcribe(Provider::Groq, m),
            Some("No microphone input detected")
        );
    }

    #[test]
    fn detects_silence_for_openai_too() {
        let wav = wav_from_samples(&vec![0i16; 16000]);
        let m = analyze_audio(&wav);
        assert!(m.likely_silent);
        assert_eq!(
            reject_before_transcribe(Provider::Openai, m),
            Some("No microphone input detected")
        );
    }

    #[test]
    fn ambient_noise_detected_as_silence() {
        // Simulate low-level ambient mic noise (mean_abs ~50, max ~500)
        let samples: Vec<i16> = (0..16000).map(|i| ((i % 100) as i16 - 50) * 3).collect();
        let wav = wav_from_samples(&samples);
        let m = analyze_audio(&wav);
        assert!(
            m.likely_silent,
            "ambient noise should be detected as silent: mean_abs={:.1}, max_abs={}",
            m.mean_abs, m.max_abs
        );
    }

    #[test]
    fn does_not_flag_voice_like_signal() {
        // Simulate speech-level signal (mean_abs ~1500, max_abs = 1500)
        let samples: Vec<i16> = (0..16000)
            .map(|i| if i % 20 < 10 { 1500 } else { -1500 })
            .collect();
        let wav = wav_from_samples(&samples);
        let m = analyze_audio(&wav);
        assert!(!m.too_short);
        assert!(!m.likely_silent);
        assert_eq!(reject_before_transcribe(Provider::Groq, m), None);
    }

    #[test]
    fn detects_flat_biased_capture_as_silence() {
        let wav = wav_from_samples(&vec![6200i16; 16000]);
        let m = analyze_audio(&wav);
        assert!(
            m.likely_silent,
            "biased capture should be detected as silent: dc_offset={:.1} centered_p90_abs={:.1} zc/s={:.1}",
            m.dc_offset, m.centered_p90_abs, m.centered_zero_crossings_per_sec
        );
    }

    #[test]
    fn does_not_flag_offset_voice_like_signal() {
        let samples: Vec<i16> = (0..16000)
            .map(|i| 6200 + if i % 20 < 10 { 1200 } else { -1200 })
            .collect();
        let wav = wav_from_samples(&samples);
        let m = analyze_audio(&wav);
        assert!(!m.too_short);
        assert!(!m.likely_silent);
        assert_eq!(reject_before_transcribe(Provider::Groq, m), None);
    }

    #[test]
    fn flags_short_thank_you_as_hallucination() {
        // 10000 samples = 20000 bytes < SHORT_HALLUCINATION_PCM_BYTES (48000)
        let wav = wav_from_samples(&vec![500i16; 10000]);
        let m = analyze_audio(&wav);
        assert_eq!(
            reject_transcript(&groq_config(), "Thank you.", m),
            Some("No microphone input detected")
        );
    }

    #[test]
    fn flags_hallucination_variants() {
        let wav = wav_from_samples(&vec![500i16; 10000]);
        let m = analyze_audio(&wav);
        for phrase in &[
            "Thank you for watching!",
            "Thanks for listening.",
            "Please subscribe",
            "See you next time!",
        ] {
            assert_eq!(
                reject_transcript(&groq_config(), phrase, m),
                Some("No microphone input detected"),
                "should reject: {phrase}"
            );
        }
    }

    #[test]
    fn handles_unfinalized_wav_headers() {
        let samples: Vec<i16> = (0..16000)
            .map(|i| if i % 20 < 10 { 1500 } else { -1500 })
            .collect();
        let wav = make_unfinalized(wav_from_samples(&samples));
        let m = analyze_audio(&wav);
        assert!(!m.too_short);
        assert!(!m.likely_silent);
    }

    #[test]
    fn wrap_pcm_creates_valid_wav() {
        let samples: Vec<i16> = (0..8000).map(|i| ((i % 100) as i16) * 50).collect();
        let pcm: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let wav = wrap_pcm_as_wav(&pcm);

        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");

        let m = analyze_audio(&wav);
        assert_eq!(m.sample_count, 8000);
        assert!(!m.too_short);
    }
}
