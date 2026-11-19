//! Minimal WAV reader. WAV is only ever *read* (as test or CLI input); nothing in
//! SprkNotes writes audio to disk (CLAUDE.md hard rule 1).

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WavError {
    #[error("not a RIFF/WAVE file")]
    NotWav,
    #[error("unsupported WAV format (need PCM 16/24/32-bit or 32-bit float)")]
    Unsupported,
    #[error("truncated or malformed WAV")]
    Malformed,
}

/// Decoded audio: mono f32 in [-1, 1] at `sample_rate`.
#[derive(Debug, Clone, PartialEq)]
pub struct Audio {
    pub sample_rate: u32,
    pub samples: Vec<f32>,
}

/// (format tag, channels, sample rate, bits per sample)
type Fmt = (u16, u16, u32, u16);

fn u16_at(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}
fn u32_at(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}

/// Parse a WAV held in memory and mix it down to mono.
pub fn parse(bytes: &[u8]) -> Result<Audio, WavError> {
    if bytes.get(0..4) != Some(b"RIFF") || bytes.get(8..12) != Some(b"WAVE") {
        return Err(WavError::NotWav);
    }
    let mut fmt: Option<Fmt> = None;
    let mut data: Option<&[u8]> = None;
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let id = bytes.get(pos..pos + 4).ok_or(WavError::Malformed)?;
        let size = u32_at(bytes, pos + 4).ok_or(WavError::Malformed)? as usize;
        let body = pos + 8;
        // Streaming writers may leave the data size as 0 or too large: clamp to the file.
        let end = body.saturating_add(size).min(bytes.len());
        let chunk = bytes.get(body..end).ok_or(WavError::Malformed)?;
        match id {
            b"fmt " => {
                let tag = u16_at(chunk, 0).ok_or(WavError::Malformed)?;
                let ch = u16_at(chunk, 2).ok_or(WavError::Malformed)?;
                let rate = u32_at(chunk, 4).ok_or(WavError::Malformed)?;
                let bits = u16_at(chunk, 14).ok_or(WavError::Malformed)?;
                // 0xFFFE = WAVE_FORMAT_EXTENSIBLE: the real tag is the first 2 bytes of the sub-format.
                let tag = if tag == 0xFFFE {
                    u16_at(chunk, 24).ok_or(WavError::Malformed)?
                } else {
                    tag
                };
                fmt = Some((tag, ch, rate, bits));
            }
            b"data" => data = Some(chunk),
            _ => {}
        }
        pos = body.saturating_add(size).saturating_add(size & 1);
    }
    let (tag, ch, rate, bits) = fmt.ok_or(WavError::Malformed)?;
    let data = data.ok_or(WavError::Malformed)?;
    if ch == 0 || rate == 0 {
        return Err(WavError::Malformed);
    }
    let bytes_per = usize::from(bits / 8);
    let frame = bytes_per * usize::from(ch);
    if frame == 0 {
        return Err(WavError::Malformed);
    }
    let mut samples = Vec::with_capacity(data.len() / frame);
    for f in data.chunks_exact(frame) {
        let mut sum = 0.0f32;
        for c in f.chunks_exact(bytes_per) {
            sum += match (tag, bits) {
                (1, 16) => i16::from_le_bytes([c[0], c[1]]) as f32 / 32768.0,
                (1, 24) => (i32::from_le_bytes([0, c[0], c[1], c[2]]) >> 8) as f32 / 8_388_608.0,
                (1, 32) => i32::from_le_bytes([c[0], c[1], c[2], c[3]]) as f32 / 2_147_483_648.0,
                (3, 32) => f32::from_le_bytes([c[0], c[1], c[2], c[3]]),
                _ => return Err(WavError::Unsupported),
            };
        }
        samples.push(sum / f32::from(ch));
    }
    Ok(Audio {
        sample_rate: rate,
        samples,
    })
}

/// Read a WAV file as input. (Read-only: this crate has no audio writer.)
pub fn read_file(path: &std::path::Path) -> Result<Audio, WavError> {
    let bytes = std::fs::read(path).map_err(|_| WavError::NotWav)?;
    parse(&bytes)
}

/// Linear-interpolation resample to 16 kHz mono (adequate for speech input to the engines).
pub fn to_16k(audio: &Audio) -> Vec<f32> {
    const TARGET: u32 = 16_000;
    if audio.sample_rate == TARGET || audio.samples.is_empty() {
        return audio.samples.clone();
    }
    let ratio = f64::from(audio.sample_rate) / f64::from(TARGET);
    let n = (audio.samples.len() as f64 / ratio) as usize;
    (0..n)
        .map(|i| {
            let p = i as f64 * ratio;
            let i0 = p as usize;
            let frac = (p - i0 as f64) as f32;
            let a = audio.samples.get(i0).copied().unwrap_or(0.0);
            let b = audio.samples.get(i0 + 1).copied().unwrap_or(a);
            a + (b - a) * frac
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav16(rate: u32, ch: u16, samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut v = b"RIFF".to_vec();
        v.extend((36 + data.len() as u32).to_le_bytes());
        v.extend(b"WAVEfmt ");
        v.extend(16u32.to_le_bytes());
        v.extend(1u16.to_le_bytes());
        v.extend(ch.to_le_bytes());
        v.extend(rate.to_le_bytes());
        v.extend((rate * u32::from(ch) * 2).to_le_bytes());
        v.extend((ch * 2).to_le_bytes());
        v.extend(16u16.to_le_bytes());
        v.extend(b"data");
        v.extend((data.len() as u32).to_le_bytes());
        v.extend(data);
        v
    }

    #[test]
    fn parses_mono_pcm16() {
        let a = parse(&wav16(16_000, 1, &[0, 16384, -32768])).unwrap();
        assert_eq!(a.sample_rate, 16_000);
        assert_eq!(a.samples, vec![0.0, 0.5, -1.0]);
    }

    #[test]
    fn mixes_stereo_to_mono() {
        let a = parse(&wav16(16_000, 2, &[16384, -16384, 8192, 8192])).unwrap();
        assert_eq!(a.samples, vec![0.0, 0.25]);
    }

    #[test]
    fn rejects_garbage_and_truncated() {
        assert_eq!(parse(b"nope"), Err(WavError::NotWav));
        let mut w = wav16(16_000, 1, &[1, 2, 3]);
        w.truncate(30);
        assert!(parse(&w).is_err());
    }

    #[test]
    fn tolerates_oversized_data_chunk() {
        let mut w = wav16(16_000, 1, &[100, 200]);
        let len = w.len();
        w[len - 8..len - 4].copy_from_slice(&u32::MAX.to_le_bytes()); // data size lies
        assert_eq!(parse(&w).unwrap().samples.len(), 2);
    }

    #[test]
    fn resamples_48k_to_16k() {
        let a = Audio {
            sample_rate: 48_000,
            samples: vec![0.0; 4800],
        };
        assert_eq!(to_16k(&a).len(), 1600);
        let same = Audio {
            sample_rate: 16_000,
            samples: vec![0.1; 10],
        };
        assert_eq!(to_16k(&same), same.samples);
    }
}
