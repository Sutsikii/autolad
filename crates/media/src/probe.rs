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
    /// Pixel shape, `"4:3"` for anamorphic footage; `"0:1"` or missing means square.
    sample_aspect_ratio: Option<String>,
    #[serde(default)]
    side_data_list: Vec<SideData>,
    tags: Option<StreamTags>,
}

#[derive(Deserialize)]
struct SideData {
    /// Phones record how they were held here (recent ffprobe).
    rotation: Option<f64>,
}

#[derive(Deserialize)]
struct StreamTags {
    /// Same information, older container convention.
    rotate: Option<String>,
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

    let size = video.and_then(|v| {
        let sar = v.sample_aspect_ratio.as_deref().map_or(1.0, parse_sar);
        display_size(v.width?, v.height?, sar, rotation_of(v))
    });

    Ok(MediaInfo {
        duration,
        has_audio,
        has_video: video.is_some(),
        width: size.map(|s| s.0),
        height: size.map(|s| s.1),
        fps,
    })
}

/// Clockwise quarter turns to apply for display, as degrees in `{0, 90, 180, 270}`.
fn rotation_of(stream: &Stream) -> u32 {
    let degrees = stream
        .side_data_list
        .iter()
        .find_map(|d| d.rotation)
        .or_else(|| {
            let tag = stream.tags.as_ref()?.rotate.as_deref()?;
            tag.trim().parse::<f64>().ok()
        })
        .unwrap_or(0.0);
    // ffprobe reports counter-clockwise angles (-90 = a clockwise quarter turn); only the axis
    // matters for the size, so the sign is irrelevant here.
    (degrees.round() as i64).rem_euclid(360) as u32
}

/// ffprobe writes `4:3` (or `4/3`); `0:1`, garbage and non-positive ratios mean square pixels.
fn parse_sar(text: &str) -> f64 {
    let Some((num, den)) = text.split_once([':', '/']) else {
        return 1.0;
    };
    match (num.parse::<f64>(), den.parse::<f64>()) {
        (Ok(n), Ok(d)) if n > 0.0 && d > 0.0 => n / d,
        _ => 1.0,
    }
}

/// Size of the picture as a viewer sees it: pixel shape applied, then the rotation flag.
/// The coded size alone would make a phone video look landscape and a 4:3 stretch look square.
fn display_size(width: u32, height: u32, sar: f64, rotation: u32) -> Option<(u32, u32)> {
    if width == 0 || height == 0 {
        return None;
    }
    let shaped = (f64::from(width) * sar).round().max(1.0) as u32;
    Some(if rotation == 90 || rotation == 270 {
        (height, shaped)
    } else {
        (shaped, height)
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

    fn size_of(stream: &str) -> (Option<u32>, Option<u32>) {
        let json = format!(r#"{{"streams":[{stream}],"format":{{"duration":"2.0"}}}}"#);
        let info = parse_probe(&json).unwrap();
        (info.width, info.height)
    }

    #[test]
    fn a_phone_video_flagged_as_rotated_is_portrait() {
        let modern = r#"{"codec_type":"video","width":1920,"height":1080,
            "side_data_list":[{"side_data_type":"Display Matrix","rotation":-90}]}"#;
        assert_eq!(size_of(modern), (Some(1080), Some(1920)));
        let legacy = r#"{"codec_type":"video","width":1920,"height":1080,"tags":{"rotate":"270"}}"#;
        assert_eq!(size_of(legacy), (Some(1080), Some(1920)));
        let upside_down = r#"{"codec_type":"video","width":1920,"height":1080,
            "side_data_list":[{"rotation":180}]}"#;
        assert_eq!(size_of(upside_down), (Some(1920), Some(1080)));
    }

    #[test]
    fn anamorphic_pixels_are_applied_to_the_width() {
        let pal =
            r#"{"codec_type":"video","width":720,"height":576,"sample_aspect_ratio":"16:15"}"#;
        assert_eq!(size_of(pal), (Some(768), Some(576)));
        let hdv =
            r#"{"codec_type":"video","width":1440,"height":1080,"sample_aspect_ratio":"4:3"}"#;
        assert_eq!(size_of(hdv), (Some(1920), Some(1080)));
    }

    #[test]
    fn unknown_or_broken_pixel_shapes_mean_square_pixels() {
        for sar in ["0:1", "N/A", "abc:def", "-3:2", "5"] {
            let stream = format!(
                r#"{{"codec_type":"video","width":640,"height":360,"sample_aspect_ratio":"{sar}"}}"#
            );
            assert_eq!(size_of(&stream), (Some(640), Some(360)), "{sar}");
        }
    }

    #[test]
    fn pixel_shape_and_rotation_combine() {
        let both = r#"{"codec_type":"video","width":1440,"height":1080,"sample_aspect_ratio":"4:3",
            "side_data_list":[{"rotation":90}]}"#;
        assert_eq!(size_of(both), (Some(1080), Some(1920)));
    }

    #[test]
    fn missing_duration_is_an_error() {
        assert!(parse_probe(r#"{"streams":[],"format":{}}"#).is_err());
        assert!(parse_probe("not json").is_err());
    }
}
