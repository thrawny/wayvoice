const WAV_HEADER_BYTES: usize = 44;
const SAMPLE_RATE_HZ: f64 = 16000.0;
const PCM_MAX_ABS: f64 = 32767.0;

#[derive(Debug, Clone, Default)]
pub struct SignalStats {
    pub sample_count: u64,
    pub mean: f64,
    pub mean_abs: f64,
    pub rms: f64,
    pub max_abs: i32,
    pub centered_mean_abs: f64,
    pub centered_rms: f64,
    pub centered_p90_abs: f64,
    pub centered_zero_crossings: u64,
}

impl SignalStats {
    pub fn centered_zero_crossings_per_sec(&self) -> f64 {
        if self.sample_count < 2 {
            return 0.0;
        }

        self.centered_zero_crossings as f64 / (self.sample_count as f64 / SAMPLE_RATE_HZ)
    }

    pub fn normalized_rms(&self) -> f32 {
        (self.rms / PCM_MAX_ABS).min(1.0) as f32
    }

    pub fn normalized_display_level(&self) -> f32 {
        (self.centered_p90_abs / PCM_MAX_ABS).min(1.0) as f32
    }
}

pub fn analyze_audio(audio_data: &[u8]) -> SignalStats {
    analyze_pcm_s16le(pcm_payload(audio_data))
}

pub fn analyze_pcm_s16le(pcm_s16le: &[u8]) -> SignalStats {
    let mut samples = Vec::with_capacity(pcm_s16le.len() / 2);
    for chunk in pcm_s16le.chunks_exact(2) {
        samples.push(i16::from_le_bytes([chunk[0], chunk[1]]) as i32);
    }

    if samples.is_empty() {
        return SignalStats::default();
    }

    let sample_count = samples.len() as u64;
    let mean =
        samples.iter().map(|sample| *sample as i64).sum::<i64>() as f64 / sample_count as f64;

    let mut sum_abs = 0.0;
    let mut sum_sq = 0.0;
    let mut max_abs = 0i32;
    let mut centered_sum_abs = 0.0;
    let mut centered_sum_sq = 0.0;
    let mut centered_abs = Vec::with_capacity(samples.len());
    let mut centered_zero_crossings = 0u64;
    let mut prev_centered = None::<f64>;

    for sample in samples {
        let sample_f = sample as f64;
        let abs = sample.abs();
        sum_abs += abs as f64;
        sum_sq += sample_f * sample_f;
        max_abs = max_abs.max(abs);

        let centered = sample_f - mean;
        let centered_abs_sample = centered.abs();
        centered_sum_abs += centered_abs_sample;
        centered_sum_sq += centered * centered;
        centered_abs.push(centered_abs_sample);

        if let Some(prev) = prev_centered {
            let crossed_zero = (prev < 0.0 && centered >= 0.0) || (prev >= 0.0 && centered < 0.0);
            if crossed_zero {
                centered_zero_crossings += 1;
            }
        }
        prev_centered = Some(centered);
    }

    centered_abs.sort_by(|a, b| a.total_cmp(b));
    let centered_p90_abs = centered_abs[percentile_index(centered_abs.len(), 90, 100)];

    SignalStats {
        sample_count,
        mean,
        mean_abs: sum_abs / sample_count as f64,
        rms: (sum_sq / sample_count as f64).sqrt(),
        max_abs,
        centered_mean_abs: centered_sum_abs / sample_count as f64,
        centered_rms: (centered_sum_sq / sample_count as f64).sqrt(),
        centered_p90_abs,
        centered_zero_crossings,
    }
}

pub fn pcm_payload(audio_data: &[u8]) -> &[u8] {
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

fn percentile_index(len: usize, numerator: usize, denominator: usize) -> usize {
    debug_assert!(len > 0);
    debug_assert!(numerator <= denominator);

    ((len - 1) * numerator).div_ceil(denominator)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm_from_samples(samples: &[i16]) -> Vec<u8> {
        let mut pcm = Vec::with_capacity(samples.len() * 2);
        for sample in samples {
            pcm.extend_from_slice(&sample.to_le_bytes());
        }
        pcm
    }

    #[test]
    fn centered_stats_remove_dc_offset() {
        let samples = vec![6200i16; 1600];
        let stats = analyze_pcm_s16le(&pcm_from_samples(&samples));

        assert!((stats.mean - 6200.0).abs() < 0.01);
        assert_eq!(stats.max_abs, 6200);
        assert!(stats.centered_mean_abs < 0.01);
        assert!(stats.centered_p90_abs < 0.01);
        assert_eq!(stats.centered_zero_crossings, 0);
    }

    #[test]
    fn centered_zero_crossings_track_voice_like_motion() {
        let samples: Vec<i16> = (0..1600)
            .map(|i| 6200 + if i % 20 < 10 { 1200 } else { -1200 })
            .collect();
        let stats = analyze_pcm_s16le(&pcm_from_samples(&samples));

        assert!(stats.mean > 5000.0);
        assert!(stats.centered_p90_abs >= 1200.0);
        assert!(stats.centered_zero_crossings_per_sec() > 1000.0);
    }
}
