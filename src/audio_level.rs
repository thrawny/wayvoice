use crate::audio_signal::analyze_pcm_s16le;

/// Compute RMS of raw s16le PCM samples, normalized to 0.0..=1.0.
pub fn rms_level(pcm_s16le: &[u8]) -> f32 {
    analyze_pcm_s16le(pcm_s16le).normalized_rms()
}

/// Compute a display-friendly audio level that ignores DC offset and isolated spikes.
pub fn display_level(pcm_s16le: &[u8]) -> f32 {
    analyze_pcm_s16le(pcm_s16le).normalized_display_level()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_gives_zero() {
        let pcm = vec![0u8; 3200];
        assert_eq!(rms_level(&pcm), 0.0);
    }

    #[test]
    fn max_amplitude_gives_one() {
        let mut pcm = Vec::new();
        for _ in 0..1600 {
            pcm.extend_from_slice(&i16::MAX.to_le_bytes());
        }
        let level = rms_level(&pcm);
        assert!((level - 1.0).abs() < 0.001);
    }

    #[test]
    fn known_sine_wave() {
        let mut pcm = Vec::new();
        for i in 0..16000 {
            let t = i as f64 / 16000.0;
            let sample = (16383.0 * (2.0 * std::f64::consts::PI * 440.0 * t).sin()) as i16;
            pcm.extend_from_slice(&sample.to_le_bytes());
        }
        let level = rms_level(&pcm);
        // RMS of sine = amplitude / sqrt(2), so 0.5 / 1.414 ≈ 0.354
        assert!((level - 0.354).abs() < 0.02, "expected ~0.354, got {level}");
    }

    #[test]
    fn empty_gives_zero() {
        assert_eq!(rms_level(&[]), 0.0);
    }

    #[test]
    fn odd_byte_count_ignores_trailing() {
        // One sample of 16384 (0x4000) + trailing byte
        let pcm = vec![0x00, 0x40, 0xFF];
        let level = rms_level(&pcm);
        assert!((level - 0.5).abs() < 0.01, "expected ~0.5, got {level}");
    }

    #[test]
    fn display_level_ignores_dc_offset() {
        let mut pcm = Vec::new();
        for _ in 0..1600 {
            pcm.extend_from_slice(&6200i16.to_le_bytes());
        }

        assert!(display_level(&pcm) < 0.001);
    }

    #[test]
    fn display_level_still_tracks_voice_like_motion() {
        let mut pcm = Vec::new();
        for i in 0..1600 {
            let sample: i16 = 6200 + if i % 20 < 10 { 1200 } else { -1200 };
            pcm.extend_from_slice(&sample.to_le_bytes());
        }

        assert!(display_level(&pcm) > 0.03);
    }
}
