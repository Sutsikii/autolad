//! Subtitles of the edit, built from the words it plays: short readable cues that follow the
//! speech, as SubRip (`.srt`) or WebVTT (`.vtt`).

use crate::transcript::{is_filler, EditWord};

/// How much text a cue line holds; a cue has at most two lines. Broadcast guidelines say 42
/// characters; a narrow portrait frame fits less.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CueLayout {
    pub line_chars: usize,
}

impl CueLayout {
    pub const LANDSCAPE: Self = Self { line_chars: 42 };
    pub const PORTRAIT: Self = Self { line_chars: 24 };

    pub fn for_frame(width: u32, height: u32) -> Self {
        if height > width {
            Self::PORTRAIT
        } else {
            Self::LANDSCAPE
        }
    }

    fn max_chars(&self) -> usize {
        self.line_chars * 2
    }
}

/// One subtitle, in seconds on the edited timeline. `text` may hold line breaks.
#[derive(Debug, Clone, PartialEq)]
pub struct Cue {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

/// A cue never stays up longer than this, however long the sentence.
const MAX_CUE_SECONDS: f64 = 6.0;
/// A silence this long clears the screen.
const CLEAR_ON_PAUSE: f64 = 0.8;
/// Short cues stay a bit longer so they can be read, unless the next one comes first.
const MIN_CUE_SECONDS: f64 = 1.0;

/// Groups the words of the edit into cues: a new cue starts when the text would not fit the
/// layout, after a sentence, after a comma once the cue is half full, on a pause, or when a
/// cue has been up too long. Hesitations are left out.
pub fn cues(words: &[EditWord], layout: CueLayout) -> Vec<Cue> {
    let spoken: Vec<&EditWord> = words.iter().filter(|w| !is_filler(&w.text)).collect();
    let mut groups: Vec<Vec<&EditWord>> = Vec::new();
    let mut current: Vec<&EditWord> = Vec::new();
    for word in spoken {
        if let (Some(first), Some(last)) = (current.first(), current.last()) {
            let mut candidate = words_of(&current);
            candidate.push(word.text.trim().to_owned());
            let full = !fits(&candidate, layout);
            let pause = word.timeline.start - last.timeline.end > CLEAR_ON_PAUSE;
            let too_long = word.timeline.end - first.timeline.start > MAX_CUE_SECONDS;
            let sentence_end = last.text.ends_with(['.', '!', '?', '…']);
            let comma_break = last.text.ends_with([',', ';', ':'])
                && joined_len(&current) * 2 >= layout.max_chars();
            if full || pause || too_long || sentence_end || comma_break {
                groups.push(std::mem::take(&mut current));
            }
        }
        current.push(word);
    }
    if !current.is_empty() {
        groups.push(current);
    }

    let mut out: Vec<Cue> = groups
        .iter()
        .map(|group| Cue {
            start: group[0].timeline.start,
            end: group[group.len() - 1].timeline.end,
            text: wrap(&words_of(group), layout),
        })
        .collect();
    // Let short cues linger, but never over the next one.
    for i in 0..out.len() {
        let limit = out.get(i + 1).map_or(f64::INFINITY, |next| next.start);
        let wanted = out[i].start + MIN_CUE_SECONDS;
        if out[i].end < wanted {
            out[i].end = wanted.min(limit).max(out[i].end);
        }
    }
    out
}

fn words_of(group: &[&EditWord]) -> Vec<String> {
    group.iter().map(|w| w.text.trim().to_owned()).collect()
}

fn joined_len(group: &[&EditWord]) -> usize {
    text_len(&words_of(group))
}

fn text_len(words: &[String]) -> usize {
    words.iter().map(|w| w.chars().count()).sum::<usize>() + words.len().saturating_sub(1)
}

/// Where to break a cue into two lines, as balanced as possible so the eye travels less, and
/// by how many characters the lines then overflow `line_chars`.
fn best_split(words: &[String], layout: CueLayout) -> Option<(usize, usize)> {
    (1..words.len())
        .map(|split| {
            let (top, bottom) = (text_len(&words[..split]), text_len(&words[split..]));
            let overflow =
                top.saturating_sub(layout.line_chars) + bottom.saturating_sub(layout.line_chars);
            (overflow, top.abs_diff(bottom), split)
        })
        .min()
        .map(|(overflow, _, split)| (split, overflow))
}

/// Whether the words fit one line, or two lines broken between words.
fn fits(words: &[String], layout: CueLayout) -> bool {
    text_len(words) <= layout.line_chars
        || best_split(words, layout).is_some_and(|(_, overflow)| overflow == 0)
}

/// The cue on one line if it fits, else on two balanced lines. A single word too long for a
/// line is left as is.
fn wrap(words: &[String], layout: CueLayout) -> String {
    if text_len(words) <= layout.line_chars {
        return words.join(" ");
    }
    match best_split(words, layout) {
        Some((split, _)) => format!("{}\n{}", words[..split].join(" "), words[split..].join(" ")),
        None => words.join(" "),
    }
}

pub fn to_srt(cues: &[Cue]) -> String {
    let mut out = String::new();
    for (i, cue) in cues.iter().enumerate() {
        out.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            i + 1,
            stamp(cue.start, ','),
            stamp(cue.end, ','),
            cue.text
        ));
    }
    out
}

