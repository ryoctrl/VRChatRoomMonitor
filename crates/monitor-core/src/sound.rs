//! 通知音。音源ファイルは持たず、ここに書いた音符から合成する（すべてこのプロジェクトのオリジナル）。

use serde::{Deserialize, Serialize};
use std::f64::consts::TAU;
use std::sync::OnceLock;

const SAMPLE_RATE: u32 = 44_100;
/// 音割れを避けるため、最大値に対してこの割合に揃える
const PEAK: f64 = 0.6;
const ATTACK_SECONDS: f64 = 0.004;
const RELEASE_SECONDS: f64 = 0.02;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Sound {
    Chime,
    Bell,
    Marimba,
    Pop,
    Soft,
    /// Windows の標準の通知音
    Windows,
    Silent,
}

struct Note {
    start: f64,
    length: f64,
    from_hz: f64,
    to_hz: f64,
    /// 減衰の速さ（大きいほど短く響く）
    decay: f64,
    gain: f64,
    /// 倍音（基本の周波数に対する倍率と大きさ）
    partials: &'static [(f64, f64)],
}

const fn note(
    start: f64,
    length: f64,
    hz: f64,
    decay: f64,
    gain: f64,
    partials: &'static [(f64, f64)],
) -> Note {
    Note {
        start,
        length,
        from_hz: hz,
        to_hz: hz,
        decay,
        gain,
        partials,
    }
}

const WARM: &[(f64, f64)] = &[(1.0, 1.0), (2.0, 0.25), (3.0, 0.08)];
const METAL: &[(f64, f64)] = &[(1.0, 1.0), (2.76, 0.3), (5.4, 0.1)];
const WOOD: &[(f64, f64)] = &[(1.0, 1.0), (4.0, 0.2)];
const ROUND: &[(f64, f64)] = &[(1.0, 1.0), (2.0, 0.1)];
const PURE: &[(f64, f64)] = &[(1.0, 1.0)];

/// 上がる 2 音
const CHIME: &[Note] = &[
    note(0.00, 0.45, 659.25, 5.0, 0.8, WARM),
    note(0.13, 0.60, 880.00, 5.0, 1.0, WARM),
];
/// 高い金属音を 1 回
const BELL: &[Note] = &[note(0.00, 0.90, 1046.50, 5.5, 1.0, METAL)];
/// 木を叩くような短い 3 音
const MARIMBA: &[Note] = &[
    note(0.00, 0.30, 523.25, 14.0, 0.9, WOOD),
    note(0.09, 0.30, 659.25, 14.0, 0.9, WOOD),
    note(0.18, 0.35, 783.99, 14.0, 1.0, WOOD),
];
/// 音程が上がる短い 1 音
const POP: &[Note] = &[Note {
    start: 0.0,
    length: 0.12,
    from_hz: 420.0,
    to_hz: 880.0,
    decay: 16.0,
    gain: 1.0,
    partials: PURE,
}];
/// 下がる低めの 2 音
const SOFT: &[Note] = &[
    note(0.00, 0.45, 440.00, 5.0, 1.0, ROUND),
    note(0.16, 0.60, 329.63, 5.0, 0.9, ROUND),
];

fn render(notes: &[Note]) -> Vec<i16> {
    let rate = f64::from(SAMPLE_RATE);
    let seconds = notes
        .iter()
        .map(|note| note.start + note.length)
        .fold(0.0, f64::max);
    let mut mixed = vec![0.0_f64; (seconds * rate) as usize + 1];
    for note in notes {
        let first = (note.start * rate) as usize;
        let count = (note.length * rate) as usize;
        for (index, sample) in mixed[first..].iter_mut().take(count).enumerate() {
            let t = index as f64 / rate;
            // 音程が変わっても波形が途切れないよう、周波数を時間で積分した値から波を作る
            let cycles =
                note.from_hz * t + (note.to_hz - note.from_hz) * t * t / (2.0 * note.length);
            let tone: f64 = note
                .partials
                .iter()
                .map(|&(ratio, level)| level * (TAU * ratio * cycles).sin())
                .sum();
            // 始まりと終わりを滑らかにして、再生時のノイズを防ぐ
            let fade_in = (t / ATTACK_SECONDS).min(1.0);
            let fade_out = ((note.length - t) / RELEASE_SECONDS).min(1.0);
            *sample += note.gain * tone * (-note.decay * t).exp() * fade_in * fade_out;
        }
    }
    let peak = mixed
        .iter()
        .fold(0.0_f64, |largest, sample| largest.max(sample.abs()));
    mixed
        .iter()
        .map(|sample| (sample / peak * PEAK * f64::from(i16::MAX)) as i16)
        .collect()
}

