use crate::config::Provider;

const WAV_HEADER_BYTES: usize = 44;
const MIN_PCM_BYTES: usize = 6000;
const MIN_SILENCE_SAMPLES: u64 = 8000;
const SILENCE_MEAN_ABS_MAX: f64 = 6.0;
const SILENCE_MAX_ABS_MAX: i32 = 120;
const SHORT_HALLUCINATION_PCM_BYTES: usize = 12000;

#[derive(Debug, Clone, Copy)]
pub struct AudioMetrics {
    pub payload_bytes: usize,
    pub sample_count: u64,
    pub mean_abs: f64,
    pub max_abs: i32,
    pub too_short: bool,
    pub likely_silent: bool,
}

pub fn analyze_audio(audio_data: &[u8]) -> AudioMetrics {
    let payload = pcm_payload(audio_data);

    let mut sample_count = 0u64;
    let mut sum_abs = 0u64;
    let mut max_abs = 0i32;

    for chunk in payload.chunks_exact(2) {
        let sample = i16::from_le_bytes([chunk[0], chunk[1]]) as i32;
        let abs = sample.abs();
        sample_count += 1;
        sum_abs += abs as u64;
        max_abs = max_abs.max(abs);
    }

    let mean_abs = if sample_count == 0 {
        0.0
    } else {
        sum_abs as f64 / sample_count as f64
    };

    let too_short = payload.len() < MIN_PCM_BYTES;
    let likely_silent = sample_count >= MIN_SILENCE_SAMPLES
        && mean_abs <= SILENCE_MEAN_ABS_MAX
        && max_abs <= SILENCE_MAX_ABS_MAX;

    AudioMetrics {
        payload_bytes: payload.len(),
        sample_count,
        mean_abs,
        max_abs,
        too_short,
        likely_silent,
    }
}

fn pcm_payload(audio_data: &[u8]) -> &[u8] {
    if audio_data.len() < 12 || &audio_data[0..4] != b"RIFF" || &audio_data[8..12] != b"WAVE" {
        return audio_data.get(WAV_HEADER_BYTES..).unwrap_or(&[]);
    }

    let mut offset = 12usize;
    while offset + 8 <= audio_data.len() {
        let chunk_id = &audio_data[offset..offset + 4];
        let chunk_size = u32::from_le_bytes([
            audio_data[offset + 4],
            audio_data[offset + 5],
            audio_data[offset + 6],
            audio_data[offset + 7],
        ]) as usize;

        let start = offset + 8;
        if start > audio_data.len() {
            return audio_data.get(WAV_HEADER_BYTES..).unwrap_or(&[]);
        }

        if chunk_id == b"data" {
            if chunk_size == 0 {
                return &audio_data[start..];
            }

            let Some(end) = start.checked_add(chunk_size) else {
                return &audio_data[start..];
            };

            return if end <= audio_data.len() {
                &audio_data[start..end]
            } else {
                &audio_data[start..]
            };
        }

        let Some(next) = start.checked_add(chunk_size) else {
            break;
        };
        offset = if chunk_size % 2 == 1 {
            match next.checked_add(1) {
                Some(v) => v,
                None => break,
            }
        } else {
            next
        };
    }

    audio_data.get(WAV_HEADER_BYTES..).unwrap_or(&[])
}

pub fn reject_before_transcribe(provider: Provider, metrics: AudioMetrics) -> Option<&'static str> {
    if metrics.too_short {
        return Some("Recording too short");
    }

    if provider == Provider::Groq && metrics.likely_silent {
        return Some("No microphone input detected");
    }

    None
}

pub fn reject_transcript(
    provider: Provider,
    transcript: &str,
    metrics: AudioMetrics,
) -> Option<&'static str> {
    if provider != Provider::Groq {
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

fn is_common_hallucination(normalized: &str) -> bool {
    matches!(
        normalized,
        "thank you" | "thank you for watching" | "thanks for watching" | "you"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let wav = wav_from_samples(&vec![0i16; 2000]);
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
    fn does_not_flag_voice_like_signal() {
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
    fn flags_short_thank_you_as_hallucination() {
        let wav = wav_from_samples(&vec![0i16; 4000]);
        let m = analyze_audio(&wav);
        assert_eq!(
            reject_transcript(Provider::Groq, "Thank you.", m),
            Some("No microphone input detected")
        );
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
}
