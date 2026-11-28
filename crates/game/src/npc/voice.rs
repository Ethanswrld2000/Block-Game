//! Pure speech timing: a line of text becomes a row of syllables, each with a
//! start time, how wide the mouth opens and which vowel it is. The mouth,
//! the talking sound and the typed-out subtitle all follow the same row, so
//! they stay in step.

/// One spoken syllable.
#[derive(Clone, Debug, PartialEq)]
pub struct Syllable {
    pub start: f32,
    pub len: f32,
    /// How far the jaw drops, 0..1.
    pub open: f32,
    /// How wide the lips spread, about 0.5 (round "oo") .. 1.1 ("ee").
    pub wide: f32,
    /// Which voice sound to play (0 ah, 1 eh, 2 oh, 3 oo).
    pub vowel: usize,
    /// Characters of the line shown once this syllable starts.
    pub shown: usize,
}

/// A whole line, timed.
#[derive(Clone, Debug, Default)]
pub struct Speech {
    pub syllables: Vec<Syllable>,
    /// Seconds until the last syllable ends.
    pub length: f32,
    /// Characters in the line.
    pub chars: usize,
}

/// Seconds of silence after punctuation.
const COMMA: f32 = 0.22;
const STOP: f32 = 0.42;
const DASH: f32 = 0.3;

fn vowel_of(c: char) -> Option<(f32, f32, usize)> {
    // (open, wide, sound)
    match c.to_ascii_lowercase() {
        'a' => Some((1.0, 0.9, 0)),
        'e' => Some((0.6, 1.05, 1)),
        'i' | 'y' => Some((0.45, 1.1, 1)),
        'o' => Some((0.8, 0.6, 2)),
        'u' | 'w' => Some((0.5, 0.5, 3)),
        _ => None,
    }
}

/// A small hash so the same word always sounds the same.
fn hash(s: &str) -> u32 {
    s.bytes().fold(2166136261u32, |h, b| (h ^ b as u32).wrapping_mul(16777619))
}

/// Time a line at `rate` syllables per second.
pub fn speak(text: &str, rate: f32) -> Speech {
    let rate = rate.max(0.5);
    let chars: Vec<char> = text.chars().collect();
    let mut out = Speech { chars: chars.len(), ..Speech::default() };
    let mut t = 0.0f32;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_alphanumeric() || c == '\'' {
            // A word: up to the next space or punctuation.
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '\'') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let h = hash(&word.to_lowercase());
            // Syllables = groups of vowels (at least one per word).
            let mut groups: Vec<(usize, char)> = Vec::new();
            let mut in_vowel = false;
            for (k, ch) in word.chars().enumerate() {
                let v = vowel_of(ch).is_some();
                if v && !in_vowel {
                    groups.push((k, ch));
                }
                in_vowel = v;
            }
            // A silent final "e" ("stone", "made") isn't a syllable of its own.
            if groups.len() > 1 && word.to_lowercase().ends_with('e') && groups.last().is_some_and(|g| g.0 == word.chars().count() - 1) {
                groups.pop();
            }
            if groups.is_empty() {
                // "Hm", "Mm", numbers: one closed-ish syllable.
                groups.push((0, 'u'));
            }
            for (n, &(_, ch)) in groups.iter().enumerate() {
                let (open, wide, vowel) = vowel_of(ch).unwrap_or((0.35, 0.7, 3));
                // Vary length and openness a little by word, so it isn't a metronome.
                let wobble = ((h >> (n * 4)) & 0xF) as f32 / 15.0;
                let len = (0.8 + 0.45 * wobble) / rate;
                let shown = if n + 1 == groups.len() { i } else { start + groups[n + 1].0 };
                out.syllables.push(Syllable { start: t, len, open: open * (0.8 + 0.3 * wobble), wide, vowel, shown });
                t += len;
            }
            // A tiny gap between words.
            t += 0.03 / rate * 4.0;
        } else {
            let pause = match c {
                ',' | ';' | ':' => COMMA,
                '.' | '!' | '?' => STOP,
                '-' | '\u{2014}' => DASH,
                _ => 0.0,
            };
            // Punctuation shows with the syllable before it.
            if let Some(last) = out.syllables.last_mut() {
                last.shown = i + 1;
            }
            t += pause;
            i += 1;
        }
    }
    out.length = out.syllables.last().map_or(0.0, |s| s.start + s.len);
    // Whatever trails the last word (closing quotes, spaces) shows with it.
    if let Some(last) = out.syllables.last_mut() {
        last.shown = out.chars;
    }
    out
}

