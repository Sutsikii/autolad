use std::path::Path;

use autolad_core::ports::{MediaInfo, MediaProbe};
use autolad_core::CoreError;
use serde::Deserialize;

use crate::binaries::Binaries;
use crate::error::MediaError;
use crate::process::{capture, command};

#[derive(Debug, Clone)]
pub struct FfprobeProbe {
    binaries: Binaries,
}

impl FfprobeProbe {
    pub fn new(binaries: Binaries) -> Self {
        Self { binaries }
    }

    pub async fn probe_file(&self, path: &Path) -> Result<MediaInfo, MediaError> {
        let mut cmd = command(&self.binaries.ffprobe);
        cmd.args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ])
        .arg(path);
        let out = capture("ffprobe", cmd).await?;
        parse_probe(&String::from_utf8_lossy(&out.stdout))
    }
}

impl MediaProbe for FfprobeProbe {
    async fn probe(&self, path: &Path) -> Result<MediaInfo, CoreError> {
        Ok(self.probe_file(path).await?)
    }
}

#[derive(Deserialize)]
struct ProbeOutput {
    format: Option<Format>,
    #[serde(default)]
    streams: Vec<Stream>,
}

#[derive(Deserialize)]
struct Format {
    duration: Option<String>,
}

#[derive(Deserialize)]
struct Stream {
    codec_type: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    duration: Option<String>,
    disposition: Option<Disposition>,
}

#[derive(Deserialize)]
struct Disposition {
    #[serde(default)]
    attached_pic: u8,
}

pub fn parse_probe(json: &str) -> Result<MediaInfo, MediaError> {
    let out: ProbeOutput =
        serde_json::from_str(json).map_err(|e| MediaError::Parse(e.to_string()))?;

    // Cover art is exposed as a video stream; it must not make a song look like a video.
    let video = out.streams.iter().find(|s| {
        s.codec_type.as_deref() == Some("video")
            && s.disposition.as_ref().map_or(0, |d| d.attached_pic) == 0
    });
    let has_audio = out
        .streams
        .iter()
        .any(|s| s.codec_type.as_deref() == Some("audio"));

    let duration = out
        .format
        .as_ref()
        .and_then(|f| f.duration.as_deref())
        .or_else(|| out.streams.iter().find_map(|s| s.duration.as_deref()))
        .and_then(|d| d.parse::<f64>().ok())
        .filter(|d| d.is_finite() && *d > 0.0)
        .ok_or_else(|| MediaError::Parse("no usable duration".into()))?;

    let fps = video.and_then(|v| {
        [v.avg_frame_rate.as_deref(), v.r_frame_rate.as_deref()]
            .into_iter()
            .flatten()
            .find_map(parse_rate)
    });

    Ok(MediaInfo {
        duration,
        has_audio,
        has_video: video.is_some(),
        width: video.and_then(|v| v.width),
        height: video.and_then(|v| v.height),
        fps,
    })
}

/// Parses ffprobe's rational frame rate (`30000/1001`); `0/0` means "unknown".
fn parse_rate(rate: &str) -> Option<f64> {
    let (num, den) = rate.split_once('/')?;
    let (num, den): (f64, f64) = (num.parse().ok()?, den.parse().ok()?);
    (den > 0.0 && num > 0.0).then(|| num / den)
}

#[cfg(test)]
mod tests {
    use super::*;

    const VIDEO: &str = r#"{
      "streams": [
        {"codec_type":"video","width":1920,"height":1080,"avg_frame_rate":"30000/1001","r_frame_rate":"30/1"},
        {"codec_type":"audio"}
      ],
      "format": {"duration":"12.500000"}
    }"#;

    #[test]
    fn parses_video_with_audio() {
        let info = parse_probe(VIDEO).unwrap();
        assert_eq!(info.duration, 12.5);
        assert!(info.has_audio && info.has_video);
        assert_eq!((info.width, info.height), (Some(1920), Some(1080)));
        assert!((info.fps.unwrap() - 29.97).abs() < 0.01);
    }

    #[test]
    fn audio_only_file_has_no_video() {
        let json = r#"{"streams":[{"codec_type":"audio"}],"format":{"duration":"3.0"}}"#;
        let info = parse_probe(json).unwrap();
        assert!(info.has_audio && !info.has_video);
        assert_eq!(info.fps, None);
    }

    #[test]
    fn cover_art_is_not_video() {
        let json = r#"{"streams":[
            {"codec_type":"video","width":500,"height":500,"disposition":{"attached_pic":1}},
            {"codec_type":"audio"}],"format":{"duration":"3.0"}}"#;
        assert!(!parse_probe(json).unwrap().has_video);
    }

    #[test]
    fn falls_back_to_stream_duration() {
        let json = r#"{"streams":[{"codec_type":"video","duration":"4.0","avg_frame_rate":"0/0","r_frame_rate":"25/1"}],"format":{}}"#;
        let info = parse_probe(json).unwrap();
        assert_eq!(info.duration, 4.0);
        assert_eq!(info.fps, Some(25.0));
    }

    #[test]
    fn missing_duration_is_an_error() {
        assert!(parse_probe(r#"{"streams":[],"format":{}}"#).is_err());
        assert!(parse_probe("not json").is_err());
    }
}
