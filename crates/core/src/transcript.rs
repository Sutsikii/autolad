//! Text-based editing: what is said in the edit, and which source ranges to remove to cut a
//! piece of text, the hesitations or the failed takes. Works best on word-level transcripts;
//! a phrase-level one gives the same answers at phrase granularity.

use crate::domain::{AssetId, Edl, TimeRange};
use crate::edl_edit::EdlOp;
use crate::ports::TranscriptSegment;

/// Hesitation sounds, in the form [`collapse_repeats`] gives them ("euuuh" → "euh"). Only
/// sounds that are never words: "ben", "eh" or "like" can carry meaning.
const FILLERS: &[&str] = &[
    "euh", "heu", "hum", "hm", "mh", "bah", "beh", "uh", "um", "uhm", "erm", "er", "ehm",
];

/// A pause this long between two words ends a sentence even without punctuation.
const SENTENCE_PAUSE: f64 = 1.0;

/// Up to one word in this many of a searched text may be missing from the transcript:
/// whisper's word mode occasionally drops one.
const MISSING_WORD_RATIO: usize = 5;

/// A sentence this short between a failed take and its retry is part of the failure
/// ("Euh, non.").
const MAX_INTERJECTION_TOKENS: usize = 3;

/// A transcribed word (or phrase) as it plays in the edit.
#[derive(Debug, Clone, PartialEq)]
pub struct EditWord {
    pub asset: AssetId,
    /// Index of the EDL cut that plays it.
    pub cut: usize,
    pub source: TimeRange,
    pub timeline: TimeRange,
    pub text: String,
}

/// Inclusive range of word indices.
pub type Span = (usize, usize);

/// A failed take and the retry that replaces it, as word spans of the edit.
#[derive(Debug, Clone, PartialEq)]
pub struct Retake {
    pub dropped: Span,
    pub kept: Span,
}

/// The words the edit plays, in order. A word belongs to the cut that holds its middle, and
/// is clipped to it. `words_of` gives each asset's transcript (`None` if it has none).
pub fn edit_words<'a>(
    edl: &Edl,
    words_of: impl Fn(&AssetId) -> Option<&'a [TranscriptSegment]>,
) -> Vec<EditWord> {
    let mut out = Vec::new();
    let mut offset = 0.0;
    for (index, cut) in edl.cuts.iter().enumerate() {
        let to_timeline = |t: f64| offset + t - cut.range.start;
        for word in words_of(&cut.asset).unwrap_or_default() {
            let middle = (word.range.start + word.range.end) / 2.0;
            if middle < cut.range.start || middle >= cut.range.end {
                continue;
            }
            let start = word.range.start.max(cut.range.start);
            let end = word.range.end.min(cut.range.end);
            let (Ok(source), Ok(timeline)) = (
                TimeRange::new(start, end),
                TimeRange::new(to_timeline(start), to_timeline(end)),
            ) else {
                continue;
            };
            out.push(EditWord {
                asset: cut.asset.clone(),
                cut: index,
                source,
                timeline,
                text: word.text.trim().to_owned(),
            });
        }
        offset += cut.range.duration();
    }
    out
}

/// Lowercase words without punctuation: what two spellings of the same speech share.
/// Apostrophes and hyphens split ("l'homme" → "l", "homme"), as whisper splits them.
pub fn tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// "euuuh" → "euh", "hmmm" → "hm": hesitations are spelled with any number of repeats.
fn collapse_repeats(token: &str) -> String {
    let mut out = String::with_capacity(token.len());
    for c in token.chars() {
        if !out.ends_with(c) {
            out.push(c);
        }
    }
    out
}

pub fn is_filler(text: &str) -> bool {
    let tokens = tokens(text);
    !tokens.is_empty()
        && tokens
            .iter()
            .all(|t| FILLERS.contains(&collapse_repeats(t).as_str()))
}

/// Indices of the words that are only a hesitation sound.
pub fn filler_words(words: &[EditWord]) -> Vec<usize> {
    (0..words.len())
        .filter(|&i| is_filler(&words[i].text))
        .collect()
}

/// Every place `text` is said, in order and without overlap, as word spans. Case and
/// punctuation are ignored, and a few words of `text` may be missing from the transcript
/// (see [`MISSING_WORD_RATIO`]), but never its first or last one.
pub fn find_text(words: &[EditWord], text: &str) -> Vec<Span> {
    let pattern = tokens(text);
    let Some(first) = pattern.first() else {
        return Vec::new();
    };
    let stream: Vec<(String, usize)> = words
        .iter()
        .enumerate()
        .flat_map(|(i, w)| tokens(&w.text).into_iter().map(move |t| (t, i)))
        .collect();
    let allowed_missing = pattern.len() / MISSING_WORD_RATIO;

    let mut found = Vec::new();
    let mut s = 0;
    while s < stream.len() {
        if stream[s].0 != *first {
            s += 1;
            continue;
        }
        match match_from(&stream, s, &pattern, allowed_missing) {
            Some(end) => {
                found.push((stream[s].1, stream[end - 1].1));
                s = end;
            }
            None => s += 1,
        }
    }
    found
}

