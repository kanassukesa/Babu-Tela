//! Short synthesized cues, one per event, so the app ships no audio files.
//!
//! Each cue is a few notes that slide into their pitch; rising shapes mean
//! "on / arrived", falling shapes mean "off / gone". They are computed into
//! a buffer once and played through the output only: nothing of them goes
//! into the microphone, so the others never hear my mute or deafen.

use std::collections::{HashMap, HashSet};
use std::num::NonZero;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use rodio::buffer::SamplesBuffer;
use rodio::{DeviceSinkBuilder, MixerDeviceSink};

const RATE: u32 = 48_000;

/// The bots of a test on one PC play nothing at all (`BABUTELA_TESTE_SAIDA=
/// mudo`): their speakers are mine, and a screen shared with its sound
/// would take what they play of it and send it back, over and over.
pub fn test_silent() -> bool {
    static SILENT: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SILENT.get_or_init(|| std::env::var("BABUTELA_TESTE_SAIDA").is_ok_and(|v| v == "mudo"))
}

/// How long a sound to be played waits for the output while it opens (it
/// is opening right after the app starts, and after the device changed).
const OPEN_WAIT: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Cue {
    Join,
    Leave,
    Disconnect,
    Mute,
    Unmute,
    Deafen,
    Undeafen,
    StreamStart,
    StreamStop,
    Message,
    /// No sound at all, for calls that end without the user noticing.
    None,
}

/// One note of a cue: its pitch, when it starts, and the pitch it slides
/// in from (as a ratio of its own; 1.0 is no slide).
#[derive(Clone, Copy)]
struct Note {
    freq: f32,
    at: f32,
    from: f32,
}

/// A note, written so the lists of them stay `'static`.
macro_rules! n {
    ($freq:expr, $at:expr, $from:expr) => {
        Note {
            freq: $freq,
            at: $at,
            from: $from,
        }
    };
}

/// How a cue sounds: its notes, how long each rings, how loud it is, how
/// fast the slides are, and how bright the strike on each note is.
struct Shape {
    notes: &'static [Note],
    ring: f32,
    gain: f32,
    glide: f32,
    strike: f32,
}

// Note frequencies.
const D4: f32 = 293.66;
const E4: f32 = 329.63;
const A4: f32 = 440.0;
const B4: f32 = 493.88;
const CS5: f32 = 554.37;
const D5: f32 = 587.33;
const E5: f32 = 659.25;
const FS5: f32 = 739.99;
const A5: f32 = 880.0;
const B5: f32 = 987.77;
const D6: f32 = 1174.66;

impl Cue {
    /// Every cue that has a sound, in the order the settings list them.
    pub const ALL: [Cue; 10] = [
        Cue::Join,
        Cue::Leave,
        Cue::Disconnect,
        Cue::Mute,
        Cue::Unmute,
        Cue::Deafen,
        Cue::Undeafen,
        Cue::StreamStart,
        Cue::StreamStop,
        Cue::Message,
    ];