/// モノラル 16 ビットの WAV 形式にする。
fn encode(samples: &[i16]) -> Vec<u8> {
    const HEADER_SIZE: usize = 44;
    const BYTES_PER_SAMPLE: u16 = 2;
    let data_size = samples.len() as u32 * u32::from(BYTES_PER_SAMPLE);
    let mut wav = Vec::with_capacity(HEADER_SIZE + data_size as usize);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(HEADER_SIZE as u32 - 8 + data_size).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16_u32.to_le_bytes());
    wav.extend_from_slice(&1_u16.to_le_bytes()); // PCM
    wav.extend_from_slice(&1_u16.to_le_bytes()); // チャンネル数
    wav.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    wav.extend_from_slice(&(SAMPLE_RATE * u32::from(BYTES_PER_SAMPLE)).to_le_bytes());
    wav.extend_from_slice(&BYTES_PER_SAMPLE.to_le_bytes());
    wav.extend_from_slice(&16_u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());
    for sample in samples {
        wav.extend_from_slice(&sample.to_le_bytes());
    }
    wav
}

impl Sound {
    /// 再生する WAV のデータ。Windows の標準音と無音には無い。
    /// 非同期の再生中はデータが残っている必要があるので、一度作ったら持ち続ける。
    pub fn wav(self) -> Option<&'static [u8]> {
        static CACHE: [OnceLock<Vec<u8>>; 5] = [const { OnceLock::new() }; 5];
        let (slot, notes) = match self {
            Self::Chime => (0, CHIME),
            Self::Bell => (1, BELL),
            Self::Marimba => (2, MARIMBA),
            Self::Pop => (3, POP),
            Self::Soft => (4, SOFT),
            Self::Windows | Self::Silent => return None,
        };
        Some(CACHE[slot].get_or_init(|| encode(&render(notes))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGINALS: [(Sound, &str); 5] = [
        (Sound::Chime, "chime"),
        (Sound::Bell, "bell"),
        (Sound::Marimba, "marimba"),
        (Sound::Pop, "pop"),
        (Sound::Soft, "soft"),
    ];

    fn samples(wav: &[u8]) -> Vec<i16> {
        wav[44..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| i16::from_le_bytes(*pair))
            .collect()
    }

    #[test]
    fn original_sounds_are_valid_wav_data() {
        for (sound, name) in ORIGINALS {
            let wav = sound.wav().unwrap();
            assert_eq!(&wav[..4], b"RIFF", "{name}");
            assert_eq!(&wav[8..16], b"WAVEfmt ", "{name}");
            assert_eq!(&wav[36..40], b"data", "{name}");
            let riff_size = u32::from_le_bytes(wav[4..8].try_into().unwrap()) as usize;
            let data_size = u32::from_le_bytes(wav[40..44].try_into().unwrap()) as usize;
            assert_eq!(riff_size + 8, wav.len(), "{name}");
            assert_eq!(data_size + 44, wav.len(), "{name}");
        }
    }

    #[test]
    fn original_sounds_are_short_and_do_not_clip_or_click() {
        for (sound, name) in ORIGINALS {
            let samples = samples(sound.wav().unwrap());
            let seconds = samples.len() as f64 / f64::from(SAMPLE_RATE);
            assert!((0.1..=1.0).contains(&seconds), "{name}: {seconds}");
            let peak = samples.iter().map(|s| i32::from(*s).abs()).max().unwrap();
            let limit = (PEAK * f64::from(i16::MAX)) as i32;
            assert!(peak <= limit && peak >= limit - 1, "{name}: {peak}");
            assert_eq!(samples[0], 0, "{name}");
            assert!(samples[samples.len() - 1].abs() < 100, "{name}");
        }
    }

    #[test]
    fn original_sounds_differ_from_each_other() {
        for (index, (first, _)) in ORIGINALS.iter().enumerate() {
            for (second, _) in &ORIGINALS[index + 1..] {
                assert_ne!(first.wav(), second.wav());
            }
        }
    }

    #[test]
    fn standard_and_silent_have_no_data() {
        assert_eq!(Sound::Windows.wav(), None);
        assert_eq!(Sound::Silent.wav(), None);
    }

    #[test]
    fn names_match_what_the_ui_sends() {
        for (sound, name) in ORIGINALS
            .into_iter()
            .chain([(Sound::Windows, "windows"), (Sound::Silent, "silent")])
        {
            assert_eq!(serde_json::to_value(sound).unwrap(), name);
            assert_eq!(serde_json::from_value::<Sound>(name.into()).unwrap(), sound);
        }
    }

    /// 音を聞いて確かめるためのファイルを `target/sounds` に書き出す。
    /// 実行: `cargo test -p monitor-core writes_sounds -- --ignored`
    #[test]
    #[ignore = "音を聞いて確かめるときだけ実行する"]
    fn writes_sounds_for_listening() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/sounds");
        std::fs::create_dir_all(&dir).unwrap();
        for (sound, name) in ORIGINALS {
            std::fs::write(dir.join(format!("{name}.wav")), sound.wav().unwrap()).unwrap();
        }
    }
}
