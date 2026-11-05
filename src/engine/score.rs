//! Scores: the music a module plays besides its knobs, a song's arrangement
//! or a `notes` module's patterns. Edited on the UI thread and moved to the
//! audio thread whole, like sounds.
//!
//! Patch files write them as text, read here: notes by name (`A2`, `C#4`),
//! `-` to hold a note and `.` for a rest; a song's cells as `A` to `D` for
//! the pattern a track plays, `.` for silence.

/// A song's tracks: its outputs, one per instrument.
pub const TRACKS: usize = 8;
pub const MAX_SECTIONS: usize = 16;
/// The most bars a section lasts.
pub const MAX_BARS: u32 = 999;
/// Patterns per `notes` module, A to D.
pub const PATTERNS: usize = 4;
pub const MAX_STEPS: usize = 64;

#[derive(Clone, Debug, PartialEq)]
pub enum Score {
    Arrangement(Arrangement),
    Patterns(Patterns),
}

/// The sections of a song, played one after the other.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arrangement {
    pub sections: Vec<Section>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub name: String,
    pub bars: u32,
    /// The pattern each track plays in this section, 0 for A, if it plays.
    pub tracks: [Option<u8>; TRACKS],
}

impl Default for Arrangement {
    fn default() -> Self {
        let section = |name: &str| Section {
            name: name.to_string(),
            bars: 4,
            tracks: [None; TRACKS],
        };
        Self {
            sections: ["intro", "verse", "chorus", "outro"].map(section).to_vec(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    Rest,
    /// A MIDI note number: 60 is middle C, `C4`.
    Note(u8),
    /// The note before goes on.
    Hold,
}

/// A `notes` module's patterns, and the key its editor offers notes in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Patterns {
    pub key: Key,
    /// A to D; an empty one plays nothing.
    pub patterns: [Vec<Step>; PATTERNS],
}

impl Default for Patterns {
    fn default() -> Self {
        Self {
            key: Key::default(),
            patterns: [vec![Step::Rest; 16], Vec::new(), Vec::new(), Vec::new()],
        }
    }
}

/// A root and a scale. Only the editor uses it, to show the notes that fit;
/// any note plays. C major by default.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Key {
    /// Pitch class, 0 for C to 11 for B.
    pub root: u8,
    /// Index in [`SCALES`].
    pub scale: usize,
}

/// Scales by name, as semitones above the root.
pub const SCALES: [(&str, &[u8]); 23] = [
    ("major", &[0, 2, 4, 5, 7, 9, 11]),
    ("minor", &[0, 2, 3, 5, 7, 8, 10]),
    ("harmonic minor", &[0, 2, 3, 5, 7, 8, 11]),
    ("melodic minor", &[0, 2, 3, 5, 7, 9, 11]),
    ("major pentatonic", &[0, 2, 4, 7, 9]),
    ("minor pentatonic", &[0, 3, 5, 7, 10]),
    ("blues", &[0, 3, 5, 6, 7, 10]),
    ("dorian", &[0, 2, 3, 5, 7, 9, 10]),
    ("phrygian", &[0, 1, 3, 5, 7, 8, 10]),
    ("lydian", &[0, 2, 4, 6, 7, 9, 11]),
    ("mixolydian", &[0, 2, 4, 5, 7, 9, 10]),
    ("locrian", &[0, 1, 3, 5, 6, 8, 10]),
    ("harmonic major", &[0, 2, 4, 5, 7, 8, 11]),
    ("phrygian dominant", &[0, 1, 4, 5, 7, 8, 10]),
    ("hungarian minor", &[0, 2, 3, 6, 7, 8, 11]),
    ("double harmonic", &[0, 1, 4, 5, 7, 8, 11]),
    ("diminished", &[0, 2, 3, 5, 6, 8, 9, 11]),
    ("half-whole diminished", &[0, 1, 3, 4, 6, 7, 9, 10]),
    ("augmented", &[0, 3, 4, 7, 8, 11]),
    ("whole tone", &[0, 2, 4, 6, 8, 10]),
    ("hirajoshi", &[0, 2, 3, 7, 8]),
    ("in", &[0, 1, 5, 7, 8]),
    ("chromatic", &[0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11]),
];

const NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];

impl Key {
    /// Whether a note is in this key.
    pub fn contains(&self, note: u8) -> bool {
        let degree = (note + 12 - self.root) % 12;
        SCALES[self.scale].1.contains(&degree)
    }

    /// `A minor`, `C# blues`.
    pub fn parse(text: &str) -> Result<Key, String> {
        let unknown = || format!("unknown key {text:?}: a note and a scale, as \"A minor\"");
        let (root, scale) = text.trim().split_once(' ').ok_or_else(unknown)?;
        let root = pitch_class(root).ok_or_else(unknown)?;
        let scale = SCALES
            .iter()
            .position(|(name, _)| *name == scale.trim())
            .ok_or_else(unknown)?;
        Ok(Key { root, scale })
    }

    pub fn name(&self) -> String {
        format!("{} {}", NAMES[usize::from(self.root)], SCALES[self.scale].0)
    }
}