    /// The name saved in prefs.json (the keys the old page used).
    pub fn key(self) -> &'static str {
        match self {
            Cue::Join => "join",
            Cue::Leave => "leave",
            Cue::Disconnect => "disconnect",
            Cue::Mute => "mute",
            Cue::Unmute => "unmute",
            Cue::Deafen => "deafen",
            Cue::Undeafen => "undeafen",
            Cue::StreamStart => "stream_start",
            Cue::StreamStop => "stream_stop",
            Cue::Message => "message",
            Cue::None => "",
        }
    }

    pub fn label(self) -> &'static str {
        use crate::t;
        match self {
            Cue::Join => t!("sound.join"),
            Cue::Leave => t!("sound.leave"),
            Cue::Disconnect => t!("sound.disconnect"),
            Cue::Mute => t!("sound.mute"),
            Cue::Unmute => t!("sound.unmute"),
            Cue::Deafen => t!("sound.deafen"),
            Cue::Undeafen => t!("sound.undeafen"),
            Cue::StreamStart => t!("sound.stream_start"),
            Cue::StreamStop => t!("sound.stream_stop"),
            Cue::Message => t!("sound.message"),
            Cue::None => "",
        }
    }

    /// Rounded "bloops" that slide into their pitch: up for on / arrived,
    /// down for off / gone. Mute and deafen are short and low, since they
    /// answer a click of mine; joins and leaves are brighter, since they
    /// tell me about someone else.
    fn shape(self) -> Option<Shape> {
        let s = |notes, ring, gain, glide, strike| {
            Some(Shape {
                notes,
                ring,
                gain,
                glide,
                strike,
            })
        };
        match self {
            // Two notes a fifth apart, the second sliding up into place.
            Cue::Join => s(&[n!(E5, 0.0, 0.92), n!(B5, 0.075, 0.86)], 0.26, 1.0, 0.05, 0.6),
            Cue::Leave => s(&[n!(B5, 0.0, 1.08), n!(E5, 0.075, 1.16)], 0.26, 1.0, 0.05, 0.6),
            Cue::Disconnect => s(
                &[n!(E5, 0.0, 1.0), n!(B4, 0.1, 1.06), n!(E4, 0.2, 1.1)],
                0.4,
                1.0,
                0.06,
                0.5,
            ),
            // A quick drop, a quick rise: a switch going off and on.
            Cue::Mute => s(&[n!(A5, 0.0, 1.0), n!(D5, 0.055, 1.25)], 0.13, 0.85, 0.04, 0.9),
            Cue::Unmute => s(&[n!(D5, 0.0, 1.0), n!(A5, 0.055, 0.8)], 0.13, 0.85, 0.04, 0.9),
            // The same, an octave lower and rounder: the whole call goes
            // quiet, or comes back.
            Cue::Deafen => s(
                &[n!(D5, 0.0, 1.0), n!(A4, 0.06, 1.2), n!(D4, 0.12, 1.15)],
                0.18,
                0.95,
                0.05,
                0.5,
            ),
            Cue::Undeafen => s(
                &[n!(D4, 0.0, 1.0), n!(A4, 0.06, 0.85), n!(D5, 0.12, 0.87)],
                0.18,
                0.95,
                0.05,
                0.5,
            ),
            Cue::StreamStart => s(
                &[n!(A4, 0.0, 1.0), n!(CS5, 0.07, 0.95), n!(E5, 0.14, 0.95), n!(A5, 0.21, 0.94)],
                0.28,
                0.9,
                0.04,
                0.6,
            ),
            Cue::StreamStop => s(
                &[n!(A5, 0.0, 1.0), n!(E5, 0.07, 1.05), n!(CS5, 0.14, 1.05), n!(A4, 0.21, 1.06)],
                0.28,
                0.9,
                0.04,
                0.6,
            ),
            Cue::Message => s(&[n!(FS5, 0.0, 1.0), n!(D6, 0.06, 0.97)], 0.34, 0.7, 0.03, 0.8),
            Cue::None => None,
        }
    }
}

