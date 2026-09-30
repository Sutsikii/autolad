//! Editing through the transcript: read what the edit says, cut a piece of text, remove the
//! hesitations or the failed takes. Word timings come from whisper's word mode, computed on
//! first use and stored in the project like every transcript.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use autolad_core::edl_edit::EdlOp;
use autolad_core::ports::TranscriptSegment;
use autolad_core::subtitles::{cues, to_srt, to_vtt, Cue, CueLayout};
use autolad_core::transcript::{
    edit_words, filler_words, find_retakes, find_text, removal_ops, sentence_spans, span_text,
    EditWord, Span,
};
use autolad_core::{AssetId, TimeRange};
use serde::Serialize;

use super::{io_err, lock, EdlSummary, Engine, TranscribeRequest};
use crate::error::EngineError;

/// Longest quote of the cut text in an undo label.
const LABEL_QUOTE_CHARS: usize = 40;

/// Searching near a phrase of the source: its whisper bounds can be this far off.
const NEAR_SLACK: f64 = 1.0;

/// One sentence of the edit, at its position on the edited timeline.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct EditLine {
    pub start: f64,
    pub end: f64,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct EditTranscript {
    pub lines: Vec<EditLine>,
    pub full_text: String,
    /// Every clip has word timings. Otherwise some lines are whole whisper phrases, whose
    /// bounds can be a second off.
    pub word_level: bool,
    /// Assets of the edit that have sound but no transcript yet: call `transcribe` on them.
    pub untranscribed: Vec<String>,
}

/// A piece of speech removed (or that would be removed) from the edit.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TextCut {
    pub text: String,
    /// Where it was on the edited timeline before the change.
    pub timeline_start: f64,
    pub duration: f64,
    /// For a failed take: the retry that is kept instead.
    pub replaced_by: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TextEditReport {
    pub removed: Vec<TextCut>,
    pub removed_seconds: f64,
    /// `false` for a dry run: nothing was changed.
    pub applied: bool,
    pub edl: EdlSummary,
}

/// Which occurrences of a text to cut.
pub enum Occurrence {
    /// The n-th, counting from 1.
    Nth(usize),
    All,
    /// The one said around `start..end` of `asset_id`'s source (a phrase picked in its
    /// transcript).
    Near {
        asset_id: String,
        start: f64,
        end: f64,
    },
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct SubtitleExport {
    pub path: PathBuf,
    pub cues: usize,
}

pub struct CutTextRequest {
    pub text: String,
    pub occurrence: Occurrence,
}

impl Engine {
    /// What the edit says, sentence by sentence, from the transcripts already computed.
    /// Never starts a transcription.
    pub fn edit_transcript(&self) -> EditTranscript {
        let (edl, audio_assets) = self.edit_assets();
        let mut by_asset = HashMap::new();
        let mut untranscribed = Vec::new();
        let mut word_level = true;
        for id in audio_assets {
            let mut stored = self.stored_transcripts(&id.0);
            // Words first: they give the exact edit, phrases only an approximation.
            stored.sort_by_key(|(key, _)| !key.word_timestamps);
            match stored.into_iter().next() {
                Some((key, segments)) => {
                    word_level &= key.word_timestamps;
                    by_asset.insert(id, segments);
                }
                None => untranscribed.push(id.0),
            }
        }
        let words = edit_words(&edl, |id| by_asset.get(id).map(Vec::as_slice));
        let lines: Vec<EditLine> = sentence_spans(&words)
            .into_iter()
            .map(|span| EditLine {
                start: words[span.0].timeline.start,
                end: words[span.1].timeline.end,
                text: span_text(&words, span),
            })
            .collect();
        let full_text = lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        EditTranscript {
            lines,
            full_text,
            word_level: word_level && untranscribed.is_empty(),
            untranscribed,
        }
    }

    /// Removes a piece of text from the edit.
    pub async fn cut_text(&self, req: CutTextRequest) -> Result<TextEditReport, EngineError> {
        let words = self.word_level_edit().await?;
        let found = find_text(&words, &req.text);
        if found.is_empty() {
            return Err(EngineError::Invalid(format!(
                "{:?} is not said in the edit (already cut, or worded differently); \
                 get_edit_transcript shows what the edit says",
                req.text
            )));
        }
        let chosen = choose(&words, found, req.occurrence)?;
        let spans: Vec<(Span, Option<String>)> = chosen.into_iter().map(|s| (s, None)).collect();
        let label = format!("Cut \u{201c}{}\u{201d}", shorten(&req.text));
        self.remove_spans(&words, &spans, label, true).await
    }