impl Speech {
    /// The mouth at time `t`: (open 0..1, wide).
    pub fn mouth(&self, t: f32) -> (f32, f32) {
        // Find the syllable around t; between syllables the mouth eases shut.
        for s in &self.syllables {
            if t >= s.start && t < s.start + s.len {
                let u = (t - s.start) / s.len;
                // Opens quickly, closes over the rest of the syllable.
                let env = if u < 0.3 { (u / 0.3).powf(0.8) } else { 1.0 - ((u - 0.3) / 0.7).powf(1.5) * 0.85 };
                return (s.open * env, s.wide);
            }
        }
        (0.0, 0.8)
    }

    /// Characters of the line on screen at time `t`.
    pub fn shown(&self, t: f32) -> usize {
        if t >= self.length {
            return self.chars;
        }
        self.syllables.iter().take_while(|s| s.start <= t).last().map_or(0, |s| s.shown)
    }

    /// Syllables that start in (from, to]. Begin a line with `from` below 0.
    pub fn starting(&self, from: f32, to: f32) -> impl Iterator<Item = &Syllable> {
        self.syllables.iter().filter(move |s| s.start > from && s.start <= to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_syllables() {
        assert_eq!(speak("Hello", 5.0).syllables.len(), 2);
        assert_eq!(speak("stone", 5.0).syllables.len(), 1, "silent e");
        assert_eq!(speak("Hm.", 5.0).syllables.len(), 1);
        assert_eq!(speak("the old swamp road", 5.0).syllables.len(), 4);
    }

    #[test]
    fn pauses_after_punctuation() {
        let a = speak("one two", 5.0);
        let b = speak("one. two", 5.0);
        let gap = |s: &Speech| s.syllables[1].start - (s.syllables[0].start + s.syllables[0].len);
        assert!(gap(&b) > gap(&a) + 0.3);
    }

    #[test]
    fn text_types_out_with_the_voice() {
        let line = "Well, well. A traveller!";
        let s = speak(line, 5.0);
        assert_eq!(s.shown(-1.0), 0);
        let mut last = 0;
        let mut t = 0.0;
        while t < s.length + 0.1 {
            let n = s.shown(t);
            assert!(n >= last, "never takes characters back");
            last = n;
            t += 0.01;
        }
        assert_eq!(last, line.chars().count());
        // Half way through the time, part of the text is up.
        let mid = s.shown(s.length * 0.5);
        assert!(mid > 3 && mid < line.chars().count());
    }

    #[test]
    fn mouth_opens_on_syllables_and_shuts_between() {
        let s = speak("Ah. Ah.", 5.0);
        let first = &s.syllables[0];
        let (open, _) = s.mouth(first.start + first.len * 0.3);
        assert!(open > 0.5);
        let between = first.start + first.len + 0.2;
        assert_eq!(s.mouth(between).0, 0.0);
        assert_eq!(s.mouth(s.length + 1.0).0, 0.0);
    }

    #[test]
    fn each_syllable_starts_once() {
        let s = speak("one two three four", 5.0);
        let mut heard = 0;
        let mut t = -0.01;
        while t < s.length {
            let next = t + 1.0 / 60.0;
            heard += s.starting(t, next).count();
            t = next;
        }
        assert_eq!(heard, s.syllables.len());
    }
}