/// Renders one cue into mono samples at 48 kHz.
///
/// Each note is a sine whose pitch eases in from `from`, with a short
/// bright strike on top (a second and a third partial that die out fast,
/// as a mallet's) and an exponential ring. A faint echo rounds the end.
fn render(shape: &Shape) -> Vec<f32> {
    const START: f32 = 0.01;
    const ATTACK: f32 = 0.006;
    const ECHO: [(f32, f32); 2] = [(0.065, 0.16), (0.13, 0.06)];
    const FADE: f32 = 0.012;
    let tau = std::f32::consts::TAU;
    let rate = RATE as f32;
    let last = shape.notes.iter().map(|n| n.at).fold(0.0, f32::max);
    // The ring is a time constant: by four of them a note is near silent,
    // and the fade takes the rest.
    let note_len = shape.ring * 4.0;
    let dry_len = ((START + last + note_len) * rate).ceil() as usize;
    let tail = (ECHO[1].0 * rate).ceil() as usize;
    let len = dry_len + tail;
    let mut dry = vec![0f32; len];
    for note in shape.notes {
        let first = ((START + note.at) * rate) as usize;
        let count = ((note_len * rate) as usize).min(len - first);
        let mut phase = 0f32;
        let mut phase2 = 0f32;
        let mut phase3 = 0f32;
        for (i, slot) in dry[first..first + count].iter_mut().enumerate() {
            let t = i as f32 / rate;
            // Smooth-step from the starting pitch to the note.
            let x = (t / shape.glide).min(1.0);
            let ease = x * x * (3.0 - 2.0 * x);
            let freq = note.freq * (note.from + (1.0 - note.from) * ease);
            phase = (phase + freq / rate).fract();
            phase2 = (phase2 + 2.0 * freq / rate).fract();
            phase3 = (phase3 + 3.01 * freq / rate).fract();
            let attack = (t / ATTACK).min(1.0);
            let env = attack * (-t / shape.ring).exp();
            // The fade at the very end keeps the cut silent.
            let left = (count - i) as f32 / rate;
            let fade = (left / FADE).min(1.0);
            let body = (tau * phase).sin();
            let strike = shape.strike
                * (0.45 * (tau * phase2).sin() * (-t * 28.0).exp()
                    + 0.2 * (tau * phase3).sin() * (-t * 60.0).exp());
            *slot += env * fade * (body + strike);
        }
    }
    let out_gain = 0.3 * shape.gain;
    let mut buf = dry.clone();
    for (delay, level) in ECHO {
        let d = (delay * rate) as usize;
        for i in d..len {
            buf[i] += level * dry[i - d];
        }
    }
    for s in &mut buf {
        *s *= out_gain;
    }
    buf
}

/// The cue's samples as a 16-bit mono WAV file, to listen to outside the
/// app.
#[cfg(test)]
fn wav(cue: Cue) -> Option<Vec<u8>> {
    let samples = render(&cue.shape()?);
    let data = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data.to_le_bytes());
    for s in samples {
        let v = (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        out.extend_from_slice(&v.to_le_bytes());
    }
    Some(out)
}

pub struct Sounds {
    sink: Mutex<Option<MixerDeviceSink>>,
    /// Told whenever an opening of the device ends, opened or not.
    opened: Condvar,
    /// Set when the device stopped working (unplugged, switched, its format
    /// changed); the next cue opens the current default one again.
    broken: Arc<AtomicBool>,
    /// The device is being opened. Turned off under `sink`'s lock, so
    /// whoever waits on `opened` sees the device and this change together.
    opening: AtomicBool,
    off: Mutex<HashSet<String>>,
    rendered: Mutex<HashMap<Cue, Arc<Vec<f32>>>>,
}

impl Sounds {
    pub fn new(off: HashSet<String>) -> Arc<Sounds> {
        let sounds = Arc::new(Sounds {
            sink: Mutex::new(None),
            opened: Condvar::new(),
            broken: Arc::new(AtomicBool::new(false)),
            opening: AtomicBool::new(false),
            off: Mutex::new(off),
            rendered: Mutex::new(HashMap::new()),
        });
        // Open from the start: kept open with nothing to play, it measured
        // under 0.1 % of a core, and opening on demand would delay the
        // first sound by the device's start-up.
        sounds.open();
        sounds
    }