/// Matches `pattern` against `stream` starting at `start`; returns the stream index just past
/// the match.
fn match_from(
    stream: &[(String, usize)],
    start: usize,
    pattern: &[String],
    allowed_missing: usize,
) -> Option<usize> {
    let (mut k, mut missing) = (start + 1, 0);
    for (j, token) in pattern.iter().enumerate().skip(1) {
        if stream.get(k).is_some_and(|(t, _)| t == token) {
            k += 1;
        } else if j + 1 < pattern.len() && missing < allowed_missing {
            missing += 1;
        } else {
            return None;
        }
    }
    Some(k)
}

/// Splits the words into sentences: one ends after `.`, `!`, `?` or `…`, or before a long
/// pause.
pub fn sentence_spans(words: &[EditWord]) -> Vec<Span> {
    spans_by(words.len(), |i| (words[i].timeline, words[i].text.as_str()))
}

/// Whisper words of a source grouped into sentences, for reading: a word-level transcript
/// shown as is would be one word per line.
pub fn sentences(words: &[TranscriptSegment]) -> Vec<TranscriptSegment> {
    spans_by(words.len(), |i| (words[i].range, words[i].text.as_str()))
        .into_iter()
        .map(|(first, last)| TranscriptSegment {
            range: TimeRange {
                start: words[first].range.start,
                end: words[last].range.end,
            },
            text: words[first..=last]
                .iter()
                .map(|w| w.text.trim())
                .collect::<Vec<_>>()
                .join(" "),
        })
        .collect()
}

fn spans_by<'a>(len: usize, word: impl Fn(usize) -> (TimeRange, &'a str)) -> Vec<Span> {
    let mut spans = Vec::new();
    let mut start = 0;
    for i in 0..len {
        let (range, text) = word(i);
        let ends_sentence = text.trim_end().ends_with(['.', '!', '?', '…']);
        let pause_follows = (i + 1 == len) || word(i + 1).0.start - range.end > SENTENCE_PAUSE;
        if ends_sentence || pause_follows {
            spans.push((start, i));
            start = i + 1;
        }
    }
    spans
}

