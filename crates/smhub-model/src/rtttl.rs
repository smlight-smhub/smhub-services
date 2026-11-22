//! RTTTL melodies: checked before they reach the buzzer, timed for the queue, and turned into
//! notes for the Linux player. The ESPHome firmware plays the same text itself.
//!
//! Form: `name:d=4,o=5,b=120:8c6,8p,4e.` (defaults, then notes). Octave 4 starts at middle C,
//! as in ESPHome's player.

use serde::Serialize;

/// Longest melody text.
pub const MAX_LEN: usize = 512;
/// Longest melody, in milliseconds.
pub const MAX_MS: u32 = 30_000;
/// Most notes in one melody.
pub const MAX_NOTES: usize = 256;

/// One note: a frequency in Hz (0 for a rest) and its length.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Note {
    pub hz: u32,
    pub ms: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Melody {
    pub name: String,
    pub notes: Vec<Note>,
}

impl Melody {
    pub fn duration_ms(&self) -> u32 {
        self.notes.iter().map(|n| n.ms).sum()
    }
}

/// The presets: id, RTTTL. The same melodies as SLZB-OS, so the products sound alike.
pub const PRESETS: [(&str, &str); 12] = [
    ("doorbell", "Doorbell:d=4,o=5,b=100:8e6,8c6"),
    ("alert", "Alert:d=8,o=6,b=200:c,p,c,p,c,p,c,2p,c,p,c,p,c,p,c"),
    ("success", "Success:d=16,o=5,b=180:c6,e6,g6,4c7"),
    ("error", "Error:d=16,o=5,b=120:4g4,8p,4g4,8p,4g4"),
    ("warning", "Warning:d=8,o=5,b=160:a,p,a,p,a,2p,a,p,a,p,a"),
    ("notification", "Notify:d=16,o=5,b=140:e6,8p,g6"),
    ("siren", "Siren:d=8,o=6,b=160:c,e,c,e,c,e,c,e,c,e,c,e"),
    ("chime", "Chime:d=8,o=5,b=120:e6,g6,4c7,8p,4c7"),
    ("beep", "Beep:d=4,o=5,b=125:4e"),
    ("double_beep", "DBeep:d=8,o=5,b=140:e,8p,e"),
    ("startup", "Startup:d=16,o=5,b=160:c,e,g,4c6"),
    ("shutdown", "Shutdown:d=16,o=6,b=160:c,g5,e5,4c5"),
];

pub fn preset(id: &str) -> Option<&'static str> {
    PRESETS.iter().find(|(i, _)| *i == id).map(|(_, r)| *r)
}

/// Semitones above C for the note letters.
fn semitone(c: char) -> Option<i32> {
    Some(match c {
        'c' => 0,
        'd' => 2,
        'e' => 4,
        'f' => 5,
        'g' => 7,
        'a' => 9,
        'b' | 'h' => 11,
        _ => return None,
    })
}

/// Frequency of a note: semitones above C4, equal temperament with A4 = 440 Hz.
fn hz(semitones_above_c4: i32) -> u32 {
    let from_a4 = f64::from(semitones_above_c4 - 9);
    let f = 440.0 * 2f64.powf(from_a4 / 12.0);
    if f.is_finite() && f > 0.0 && f < 100_000.0 { f.round() as u32 } else { 0 }
}