    /// Opens the output (the one chosen, or the default) on a thread of its
    /// own: the audio device sets its thread up for COM in a way that must
    /// not clash with the window's.
    fn open(self: &Arc<Self>) {
        if self.opening.swap(true, Ordering::SeqCst) {
            return;
        }
        let s = self.clone();
        let spawned = std::thread::Builder::new()
            .name("sounds".into())
            .spawn(move || {
                // The output chosen in the settings, or else the default.
                let mut sink = Err("nenhuma saída de áudio".to_owned());
                for device in crate::devices::outputs(&cpal::default_host()) {
                    use cpal::traits::DeviceTrait;
                    let name = device
                        .description()
                        .map(|d| d.name().to_owned())
                        .unwrap_or_default();
                    let broken = s.broken.clone();
                    sink = DeviceSinkBuilder::from_device(device)
                        .and_then(|b| {
                            b.with_error_callback(move |_e| broken.store(true, Ordering::SeqCst))
                                .open_stream()
                        })
                        .map_err(|e| e.to_string());
                    if sink.is_ok() {
                        crate::logln!("sons: saída \"{name}\"");
                        break;
                    }
                }
                let mut slot = s.sink.lock().unwrap_or_else(PoisonError::into_inner);
                match sink {
                    Ok(mut sink) => {
                        sink.log_on_drop(false);
                        s.broken.store(false, Ordering::SeqCst);
                        *slot = Some(sink);
                    }
                    Err(e) => crate::logln!("sons: {e}"),
                }
                s.opening.store(false, Ordering::SeqCst);
                drop(slot);
                s.opened.notify_all();
            });
        if spawned.is_err() {
            self.opening.store(false, Ordering::SeqCst);
        }
    }

    /// Another output was chosen: opens it now, so the next sound goes
    /// through it.
    pub fn output_changed(self: &Arc<Self>) {
        self.sink
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        self.open();
    }

    pub fn enabled(&self, cue: Cue) -> bool {
        !self
            .off
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(cue.key())
    }

    pub fn set_enabled(&self, cue: Cue, on: bool) {
        let mut off = self.off.lock().unwrap_or_else(PoisonError::into_inner);
        if on {
            off.remove(cue.key());
        } else {
            off.insert(cue.key().to_owned());
        }
    }

    /// The cues turned off, for prefs.json.
    pub fn off_list(&self) -> Vec<String> {
        let mut list: Vec<String> = self
            .off
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned()
            .collect();
        list.sort();
        list
    }

    pub fn enabled_count(&self) -> usize {
        Cue::ALL.iter().filter(|c| self.enabled(**c)).count()
    }

    /// Plays any sound through the same output as the cues (an attachment
    /// being played). False when there is no output right now.
    pub fn add_source(self: &Arc<Self>, source: impl rodio::Source + Send + 'static) -> bool {
        if test_silent() {
            return true;
        }
        if self.broken.load(Ordering::SeqCst) {
            self.sink
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take();
            self.open();
        }
        // The device may still be opening (it is, right after start): until
        // that ends, one way or the other.
        let sink = self.sink.lock().unwrap_or_else(PoisonError::into_inner);
        let (sink, _) = self
            .opened
            .wait_timeout_while(sink, OPEN_WAIT, |sink| {
                sink.is_none() && self.opening.load(Ordering::SeqCst)
            })
            .unwrap_or_else(PoisonError::into_inner);
        match sink.as_ref() {
            Some(sink) => {
                sink.mixer().add(source);
                true
            }
            None => false,
        }
    }

    /// Plays a cue unless it is turned off.
    pub fn play(self: &Arc<Self>, cue: Cue) {
        if self.enabled(cue) {
            self.play_always(cue);
        }
    }

