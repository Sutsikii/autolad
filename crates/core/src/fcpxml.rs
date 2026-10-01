//! Final Cut Pro XML (FCPXML 1.9) of the edit, so it can be finished in Final Cut Pro or
//! DaVinci Resolve: one clip per cut on the primary storyline, pointing to the source files.

use std::fmt::Write as _;
use std::path::Path;

use crate::domain::{Asset, Edl};
use crate::error::CoreError;

/// Picture format of a source, as probed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MediaFormat {
    pub width: u32,
    pub height: u32,
    pub fps: f64,
}

impl Default for MediaFormat {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
            fps: 30.0,
        }
    }
}

/// A frame duration as the exact fraction FCPXML wants: NTSC rates are `1001/30000`, not a
/// rounded decimal, or every edit point drifts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FrameDuration {
    num: u64,
    den: u64,
}

impl FrameDuration {
    fn of(fps: f64) -> Self {
        const NTSC: [(f64, u64); 4] = [
            (23.976, 24_000),
            (29.97, 30_000),
            (47.952, 48_000),
            (59.94, 60_000),
        ];
        if let Some((_, den)) = NTSC.iter().find(|(rate, _)| (fps - rate).abs() < 0.01) {
            return Self {
                num: 1001,
                den: *den,
            };
        }
        let whole = if fps.is_finite() && fps >= 1.0 {
            fps.round() as u64
        } else {
            30
        };
        Self { num: 1, den: whole }
    }

    fn fps(self) -> f64 {
        self.den as f64 / self.num as f64
    }

    /// Nearest whole frame to `seconds`.
    fn frames(self, seconds: f64) -> u64 {
        (seconds.max(0.0) * self.fps()).round() as u64
    }

    /// `frames` as an FCPXML time: `"3003/30000s"`, or `"0s"`.
    fn time(self, frames: u64) -> String {
        if frames == 0 {
            "0s".to_owned()
        } else if self.num == 1 {
            format!("{frames}/{}s", self.den)
        } else {
            format!("{}/{}s", frames * self.num, self.den)
        }
    }

    fn attr(self) -> String {
        format!("{}/{}s", self.num, self.den)
    }
}

/// The edit as an FCPXML document named `name`. `sources` gives each asset of the EDL with
/// its format; the sequence takes the first cut's. Edit points are snapped to the sequence's
/// frames.
pub fn to_fcpxml(
    name: &str,
    edl: &Edl,
    sources: &[(Asset, MediaFormat)],
) -> Result<String, CoreError> {
    let first = edl.cuts.first().ok_or(CoreError::InvalidSetting(
        "the EDL is empty: nothing to export",
    ))?;
    let source_of = |id: &crate::domain::AssetId| {
        sources
            .iter()
            .position(|(asset, _)| asset.id == *id)
            .ok_or_else(|| CoreError::UnknownAsset(id.0.clone()))
    };
    let sequence = sources[source_of(&first.asset)?].1;
    let seq_frame = FrameDuration::of(sequence.fps);

    // Resource ids: the sequence format is r0, then each used source gets a format and an asset.
    let mut used: Vec<usize> = Vec::new();
    for cut in &edl.cuts {
        let index = source_of(&cut.asset)?;
        if !used.contains(&index) {
            used.push(index);
        }
    }
    let format_id = |slot: usize| format!("r{}", 1 + slot * 2);
    let asset_id = |slot: usize| format!("r{}", 2 + slot * 2);

    let mut resources = String::new();
    push_format(&mut resources, "r0", sequence);
    for (slot, &index) in used.iter().enumerate() {
        let (asset, format) = &sources[index];
        push_format(&mut resources, &format_id(slot), *format);
        let frame = FrameDuration::of(format.fps);
        let audio = if asset.has_audio {
            "hasAudio=\"1\" audioSources=\"1\" audioChannels=\"2\" audioRate=\"48000\""
        } else {
            "hasAudio=\"0\""
        };
        let _ = writeln!(
            resources,
            "    <asset id=\"{}\" name=\"{}\" start=\"0s\" duration=\"{}\" hasVideo=\"1\" \
             {audio} format=\"{}\">\n\
             \x20     <media-rep kind=\"original-media\" src=\"{}\"/>\n    </asset>",
            asset_id(slot),
            escape(&clip_name(&asset.path)),
            frame.time(frame.frames(asset.duration)),
            format_id(slot),
            escape(&file_url(&asset.path)),
        );
    }

    let mut spine = String::new();
    let mut offset = 0u64;
    for cut in &edl.cuts {
        let index = source_of(&cut.asset)?;
        let slot = used.iter().position(|&u| u == index).unwrap_or_default();
        let start = seq_frame.frames(cut.range.start);
        let length = seq_frame.frames(cut.range.end).saturating_sub(start);
        if length == 0 {
            continue;
        }
        let _ = writeln!(
            spine,
            "            <asset-clip ref=\"{}\" name=\"{}\" offset=\"{}\" start=\"{}\" duration=\"{}\"/>",
            asset_id(slot),
            escape(&clip_name(&sources[index].0.path)),
            seq_frame.time(offset),
            seq_frame.time(start),
            seq_frame.time(length),
        );
        offset += length;
    }

    let name = escape(name);
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE fcpxml>\n<fcpxml version=\"1.9\">\n  \
         <resources>\n{resources}  </resources>\n  <library>\n    <event name=\"{name}\">\n      \
         <project name=\"{name}\">\n        <sequence format=\"r0\" duration=\"{}\" tcStart=\"0s\" \
         tcFormat=\"NDF\" audioLayout=\"stereo\" audioRate=\"48k\">\n          <spine>\n{spine}          \
         </spine>\n        </sequence>\n      </project>\n    </event>\n  </library>\n</fcpxml>\n",
        seq_frame.time(offset)
    ))
}

