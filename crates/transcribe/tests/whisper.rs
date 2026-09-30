//! End-to-end transcription with a real model on synthesized French speech.
//!
//! Ignored by default: it downloads the `small` model (~190 MB, cached in `.cache/models`)
//! and needs the Windows French voice. Run with:
//!   cargo test -p autolad-transcribe --test whisper -- --ignored --nocapture

// clippy's allow-unwrap-in-tests doesn't cover helper fns in integration-test files.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use autolad_media::Binaries;
use autolad_transcribe::{ModelStore, TranscribeOptions, WhisperModel, WhisperTranscriber};

fn binaries() -> Binaries {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/binaries");
    let triple = "x86_64-pc-windows-msvc";
    let b = Binaries {
        ffmpeg: dir.join(format!("ffmpeg-{triple}.exe")),
        ffprobe: dir.join(format!("ffprobe-{triple}.exe")),
    };
    assert!(
        b.ffmpeg.is_file(),
        "run `pwsh scripts/fetch-ffmpeg.ps1` first"
    );
    b
}

fn speech_wav(dir: &Path) -> PathBuf {
    let wav = dir.join("speech.wav");
    // Two sentences separated by a 2 s pause, so timestamps can be sanity-checked.
    let script = format!(
        r#"Add-Type -AssemblyName System.Speech
$s = New-Object System.Speech.Synthesis.SpeechSynthesizer
$s.SelectVoice('Microsoft Hortense Desktop')
$s.SetOutputToWaveFile('{}')
$s.SpeakSsml('<speak version="1.0" xml:lang="fr-FR" xmlns="http://www.w3.org/2001/10/synthesis">Bonjour à tous, bienvenue sur la chaîne.<break time="2s"/>Aujourd''hui, nous allons apprendre à monter une vidéo.</speak>')
$s.Dispose()"#,
        wav.display()
    );
    let status = Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .status()
        .unwrap();
    assert!(status.success() && wav.is_file(), "speech synthesis failed");
    wav
}

async fn setup(word_timestamps: bool) -> (WhisperTranscriber, PathBuf) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.cache");
    let store = ModelStore::new(root.join("models"));
    let model = store
        .ensure(WhisperModel::Small, &|done, total| {
            if total > 0 && done % (32 * 1024 * 1024) < 65_536 {
                println!("model download: {}%", done * 100 / total);
            }
        })
        .await
        .unwrap();

    let work = std::env::temp_dir().join(format!(
        "autolad-whisper-{}-{word_timestamps}",
        std::process::id()
    ));
    std::fs::create_dir_all(&work).unwrap();
    let wav = speech_wav(&work);

    let transcriber = WhisperTranscriber::load(
        binaries(),
        model,
        TranscribeOptions {
            language: "fr".into(),
            word_timestamps,
            ..TranscribeOptions::default()
        },
    )
    .await
    .unwrap();
    (transcriber, wav)
}

#[tokio::test]
#[ignore = "downloads a ~190 MB model"]
async fn transcribes_french_speech_with_timestamps() {
    let (transcriber, wav) = setup(false).await;

    let started = Instant::now();
    let segments = transcriber.transcribe_file(&wav).await.unwrap();
    println!("transcribed in {:.1}s", started.elapsed().as_secs_f32());
    for s in &segments {
        println!("[{:.2} -> {:.2}] {}", s.range.start, s.range.end, s.text);
    }

    let text = segments
        .iter()
        .map(|s| s.text.to_lowercase())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(text.contains("bonjour"), "got: {text}");
    assert!(
        text.contains("vidéo") || text.contains("video") || text.contains("monter"),
        "got: {text}"
    );

    // The 2 s pause pushes the second sentence well past the start.
    assert!(segments.first().unwrap().range.start < 1.5);
    assert!(segments.last().unwrap().range.end > 4.0);
    assert!(segments
        .windows(2)
        .all(|w| w[0].range.start <= w[1].range.start));
}

#[tokio::test]
#[ignore = "downloads a ~190 MB model"]
async fn word_timestamps_give_one_segment_per_word() {
    let (transcriber, wav) = setup(true).await;

    let words = transcriber.transcribe_file(&wav).await.unwrap();
    for w in &words {
        println!("[{:.2} -> {:.2}] {}", w.range.start, w.range.end, w.text);
    }

    // Two sentences of 5+ words each: phrase-level output would give 2 segments.
    assert!(words.len() >= 8, "got {} segments", words.len());
    assert!(words.iter().all(|w| w.text.split_whitespace().count() <= 2));
    let bonjour = words
        .iter()
        .find(|w| w.text.to_lowercase().contains("bonjour"))
        .expect("word 'bonjour' missing");
    assert!(bonjour.range.start < 1.0 && bonjour.range.end < 2.0);
}

#[tokio::test]
async fn loading_a_missing_model_fails_cleanly() {
    let err = WhisperTranscriber::load(
        binaries(),
        PathBuf::from("does-not-exist.bin"),
        TranscribeOptions::default(),
    )
    .await
    .err()
    .unwrap();
    assert!(matches!(
        err,
        autolad_transcribe::TranscribeError::ModelMissing(_)
    ));
}