    /// Removes every hesitation sound (euh, um…) from the edit, or lists them on a dry run.
    pub async fn remove_fillers(&self, dry_run: bool) -> Result<TextEditReport, EngineError> {
        let words = self.word_level_edit().await?;
        let spans: Vec<(Span, Option<String>)> = filler_words(&words)
            .into_iter()
            .map(|i| ((i, i), None))
            .collect();
        let label = format!("Remove {}", counted(spans.len(), "hesitation"));
        self.remove_spans(&words, &spans, label, !dry_run).await
    }

    /// Removes the sentences that were started over, keeping the last attempt.
    pub async fn remove_retakes(&self, dry_run: bool) -> Result<TextEditReport, EngineError> {
        let words = self.word_level_edit().await?;
        let spans: Vec<(Span, Option<String>)> = find_retakes(&words)
            .into_iter()
            .map(|r| (r.dropped, Some(span_text(&words, r.kept))))
            .collect();
        let label = format!("Remove {}", counted(spans.len(), "retake"));
        self.remove_spans(&words, &spans, label, !dry_run).await
    }

    /// Writes the subtitles of the edit to an `.srt` or `.vtt` file, timed on the edited
    /// timeline.
    pub async fn export_subtitles(&self, output: &Path) -> Result<SubtitleExport, EngineError> {
        let output = std::path::absolute(output).map_err(|e| io_err(output, &e))?;
        let extension = output
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase());
        let format: fn(&[Cue]) -> String = match extension.as_deref() {
            Some("srt") => to_srt,
            Some("vtt") => to_vtt,
            _ => {
                return Err(EngineError::Invalid(
                    "subtitles are written as .srt or .vtt".into(),
                ))
            }
        };
        if !output.parent().is_some_and(Path::is_dir) {
            return Err(EngineError::Invalid(format!(
                "output folder does not exist: {}",
                output.display()
            )));
        }
        let cues = self.subtitle_cues().await?;
        tokio::fs::write(&output, format(&cues))
            .await
            .map_err(|e| io_err(&output, &e))?;
        Ok(SubtitleExport {
            path: output,
            cues: cues.len(),
        })
    }

    /// Subtitles of the edit as a SubRip scratch file, for burning into a render.
    pub(super) async fn subtitle_file(&self) -> Result<PathBuf, EngineError> {
        let cues = self.subtitle_cues().await?;
        let path = self.scratch_path("subtitles", "srt").await?;
        tokio::fs::write(&path, to_srt(&cues))
            .await
            .map_err(|e| io_err(&path, &e))?;
        Ok(path)
    }

    async fn subtitle_cues(&self) -> Result<Vec<Cue>, EngineError> {
        let words = self.word_level_edit().await?;
        let cues = cues(&words, self.cue_layout());
        if cues.is_empty() {
            return Err(EngineError::Invalid(
                "nothing is said in the edit: there are no subtitles to make".into(),
            ));
        }
        Ok(cues)
    }

    /// Cues are sized for the sequence's frame, which is the first clip's.
    fn cue_layout(&self) -> CueLayout {
        let state = lock(&self.state);
        state
            .project
            .edl
            .cuts
            .first()
            .and_then(|cut| {
                state
                    .project
                    .assets
                    .iter()
                    .find(|e| e.asset.id == cut.asset)
            })
            .and_then(|e| Some(CueLayout::for_frame(e.width?, e.height?)))
            .unwrap_or(CueLayout::LANDSCAPE)
    }

    /// The EDL and the assets it plays that have sound, each once.
    fn edit_assets(&self) -> (autolad_core::Edl, Vec<AssetId>) {
        let state = lock(&self.state);
        let mut ids: Vec<AssetId> = Vec::new();
        for cut in &state.project.edl.cuts {
            let has_audio = state
                .project
                .assets
                .iter()
                .any(|e| e.asset.id == cut.asset && e.asset.has_audio);
            if has_audio && !ids.contains(&cut.asset) {
                ids.push(cut.asset.clone());
            }
        }
        (state.project.edl.clone(), ids)
    }