/// Text of a span of words, as it would be read.
pub fn span_text(words: &[EditWord], (first, last): Span) -> String {
    words[first..=last]
        .iter()
        .map(|w| w.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Sentences started over: a sentence followed by one that repeats or restarts it is a failed
/// take, and only the last attempt is kept. A short interjection between the two ("Non,
/// pardon.") goes with the failed take.
pub fn find_retakes(words: &[EditWord]) -> Vec<Retake> {
    let spans = sentence_spans(words);
    let said: Vec<Vec<String>> = spans
        .iter()
        .map(|&(a, b)| {
            words[a..=b]
                .iter()
                .filter(|w| !is_filler(&w.text))
                .flat_map(|w| tokens(&w.text))
                .collect()
        })
        .collect();

    let mut retakes = Vec::new();
    let mut i = 0;
    while i < spans.len() {
        let next = i + 1;
        let retry = if said.get(next).is_some_and(|b| restarts(&said[i], b)) {
            Some(next)
        } else if said
            .get(next)
            .is_some_and(|b| b.len() <= MAX_INTERJECTION_TOKENS)
            && said.get(next + 1).is_some_and(|b| restarts(&said[i], b))
        {
            Some(next + 1)
        } else {
            None
        };
        match retry {
            Some(j) => {
                retakes.push(Retake {
                    dropped: (spans[i].0, spans[j - 1].1),
                    kept: spans[j],
                });
                i = j;
            }
            None => i += 1,
        }
    }
    retakes
}

/// Whether `retry` says `failed` again: it starts the same way, or repeats almost all of it.
fn restarts(failed: &[String], retry: &[String]) -> bool {
    if failed.len() < 2 || retry.is_empty() {
        return false;
    }
    let common_prefix = failed.iter().zip(retry).take_while(|(a, b)| a == b).count();
    if common_prefix == failed.len() {
        return true;
    }
    if common_prefix >= 3 && common_prefix * 2 >= failed.len() {
        return true;
    }
    failed.len() >= 4
        && retry.len() <= failed.len() * 2
        && longest_common_subsequence(failed, retry) * 5 >= failed.len() * 4
}

fn longest_common_subsequence(a: &[String], b: &[String]) -> usize {
    let mut row = vec![0usize; b.len() + 1];
    for x in a {
        let mut diagonal = 0;
        for (j, y) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = if x == y {
                diagonal + 1
            } else {
                above.max(row[j])
            };
            diagonal = above;
        }
    }
    row[b.len()]
}

/// The EDL edits that remove the given spans of words: one source range per run of words
/// played by the same cut, from the first word's start to the last one's end.
pub fn removal_ops(words: &[EditWord], spans: &[Span]) -> Vec<EdlOp> {
    let mut ops = Vec::new();
    for &(first, last) in spans {
        let mut run_start = first;
        for i in first..=last {
            let run_ends = i == last || words[i + 1].cut != words[i].cut;
            if run_ends {
                ops.push(EdlOp::RemoveRange {
                    asset: words[run_start].asset.clone(),
                    start: words[run_start].source.start,
                    end: words[i].source.end,
                });
                run_start = i + 1;
            }
        }
    }
    ops
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{Asset, Cut};
    use crate::edl_edit::apply_ops;

    fn id() -> AssetId {
        AssetId("a".into())
    }

    fn range(start: f64, end: f64) -> TimeRange {
        TimeRange::new(start, end).unwrap()
    }

    /// One word every 0.5 s (0.4 s long) in the source, from `start`.
    fn source_words(text: &str, start: f64) -> Vec<TranscriptSegment> {
        text.split(' ')
            .enumerate()
            .map(|(i, w)| {
                let at = start + i as f64 * 0.5;
                TranscriptSegment {
                    range: range(at, at + 0.4),
                    text: format!(" {w}"),
                }
            })
            .collect()
    }

    fn whole(duration: f64) -> Edl {
        Edl {
            cuts: vec![Cut {
                asset: id(),
                range: range(0.0, duration),
            }],
        }
    }

    fn words(text: &str) -> Vec<EditWord> {
        let source = source_words(text, 0.0);
        edit_words(&whole(100.0), |_| Some(&source))
    }

    #[test]
    fn edit_words_follow_the_cuts_onto_the_timeline() {
        let source = source_words("un deux trois quatre", 0.0);
        // Keeps "un" and "trois", in that order; "deux" and "quatre" are cut out.
        let edl = Edl {
            cuts: vec![
                Cut {
                    asset: id(),
                    range: range(1.0, 1.45),
                },
                Cut {
                    asset: id(),
                    range: range(0.0, 0.45),
                },
            ],
        };
        let kept = edit_words(&edl, |_| Some(&source));
        let texts: Vec<_> = kept.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(texts, ["trois", "un"]);
        let close =
            |r: TimeRange, (s, e): (f64, f64)| (r.start - s).abs() + (r.end - e).abs() < 1e-9;
        assert!(
            close(kept[0].timeline, (0.0, 0.4)),
            "{:?}",
            kept[0].timeline
        );
        assert!(
            close(kept[1].timeline, (0.45, 0.85)),
            "{:?}",
            kept[1].timeline
        );
        assert_eq!(kept[1].source, range(0.0, 0.4));
        assert_eq!((kept[0].cut, kept[1].cut), (0, 1));
    }

    #[test]
    fn a_word_belongs_to_the_cut_holding_its_middle() {
        let source = source_words("mot", 0.0);
        let clipped = Edl {
            cuts: vec![Cut {
                asset: id(),
                range: range(0.1, 5.0),
            }],
        };
        let kept = edit_words(&clipped, |_| Some(&source));
        assert_eq!(kept[0].source, range(0.1, 0.4));
        let mostly_cut = Edl {
            cuts: vec![Cut {
                asset: id(),
                range: range(0.3, 5.0),
            }],
        };
        assert!(edit_words(&mostly_cut, |_| Some(&source)).is_empty());
        assert!(edit_words(&whole(5.0), |_| None).is_empty());
    }

    #[test]
    fn tokens_ignore_case_and_punctuation() {
        assert_eq!(tokens(" Bonjour, l'homme!"), ["bonjour", "l", "homme"]);
        assert_eq!(tokens("Peut-être…"), ["peut", "être"]);
        assert!(tokens(" ... ").is_empty());
    }

    #[test]
    fn hesitations_are_fillers_but_words_are_not() {
        for filler in ["euh", " Euuuh,", "hmmm...", "Um", "bah", "heu"] {
            assert!(is_filler(filler), "{filler}");
        }
        for word in ["eu", "ben", "hein", "humain", "bahut", "", "..."] {
            assert!(!is_filler(word), "{word}");
        }
        let w = words("alors euh on hum commence");
        assert_eq!(filler_words(&w), [1, 3]);
    }

    #[test]
    fn text_is_found_whatever_its_case_and_punctuation() {
        let w = words("Bonjour à tous. Aujourd'hui on parle de montage, on parle de montage.");
        assert_eq!(find_text(&w, "on parle de montage"), [(4, 7), (8, 11)]);
        assert_eq!(find_text(&w, "BONJOUR À TOUS"), [(0, 2)]);
        assert_eq!(find_text(&w, "aujourd'hui"), [(3, 3)]);
        assert!(find_text(&w, "vidéo").is_empty());
        assert!(find_text(&w, "  ").is_empty());
    }

    #[test]
    fn a_word_dropped_by_the_transcriber_does_not_prevent_a_match() {
        // "vraiment" is missing from the transcript.
        let w = words("il faut apprendre à monter ses vidéos soi-même");
        let text = "il faut vraiment apprendre à monter ses vidéos";
        assert_eq!(find_text(&w, text), [(0, 6)]);
        // Short texts must match exactly, and the last word must be there.
        assert!(find_text(&w, "il vraiment faut").is_empty());
        assert!(find_text(&w, "il faut apprendre à monter ses films").is_empty());
    }

    #[test]
    fn sentences_end_at_punctuation_or_long_pauses() {
        let mut w = words("Salut. Ça va ? Oui");
        assert_eq!(sentence_spans(&w), [(0, 0), (1, 3), (4, 4)]);
        assert_eq!(span_text(&w, (1, 3)), "Ça va ?");
        // A 2 s silence before "Oui" ends the sentence even without punctuation.
        let source = [source_words("on y va", 0.0), source_words("oui", 3.0)].concat();
        w = edit_words(&whole(10.0), |_| Some(&source));
        assert_eq!(sentence_spans(&w), [(0, 2), (3, 3)]);
    }

    #[test]
    fn source_words_read_as_sentences() {
        let grouped = sentences(&source_words("Salut. Ça va ?", 2.0));
        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped[1].text, "Ça va ?");
        assert_eq!(grouped[1].range, range(2.5, 3.9));
        assert!(sentences(&[]).is_empty());
    }

    #[test]
    fn a_restarted_sentence_keeps_only_the_last_take() {
        let w = words("Aujourd'hui on va. Aujourd'hui on va parler de montage. C'est simple.");
        let retakes = find_retakes(&w);
        assert_eq!(retakes.len(), 1);
        assert_eq!(span_text(&w, retakes[0].dropped), "Aujourd'hui on va.");
        assert_eq!(
            span_text(&w, retakes[0].kept),
            "Aujourd'hui on va parler de montage."
        );
    }

    #[test]
    fn a_repeated_sentence_with_an_interjection_between_is_one_retake() {
        let w =
            words("Le montage est long à faire. Euh non. Le montage est vraiment long à faire.");
        let retakes = find_retakes(&w);
        assert_eq!(retakes.len(), 1);
        assert_eq!(
            span_text(&w, retakes[0].dropped),
            "Le montage est long à faire. Euh non."
        );
    }

    #[test]
    fn several_failed_takes_are_all_dropped() {
        let w = words("On commence. On commence par. On commence par importer.");
        let dropped: Vec<_> = find_retakes(&w)
            .iter()
            .map(|r| span_text(&w, r.dropped))
            .collect();
        assert_eq!(dropped, ["On commence.", "On commence par."]);
    }

    #[test]
    fn different_sentences_are_not_retakes() {
        let w = words(
            "On importe la vidéo. Ensuite on coupe les silences. Et on exporte le résultat. Oui. Oui.",
        );
        assert!(find_retakes(&w).is_empty());
    }

    #[test]
    fn removal_follows_the_cuts_the_words_are_in() {
        let source = source_words("un deux trois quatre", 0.0);
        let edl = Edl {
            cuts: vec![
                Cut {
                    asset: id(),
                    range: range(0.0, 0.95),
                },
                Cut {
                    asset: id(),
                    range: range(1.0, 2.0),
                },
            ],
        };
        let w = edit_words(&edl, |_| Some(&source));
        // "deux trois" straddles the two cuts: one removal per cut.
        let ops = removal_ops(&w, &find_text(&w, "deux trois"));
        assert_eq!(
            ops,
            [
                EdlOp::RemoveRange {
                    asset: id(),
                    start: 0.5,
                    end: 0.9
                },
                EdlOp::RemoveRange {
                    asset: id(),
                    start: 1.0,
                    end: 1.4
                },
            ]
        );
        let asset = Asset {
            id: id(),
            path: "a.mp4".into(),
            duration: 2.0,
            has_audio: true,
        };
        let edited = apply_ops(&edl, &ops, &[asset]).unwrap();
        let left = edit_words(&edited, |_| Some(&source));
        let texts: Vec<_> = left.iter().map(|w| w.text.as_str()).collect();
        assert_eq!(texts, ["un", "quatre"]);
    }
}