/// Read a melody. The message says what is wrong, for the page.
pub fn parse(text: &str) -> Result<Melody, String> {
    let text = text.trim();
    if text.is_empty() || text.len() > MAX_LEN || !text.chars().all(|c| c.is_ascii() && !c.is_ascii_control()) {
        return Err(format!("an RTTTL melody: name:d=4,o=5,b=100:notes, at most {MAX_LEN} plain characters"));
    }
    let mut parts = text.splitn(3, ':');
    let (Some(name), Some(defaults), Some(notes)) = (parts.next(), parts.next(), parts.next()) else {
        return Err("an RTTTL melody needs three parts: name:defaults:notes".into());
    };
    let (mut d, mut o, mut b) = (4u32, 5i32, 63u32);
    for kv in defaults.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let (k, v) = kv.split_once('=').ok_or_else(|| format!("default {kv:?} is not key=value"))?;
        let v = v.trim();
        match k.trim().to_ascii_lowercase().as_str() {
            "d" => d = v.parse().ok().filter(|x| [1, 2, 4, 8, 16, 32].contains(x)).ok_or("d= takes 1, 2, 4, 8, 16 or 32")?,
            "o" => o = v.parse().ok().filter(|x| (3..=8).contains(x)).ok_or("o= takes an octave from 3 to 8")?,
            "b" => b = v.parse().ok().filter(|x| (5..=900).contains(x)).ok_or("b= takes a tempo from 5 to 900")?,
            other => return Err(format!("unknown default {other:?}")),
        }
    }
    let whole_ms = 240_000 / b;
    let mut out = Vec::new();
    let mut total: u32 = 0;
    for raw in notes.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if out.len() >= MAX_NOTES {
            return Err(format!("at most {MAX_NOTES} notes"));
        }
        let s = raw.to_ascii_lowercase();
        let mut chars = s.chars().peekable();
        let mut digits = String::new();
        while let Some(c) = chars.peek().filter(|c| c.is_ascii_digit()) {
            digits.push(*c);
            chars.next();
        }
        let dur: u32 = if digits.is_empty() { d } else { digits.parse().ok().filter(|x| [1, 2, 4, 8, 16, 32, 64].contains(x)).ok_or_else(|| format!("note {raw:?}: length must be 1 to 64"))? };
        let letter = chars.next().ok_or_else(|| format!("note {raw:?} has no letter"))?;
        let mut sharp = false;
        let mut dotted = false;
        let mut octave = o;
        let mut rest: Vec<char> = chars.collect();
        if rest.first() == Some(&'#') {
            sharp = true;
            rest.remove(0);
        }
        // the dot may come before or after the octave
        let mut oct_digits = String::new();
        for c in rest {
            match c {
                '.' => dotted = true,
                '0'..='9' => oct_digits.push(c),
                '_' => sharp = true,
                _ => return Err(format!("note {raw:?} is not valid")),
            }
        }
        if !oct_digits.is_empty() {
            octave = oct_digits.parse().ok().filter(|x| (3..=8).contains(x)).ok_or_else(|| format!("note {raw:?}: octave 3 to 8"))?;
        }
        let mut ms = whole_ms / dur;
        if dotted {
            ms += ms / 2;
        }
        let freq = if letter == 'p' {
            0
        } else {
            let st = semitone(letter).ok_or_else(|| format!("note {raw:?}: use a to g, or p for a rest"))?;
            hz((octave - 4) * 12 + st + i32::from(sharp))
        };
        total = total.saturating_add(ms);
        if total > MAX_MS {
            return Err(format!("the melody is longer than {} s", MAX_MS / 1000));
        }
        out.push(Note { hz: freq, ms });
    }
    if out.is_empty() {
        return Err("the melody has no notes".into());
    }
    Ok(Melody { name: name.trim().to_owned(), notes: out })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_parse() {
        for (id, r) in PRESETS {
            let m = parse(r);
            assert!(m.is_ok(), "{id}: {m:?}");
        }
    }

    #[test]
    fn notes_and_timing() {
        let m = parse("t:d=4,o=5,b=120:a,8a#6,2p,4c.").unwrap_or_else(|_| Melody { name: String::new(), notes: vec![] });
        assert_eq!(m.notes.len(), 4);
        assert_eq!(m.notes[0], Note { hz: 880, ms: 500 });
        assert_eq!(m.notes[1].ms, 250);
        assert_eq!(m.notes[1].hz, 1865);
        assert_eq!(m.notes[2], Note { hz: 0, ms: 1000 });
        assert_eq!(m.notes[3].ms, 750);
        assert_eq!(m.duration_ms(), 2500);
        assert_eq!(hz(0), 262);
    }

    #[test]
    fn refused() {
        assert!(parse("no colons").is_err());
        assert!(parse("x:d=3:c").is_err());
        assert!(parse("x::q").is_err());
        assert!(parse("x::").is_err());
        assert!(parse(&format!("x:d=1,b=5:{}", ["c"; 40].join(","))).is_err());
        assert!(parse("x:o=9:c").is_err());
    }
}