fn push_format(out: &mut String, id: &str, format: MediaFormat) {
    let _ = writeln!(
        out,
        "    <format id=\"{id}\" frameDuration=\"{}\" width=\"{}\" height=\"{}\"/>",
        FrameDuration::of(format.fps).attr(),
        format.width,
        format.height
    );
}

fn clip_name(path: &Path) -> String {
    path.file_stem()
        .map_or_else(|| "clip".to_owned(), |s| s.to_string_lossy().into_owned())
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// `C:\Rushes\take 1.mp4` → `file:///C:/Rushes/take%201.mp4`; a network share
/// `\\nas\videos\a.mp4` → `file://nas/videos/a.mp4`.
fn file_url(path: &Path) -> String {
    let text = path.to_string_lossy().replace('\\', "/");
    let (prefix, rest) = match text.strip_prefix("//") {
        Some(share) => ("file://", share.to_owned()),
        None if text.starts_with('/') => ("file://", text),
        None => ("file:///", text),
    };
    let mut url = String::from(prefix);
    for byte in rest.bytes() {
        let keep = byte.is_ascii_alphanumeric() || b"-._~/:".contains(&byte);
        if keep {
            url.push(byte as char);
        } else {
            let _ = write!(url, "%{byte:02X}");
        }
    }
    url
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::domain::{AssetId, Cut, TimeRange};

    fn source(id: &str, path: &str, fps: f64) -> (Asset, MediaFormat) {
        (
            Asset {
                id: AssetId(id.into()),
                path: PathBuf::from(path),
                duration: 60.0,
                has_audio: true,
            },
            MediaFormat {
                width: 1920,
                height: 1080,
                fps,
            },
        )
    }

    fn cut(id: &str, start: f64, end: f64) -> Cut {
        Cut {
            asset: AssetId(id.into()),
            range: TimeRange::new(start, end).unwrap(),
        }
    }

    #[test]
    fn frame_rates_are_exact_fractions() {
        assert_eq!(FrameDuration::of(29.97002997).attr(), "1001/30000s");
        assert_eq!(FrameDuration::of(23.976).attr(), "1001/24000s");
        assert_eq!(FrameDuration::of(25.0).attr(), "1/25s");
        assert_eq!(FrameDuration::of(60.0).attr(), "1/60s");
        assert_eq!(FrameDuration::of(f64::NAN).attr(), "1/30s");
        let ntsc = FrameDuration::of(29.97);
        assert_eq!(ntsc.time(ntsc.frames(1.0)), "30030/30000s");
        assert_eq!(ntsc.time(0), "0s");
    }

    #[test]
    fn cuts_follow_each_other_on_the_storyline() {
        let edl = Edl {
            cuts: vec![cut("a", 1.0, 3.0), cut("b", 0.5, 1.5), cut("a", 10.0, 11.0)],
        };
        let sources = [
            source("a", r"C:\Rushes\take 1.mp4", 25.0),
            source("b", r"C:\Rushes\b-roll & co.mov", 25.0),
        ];
        let xml = to_fcpxml("My edit", &edl, &sources).unwrap();
        assert!(xml.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE fcpxml>"));
        assert!(xml.contains("<fcpxml version=\"1.9\">"));
        assert!(xml.contains(
            "<asset-clip ref=\"r2\" name=\"take 1\" offset=\"0s\" start=\"25/25s\" duration=\"50/25s\""
        ));
        assert!(
            xml.contains("ref=\"r4\" name=\"b-roll &amp; co\" offset=\"50/25s\" start=\"13/25s\"")
        );
        assert!(xml.contains("ref=\"r2\" name=\"take 1\" offset=\"75/25s\" start=\"250/25s\""));
        assert!(xml.contains("<sequence format=\"r0\" duration=\"100/25s\""));
        assert_eq!(
            xml.matches("<asset id=").count(),
            2,
            "each source is listed once"
        );
        assert!(xml.contains("src=\"file:///C:/Rushes/take%201.mp4\""));
        assert!(xml.contains("<event name=\"My edit\">"));
    }

    #[test]
    fn empty_edits_and_unknown_sources_are_refused() {
        let sources = [source("a", "a.mp4", 25.0)];
        assert!(to_fcpxml("x", &Edl::default(), &sources).is_err());
        let edl = Edl {
            cuts: vec![cut("ghost", 0.0, 1.0)],
        };
        assert_eq!(
            to_fcpxml("x", &edl, &sources),
            Err(CoreError::UnknownAsset("ghost".into()))
        );
    }

    #[test]
    fn a_cut_shorter_than_a_frame_is_left_out() {
        let edl = Edl {
            cuts: vec![cut("a", 1.0, 1.01), cut("a", 2.0, 3.0)],
        };
        let xml = to_fcpxml("x", &edl, &[source("a", "a.mp4", 25.0)]).unwrap();
        assert_eq!(xml.matches("<asset-clip").count(), 1);
        assert!(xml.contains("offset=\"0s\" start=\"50/25s\""));
    }

    #[test]
    fn paths_become_file_urls() {
        assert_eq!(
            file_url(Path::new(r"C:\Vidéos\été 1.mp4")),
            "file:///C:/Vid%C3%A9os/%C3%A9t%C3%A9%201.mp4"
        );
        assert_eq!(
            file_url(Path::new(r"\\nas\share\a.mp4")),
            "file://nas/share/a.mp4"
        );
    }
}