    /// The words the edit plays, with word timings, transcribing the clips that lack them.
    async fn word_level_edit(&self) -> Result<Vec<EditWord>, EngineError> {
        let (_, audio_assets) = self.edit_assets();
        if audio_assets.is_empty() {
            return Err(EngineError::Invalid(
                "the edit has no clip with sound: there is no speech to edit".into(),
            ));
        }
        let mut by_asset = HashMap::new();
        for id in audio_assets {
            let words = self.word_transcript(&id.0).await?;
            by_asset.insert(id, words);
        }
        // Transcribing can take minutes: work on the EDL as it is now.
        let edl = lock(&self.state).project.edl.clone();
        Ok(edit_words(&edl, |id| by_asset.get(id).map(Vec::as_slice)))
    }

    /// Word-mode transcript of an asset, computed with the model and language of its phrase
    /// transcript when there is one.
    async fn word_transcript(&self, asset_id: &str) -> Result<Vec<TranscriptSegment>, EngineError> {
        let stored = self.stored_transcripts(asset_id);
        if let Some((_, words)) = stored.iter().find(|(key, _)| key.word_timestamps) {
            return Ok(words.clone());
        }
        let phrase_key = stored.into_iter().next().map(|(key, _)| key);
        let request = TranscribeRequest {
            asset_id: asset_id.to_owned(),
            language: phrase_key
                .as_ref()
                .map(|k| k.language.clone())
                .filter(|l| l != "auto"),
            model: phrase_key.map(|k| k.model),
            word_timestamps: true,
        };
        let report = self.transcribe(request).await?;
        Ok(report
            .segments
            .into_iter()
            .filter_map(|e| {
                let range = TimeRange::new(e.start, e.end).ok()?;
                Some(TranscriptSegment {
                    range,
                    text: e.text,
                })
            })
            .collect())
    }

    async fn remove_spans(
        &self,
        words: &[EditWord],
        spans: &[(Span, Option<String>)],
        label: String,
        apply: bool,
    ) -> Result<TextEditReport, EngineError> {
        let removed: Vec<TextCut> = spans
            .iter()
            .map(|((first, last), kept)| TextCut {
                text: span_text(words, (*first, *last)),
                timeline_start: words[*first].timeline.start,
                duration: words[*last].timeline.end - words[*first].timeline.start,
                replaced_by: kept.clone(),
            })
            .collect();
        let only_spans: Vec<Span> = spans.iter().map(|(span, _)| *span).collect();
        let ops = removal_ops(words, &only_spans);
        let removed_seconds = ops
            .iter()
            .map(|op| match op {
                EdlOp::RemoveRange { start, end, .. } => end - start,
                _ => 0.0,
            })
            .sum();
        let edl = if apply && !ops.is_empty() {
            self.change_edl(&ops, label).await?
        } else {
            self.edl_summary()
        };
        Ok(TextEditReport {
            removed,
            removed_seconds,
            applied: apply,
            edl,
        })
    }
}

fn choose(
    words: &[EditWord],
    found: Vec<Span>,
    occurrence: Occurrence,
) -> Result<Vec<Span>, EngineError> {
    let count = found.len();
    match occurrence {
        Occurrence::All => Ok(found),
        Occurrence::Nth(n) => found
            .get(n.wrapping_sub(1))
            .map(|span| vec![*span])
            .ok_or_else(|| {
                EngineError::Invalid(format!(
                    "occurrence {n} does not exist: the text is said {count} time(s) in the edit"
                ))
            }),
        Occurrence::Near {
            asset_id,
            start,
            end,
        } => found
            .into_iter()
            .find(|&(first, _)| {
                let word = &words[first];
                word.asset.0 == asset_id
                    && word.source.start >= start - NEAR_SLACK
                    && word.source.start <= end + NEAR_SLACK
            })
            .map(|span| vec![span])
            .ok_or_else(|| EngineError::Invalid("that phrase is no longer in the edit".to_owned())),
    }
}

fn counted(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("1 {noun}")
    } else {
        format!("{n} {noun}s")
    }
}

fn shorten(text: &str) -> String {
    let text = text.trim();
    if text.chars().count() <= LABEL_QUOTE_CHARS {
        return text.to_owned();
    }
    let cut: String = text.chars().take(LABEL_QUOTE_CHARS - 1).collect();
    format!("{}…", cut.trim_end())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_are_spelled_for_humans() {
        assert_eq!(counted(1, "retake"), "1 retake");
        assert_eq!(counted(3, "retake"), "3 retakes");
    }

    #[test]
    fn long_quotes_are_shortened_for_the_undo_menu() {
        assert_eq!(shorten("  court "), "court");
        let long = "a".repeat(60);
        let short = shorten(&long);
        assert_eq!(short.chars().count(), LABEL_QUOTE_CHARS);
        assert!(short.ends_with('…'));
    }
}