pub fn to_vtt(cues: &[Cue]) -> String {
    let mut out = String::from("WEBVTT\n\n");
    for cue in cues {
        out.push_str(&format!(
            "{} --> {}\n{}\n\n",
            stamp(cue.start, '.'),
            stamp(cue.end, '.'),
            cue.text
        ));
    }
    out
}

/// `HH:MM:SS,mmm` (SubRip) or `HH:MM:SS.mmm` (WebVTT).
fn stamp(seconds: f64, separator: char) -> String {
    let ms = (seconds.max(0.0) * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02}{separator}{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{AssetId, TimeRange};

    /// Words every 0.4 s (0.3 s long) on the timeline, or after `gap` seconds when a word is
    /// prefixed with `|`.
    fn words(text: &str) -> Vec<EditWord> {
        let mut at = 0.0;
        text.split(' ')
            .map(|raw| {
                let (word, pause) = match raw.strip_prefix('|') {
                    Some(w) => (w, 2.0),
                    None => (raw, 0.0),
                };
                at += pause;
                let range = TimeRange::new(at, at + 0.3).unwrap();
                at += 0.4;
                EditWord {
                    asset: AssetId("a".into()),
                    cut: 0,
                    source: range,
                    timeline: range,
                    text: word.to_owned(),
                }
            })
            .collect()
    }

    fn texts(cues: &[Cue]) -> Vec<&str> {
        cues.iter().map(|c| c.text.as_str()).collect()
    }

    #[test]
    fn a_cue_ends_with_its_sentence() {
        let c = cues(
            &words("Bonjour à tous. On commence ?"),
            CueLayout::LANDSCAPE,
        );
        assert_eq!(texts(&c), ["Bonjour à tous.", "On commence ?"]);
        assert_eq!(c[0].start, 0.0);
    }

    #[test]
    fn long_speech_is_split_to_fit_two_balanced_lines() {
        let speech = "aujourd'hui nous allons voir ensemble comment monter une vidéo \
                      rapidement sans jamais toucher à la moindre timeline complexe";
        let c = cues(&words(speech), CueLayout::LANDSCAPE);
        assert!(c.len() >= 2);
        for cue in &c {
            let lines: Vec<&str> = cue.text.lines().collect();
            assert!(lines.len() <= 2, "{:?}", cue.text);
            assert!(
                lines.iter().all(|l| l.chars().count() <= 42),
                "{:?}",
                cue.text
            );
        }
        let first: Vec<&str> = c[0].text.lines().collect();
        assert!(first[0].len().abs_diff(first[1].len()) <= 12, "{first:?}");
    }

    #[test]
    fn portrait_cues_are_narrower() {
        let c = cues(
            &words("un deux trois quatre cinq six sept huit neuf dix"),
            CueLayout::PORTRAIT,
        );
        assert!(c
            .iter()
            .all(|cue| cue.text.lines().all(|l| l.chars().count() <= 24)));
        assert_eq!(CueLayout::for_frame(1080, 1920), CueLayout::PORTRAIT);
        assert_eq!(CueLayout::for_frame(1920, 1080), CueLayout::LANDSCAPE);
    }

    #[test]
    fn a_pause_clears_the_screen_and_hesitations_are_not_shown() {
        let c = cues(&words("alors euh on |commence"), CueLayout::LANDSCAPE);
        assert_eq!(texts(&c), ["alors on", "commence"]);
    }

    #[test]
    fn short_cues_linger_without_overlapping_the_next() {
        let c = cues(&words("Oui. Non. |Peut-être."), CueLayout::LANDSCAPE);
        // "Oui." would last 0.3 s; it stays until "Non." starts.
        assert_eq!((c[0].start, c[0].end), (0.0, 0.4));
        // "Non." is followed by a pause: it gets its full second.
        assert!((c[1].end - 1.4).abs() < 1e-9, "{:?}", c[1]);
        assert!(c.windows(2).all(|p| p[0].end <= p[1].start));
    }

    #[test]
    fn srt_and_vtt_formats() {
        let cue = |start, end, text: &str| Cue {
            start,
            end,
            text: text.into(),
        };
        let list = [
            cue(0.0, 1.5, "Bonjour"),
            cue(3661.25, 3662.0, "deux\nlignes"),
        ];
        assert_eq!(
            to_srt(&list),
            "1\n00:00:00,000 --> 00:00:01,500\nBonjour\n\n\
             2\n01:01:01,250 --> 01:01:02,000\ndeux\nlignes\n\n"
        );
        assert!(to_vtt(&list).starts_with("WEBVTT\n\n00:00:00.000 --> 00:00:01.500\nBonjour\n"));
        assert_eq!(to_srt(&[]), "");
    }
}
