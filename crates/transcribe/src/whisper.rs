use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use autolad_core::ports::{Transcriber, TranscriptSegment};
use autolad_core::{CoreError, TimeRange};
use autolad_media::audio::decode_pcm_16k_mono;
use autolad_media::Binaries;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::error::TranscribeError;

/// Segments whisper itself flags as "probably no speech" are usually hallucinations
/// on silence or music ("Thanks for watching!"), so they are dropped.
const NO_SPEECH_CUTOFF: f32 = 0.85;

#[derive(Debug, Clone, PartialEq)]
pub struct TranscribeOptions {
    /// `"auto"` to detect, or an ISO 639-1 code (`"fr"`).
    pub language: String,
    /// Inference threads; `None` lets whisper.cpp choose.
    pub threads: Option<i32>,
    /// One segment per word instead of per phrase. Phrase boundaries can be seconds
    /// off; word timings are what makes cutting on speech precise.
    pub word_timestamps: bool,
}

impl Default for TranscribeOptions {
    fn default() -> Self {
        Self {
            language: "auto".to_owned(),
            threads: None,
            word_timestamps: false,
        }
    }
}

impl TranscribeOptions {
    /// whisper-rs panics on interior NULs, so the language is checked up front.
    fn validate(&self) -> Result<(), TranscribeError> {
        let ok = self.language == "auto"
            || (matches!(self.language.len(), 2 | 3)
                && self.language.chars().all(|c| c.is_ascii_lowercase()));
        if ok {
            Ok(())
        } else {
            Err(TranscribeError::InvalidLanguage(self.language.clone()))
        }
    }
}

/// Stops the running inference when the transcription future is dropped.
struct CancelOnDrop(Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

#[derive(Clone)]
pub struct WhisperTranscriber {
    ctx: Arc<WhisperContext>,
    binaries: Binaries,
    options: TranscribeOptions,
}

impl WhisperTranscriber {
    /// Loads the model (hundreds of MB, and GPU init), so it runs off the async runtime.
    pub async fn load(
        binaries: Binaries,
        model_path: PathBuf,
        options: TranscribeOptions,
    ) -> Result<Self, TranscribeError> {
        options.validate()?;
        if !model_path.is_file() {
            return Err(TranscribeError::ModelMissing(model_path));
        }
        let ctx = tokio::task::spawn_blocking(move || {
            let path = model_path.to_string_lossy().into_owned();
            WhisperContext::new_with_params(&path, WhisperContextParameters::default())
        })
        .await
        .map_err(|e| TranscribeError::Task(e.to_string()))?
        .map_err(|e| TranscribeError::Whisper(e.to_string()))?;

        Ok(Self {
            ctx: Arc::new(ctx),
            binaries,
            options,
        })
    }

    /// Same loaded model with different options: cheap, the weights are shared.
    pub fn with_options(&self, options: TranscribeOptions) -> Result<Self, TranscribeError> {
        options.validate()?;
        Ok(Self {
            options,
            ..self.clone()
        })
    }

    /// Dropping the returned future cancels the inference.
    pub async fn transcribe_file(
        &self,
        path: &Path,
    ) -> Result<Vec<TranscriptSegment>, TranscribeError> {
        let samples = decode_pcm_16k_mono(&self.binaries, path)
            .await
            .map_err(|e| TranscribeError::Audio(e.to_string()))?;

        let cancel = Arc::new(AtomicBool::new(false));
        let _guard = CancelOnDrop(cancel.clone());
        let ctx = self.ctx.clone();
        let options = self.options.clone();
        tokio::task::spawn_blocking(move || run_inference(&ctx, &options, &samples, cancel))
            .await
            .map_err(|e| TranscribeError::Task(e.to_string()))?
    }
}

fn run_inference(
    ctx: &WhisperContext,
    options: &TranscribeOptions,
    samples: &[f32],
    cancel: Arc<AtomicBool>,
) -> Result<Vec<TranscriptSegment>, TranscribeError> {
    let whisper_err = |e: whisper_rs::WhisperError| TranscribeError::Whisper(e.to_string());

    let mut state = ctx.create_state().map_err(whisper_err)?;
    let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    params.set_language(Some(&options.language));
    if let Some(threads) = options.threads {
        params.set_n_threads(threads);
    }
    if options.word_timestamps {
        params.set_token_timestamps(true);
        params.set_split_on_word(true);
        params.set_max_len(1);
    }
    params.set_print_special(false);
    params.set_print_progress(false);
    params.set_print_realtime(false);
    params.set_print_timestamps(false);
    params.set_suppress_blank(true);
    // whisper-rs 0.16.0 casts the callback pointer to the closure's concrete type, but
    // stores it as a boxed trait object: with a bare closure the abort callback reads
    // garbage and whisper_full fails with -6. Passing the box itself as `F` makes the
    // cast match. Revisit when whisper-rs fixes its trampoline.
    let abort: Box<dyn FnMut() -> bool> = Box::new(move || cancel.load(Ordering::Relaxed));
    params.set_abort_callback_safe::<_, Box<dyn FnMut() -> bool>>(abort);

    state.full(params, samples).map_err(whisper_err)?;

    Ok(state
        .as_iter()
        .filter_map(|seg| {
            let text = seg.to_str_lossy().ok()?;
            to_segment(
                seg.start_timestamp(),
                seg.end_timestamp(),
                &text,
                seg.no_speech_probability(),
            )
        })
        .collect())
}

/// Converts one whisper segment (timestamps in centiseconds) into a domain segment,
/// or `None` if it is empty, degenerate, or likely a silence hallucination.
fn to_segment(
    start_cs: i64,
    end_cs: i64,
    text: &str,
    no_speech_prob: f32,
) -> Option<TranscriptSegment> {
    let text = text.trim();
    if text.is_empty() || no_speech_prob > NO_SPEECH_CUTOFF {
        return None;
    }
    let range = TimeRange::new(start_cs as f64 / 100.0, end_cs as f64 / 100.0).ok()?;
    Some(TranscriptSegment {
        range,
        text: text.to_owned(),
    })
}

impl Transcriber for WhisperTranscriber {
    async fn transcribe(&self, path: &Path) -> Result<Vec<TranscriptSegment>, CoreError> {
        Ok(self.transcribe_file(path).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segment_timestamps_are_centiseconds() {
        let s = to_segment(150, 425, "  Bonjour tout le monde ", 0.01).unwrap();
        assert_eq!(s.range, TimeRange::new(1.5, 4.25).unwrap());
        assert_eq!(s.text, "Bonjour tout le monde");
    }

    #[test]
    fn empty_degenerate_and_hallucinated_segments_are_dropped() {
        assert!(to_segment(0, 100, "   ", 0.0).is_none());
        assert!(to_segment(100, 100, "x", 0.0).is_none());
        assert!(to_segment(0, 100, "Thanks for watching!", 0.95).is_none());
    }

    #[test]
    fn language_validation() {
        let opts = |l: &str| TranscribeOptions {
            language: l.to_owned(),
            ..TranscribeOptions::default()
        };
        assert!(opts("auto").validate().is_ok());
        assert!(opts("fr").validate().is_ok());
        assert!(opts("FR").validate().is_err());
        assert!(opts("fr\0").validate().is_err());
        assert!(opts("").validate().is_err());
        assert!(opts("french").validate().is_err());
    }

    #[test]
    fn dropping_the_guard_raises_the_cancel_flag() {
        let flag = Arc::new(AtomicBool::new(false));
        drop(CancelOnDrop(flag.clone()));
        assert!(flag.load(Ordering::Relaxed));
    }
}