    /// Plays a cue even if it is turned off (the preview button).
    pub fn play_always(self: &Arc<Self>, cue: Cue) {
        let Some(shape) = cue.shape() else { return };
        if test_silent() {
            return;
        }
        if self.broken.load(Ordering::SeqCst) {
            // This cue is lost; the next one plays on the reopened device.
            self.sink
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take();
            self.open();
            return;
        }
        let samples = self
            .rendered
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(cue)
            .or_insert_with(|| Arc::new(render(&shape)))
            .clone();
        let sink = self.sink.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(sink) = sink.as_ref() else { return };
        let (Some(channels), Some(rate)) = (NonZero::new(1u16), NonZero::new(RATE)) else {
            return;
        };
        sink.mixer().add(SamplesBuffer::new(
            channels,
            rate,
            samples.as_slice().to_vec(),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_cue_renders_something_audible_and_bounded() {
        for cue in Cue::ALL {
            let shape = cue.shape().expect("todo aviso tem forma");
            let buf = render(&shape);
            let peak = buf.iter().fold(0f32, |m, s| m.max(s.abs()));
            assert!(peak > 0.1, "{cue:?} está mudo ({peak})");
            assert!(peak < 0.95, "{cue:?} estoura ({peak})");
            // Starts and ends in silence, so there is no click.
            assert!(buf[0].abs() < 1e-3, "{cue:?} começa com estalo");
            assert!(
                buf[buf.len() - 1].abs() < 1e-3,
                "{cue:?} termina com estalo"
            );
            // Short, as a cue should be.
            assert!(buf.len() < 2 * RATE as usize, "{cue:?} é longo demais");
        }
        assert!(Cue::None.shape().is_none());
    }

    #[test]
    fn slides_go_the_way_the_cue_means() {
        // Turning something on slides up into the notes; off slides down.
        for (on, off) in [
            (Cue::Join, Cue::Leave),
            (Cue::Unmute, Cue::Mute),
            (Cue::Undeafen, Cue::Deafen),
            (Cue::StreamStart, Cue::StreamStop),
        ] {
            let up = on.shape().unwrap();
            let down = off.shape().unwrap();
            assert!(up.notes.first().unwrap().freq < up.notes.last().unwrap().freq);
            assert!(down.notes.first().unwrap().freq > down.notes.last().unwrap().freq);
            assert!(up.notes.iter().all(|n| n.from <= 1.0), "{on:?}");
            assert!(down.notes.iter().all(|n| n.from >= 1.0), "{off:?}");
        }
    }

    #[test]
    fn a_wav_holds_the_samples() {
        let bytes = wav(Cue::Mute).unwrap();
        let samples = render(&Cue::Mute.shape().unwrap()).len();
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(bytes.len(), 44 + samples * 2);
        assert!(wav(Cue::None).is_none());
    }

    /// Writes every cue as a WAV into $SONS_DIR, to listen to them:
    /// `SONS_DIR=pasta cargo test -p babutela export_cues -- --ignored`.
    #[test]
    #[ignore]
    fn export_cues() {
        let dir = std::env::var("SONS_DIR").expect("SONS_DIR");
        std::fs::create_dir_all(&dir).unwrap();
        for (i, cue) in Cue::ALL.iter().enumerate() {
            let path = std::path::Path::new(&dir).join(format!("{:02}-{}.wav", i + 1, cue.key()));
            std::fs::write(path, wav(*cue).unwrap()).unwrap();
        }
    }

    #[test]
    fn turning_cues_off_is_remembered_by_key() {
        let s = Arc::new(Sounds {
            sink: Mutex::new(None),
            opened: Condvar::new(),
            broken: Arc::new(AtomicBool::new(false)),
            // As if already opening, so the test never touches a device.
            opening: AtomicBool::new(true),
            off: Mutex::new(HashSet::new()),
            rendered: Mutex::new(HashMap::new()),
        });
        assert_eq!(s.enabled_count(), 10);
        s.set_enabled(Cue::Join, false);
        s.set_enabled(Cue::Message, false);
        assert!(!s.enabled(Cue::Join));
        assert_eq!(s.off_list(), ["join", "message"]);
        s.set_enabled(Cue::Join, true);
        assert_eq!(s.off_list(), ["message"]);
        assert_eq!(s.enabled_count(), 9);
        // No device here: playing is a no-op, not a crash.
        s.play_always(Cue::Join);
    }
}