/// `C`, `F#`, `Bb`, as 0..12.
fn pitch_class(text: &str) -> Option<u8> {
    let mut chars = text.chars();
    let natural = match chars.next()? {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let shift = match chars.as_str() {
        "" => 0,
        "#" => 1,
        "b" => 11,
        _ => return None,
    };
    Some((natural + shift) % 12)
}

/// `A2`, `C#4`, `Bb-1`, as a MIDI note number.
pub fn parse_note(text: &str) -> Option<u8> {
    let split = text.find(|c: char| c == '-' || c.is_ascii_digit())?;
    let (name, octave) = text.split_at(split);
    let octave: i32 = octave.parse().ok()?;
    let note = (octave + 1) * 12 + i32::from(pitch_class(name)?);
    u8::try_from(note).ok().filter(|&n| n < 128)
}

/// A MIDI note number by name, with sharps: 69 is `A4`.
pub fn note_name(note: u8) -> String {
    let octave = i32::from(note / 12) - 1;
    format!("{}{octave}", NAMES[usize::from(note % 12)])
}

/// A pattern from its text: `A2 - . C3`.
pub fn parse_steps(text: &str) -> Result<Vec<Step>, String> {
    let steps = text
        .split_whitespace()
        .map(|token| match token {
            "." => Ok(Step::Rest),
            "-" => Ok(Step::Hold),
            note => parse_note(note)
                .map(Step::Note)
                .ok_or_else(|| format!("unknown note {note:?}: a name and an octave, as \"A2\"")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if steps.len() > MAX_STEPS {
        return Err(format!("{} steps, the most is {MAX_STEPS}", steps.len()));
    }
    Ok(steps)
}

pub fn steps_text(steps: &[Step]) -> String {
    let token = |step: &Step| match step {
        Step::Rest => ".".to_string(),
        Step::Hold => "-".to_string(),
        Step::Note(note) => note_name(*note),
    };
    steps.iter().map(token).collect::<Vec<_>>().join(" ")
}

/// A pattern by letter, `A` to `D`.
pub fn pattern_letter(pattern: u8) -> char {
    char::from(b'A' + pattern)
}

/// One track's cells from its text, one per section: `A`, `B`... or `.`.
/// Missing cells are silent.
pub fn parse_cells(text: &str, sections: usize) -> Result<Vec<Option<u8>>, String> {
    let mut cells = text
        .split_whitespace()
        .map(|token| match token {
            "." => Ok(None),
            "A" | "B" | "C" | "D" => Ok(Some(token.as_bytes()[0] - b'A')),
            _ => Err(format!("unknown cell {token:?}: A to D, or . for silence")),
        })
        .collect::<Result<Vec<_>, _>>()?;
    if cells.len() > sections {
        return Err(format!("{} cells for {sections} sections", cells.len()));
    }
    cells.resize(sections, None);
    Ok(cells)
}

pub fn cells_text(cells: impl Iterator<Item = Option<u8>>) -> String {
    cells
        .map(|cell| cell.map_or('.', pattern_letter).to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notes_are_read_and_written_by_name() {
        assert_eq!(parse_note("C4"), Some(60));
        assert_eq!(parse_note("A4"), Some(69));
        assert_eq!(parse_note("Bb3"), Some(58));
        assert_eq!(parse_note("C#-1"), Some(1));
        assert_eq!(parse_note("G9"), Some(127));
        for bad in ["H2", "A", "4", "A#b2", "G#9", "C-2"] {
            assert_eq!(parse_note(bad), None, "{bad}");
        }
        assert_eq!(note_name(58), "A#3");
        for note in 0..128 {
            assert_eq!(parse_note(&note_name(note)), Some(note));
        }
    }

    #[test]
    fn patterns_round_trip_through_text() {
        let steps = parse_steps("A2 - . C3  E3").unwrap();
        assert_eq!(
            steps,
            [
                Step::Note(45),
                Step::Hold,
                Step::Rest,
                Step::Note(48),
                Step::Note(52)
            ]
        );
        assert_eq!(steps_text(&steps), "A2 - . C3 E3");
        assert_eq!(parse_steps(""), Ok(Vec::new()));
        assert!(parse_steps("A2 X").unwrap_err().contains("\"X\""));
        assert!(parse_steps(&". ".repeat(MAX_STEPS + 1)).is_err());
    }

    #[test]
    fn keys_are_a_root_and_a_scale() {
        let key = Key::parse("A minor").unwrap();
        assert_eq!(key.name(), "A minor");
        assert!(key.contains(45) && key.contains(48) && !key.contains(46));
        assert_eq!(
            Key::parse("Bb harmonic minor").unwrap().name(),
            "A# harmonic minor"
        );
        assert!(Key::parse("H minor").is_err());
        assert!(Key::parse("A moody").is_err());
        let names: Vec<_> = SCALES.iter().map(|(name, _)| name).collect();
        for (n, name) in names.iter().enumerate() {
            assert!(!names[n + 1..].contains(name), "{name} twice");
        }
    }

    #[test]
    fn cells_say_which_pattern_plays_in_each_section() {
        assert_eq!(
            parse_cells("A . D", 4),
            Ok(vec![Some(0), None, Some(3), None])
        );
        assert!(parse_cells("A A A", 2).is_err());
        assert!(parse_cells("E", 2).is_err());
        assert_eq!(cells_text([Some(1), None].into_iter()), "B .");
    }
}
