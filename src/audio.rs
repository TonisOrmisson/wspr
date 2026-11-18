//! Pure audio helpers: downmix and resample to 16 kHz mono, WAV encoding, beep synthesis.

pub const TARGET_RATE: u32 = 16_000;

/// Interleaved f32 samples at `rate` Hz with `channels` channels -> 16 kHz mono i16.
/// Downsampling averages each output window (a box filter), which is enough for speech.
pub fn to_mono_16k(interleaved: &[f32], channels: u16, rate: u32) -> Vec<i16> {
    let ch = channels.max(1) as usize;
    let mono: Vec<f32> = interleaved
        .chunks(ch)
        .map(|f| f.iter().sum::<f32>() / f.len() as f32)
        .collect();
    let ratio = rate as f64 / TARGET_RATE as f64;
    let out_len = (mono.len() as f64 / ratio) as usize;
    (0..out_len)
        .map(|i| {
            let start = (i as f64 * ratio) as usize;
            let end = (((i + 1) as f64 * ratio) as usize).clamp(start + 1, mono.len());
            let avg = mono[start..end].iter().sum::<f32>() / (end - start) as f32;
            (avg.clamp(-1.0, 1.0) * i16::MAX as f32) as i16
        })
        .collect()
}

/// 16-bit mono PCM WAV file bytes.
pub fn wav(samples: &[i16], rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&1u16.to_le_bytes()); // mono
    v.extend_from_slice(&rate.to_le_bytes());
    v.extend_from_slice(&(rate * 2).to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        v.extend_from_slice(&s.to_le_bytes());
    }
    v
}

/// A short sine beep as WAV bytes.
pub fn beep(freq: f32, ms: u32) -> Vec<u8> {
    let rate = 22_050u32;
    let n = rate * ms / 1000;
    let samples: Vec<i16> = (0..n)
        .map(|i| {
            let t = i as f32 / rate as f32;
            // 5 ms fade in and out, otherwise the speaker clicks.
            let fade = (i.min(n - i) as f32 / (rate as f32 * 0.005)).min(1.0);
            ((t * freq * std::f32::consts::TAU).sin() * fade * 0.3 * i16::MAX as f32) as i16
        })
        .collect();
    wav(&samples, rate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stereo_48k_becomes_mono_16k() {
        let input = vec![0.5f32; 4800 * 2]; // 0.1 s stereo
        let out = to_mono_16k(&input, 2, 48_000);
        assert_eq!(out.len(), 1600);
        assert!(out.iter().all(|&s| s == (0.5 * i16::MAX as f32) as i16));
    }

    #[test]
    fn upsampling_and_empty_input_do_not_panic() {
        assert_eq!(to_mono_16k(&[0.1; 800], 1, 8_000).len(), 1600);
        assert!(to_mono_16k(&[], 2, 44_100).is_empty());
    }

    #[test]
    fn wav_header_is_consistent() {
        let w = wav(&[1, 2, 3], 16_000);
        assert_eq!(w.len(), 44 + 6);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap()), 36 + 6);
        assert_eq!(u32::from_le_bytes(w[40..44].try_into().unwrap()), 6);
        assert_eq!(beep(880.0, 80).len(), 44 + 2 * 1764);
    }
}
