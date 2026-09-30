//! EDL → ffmpeg `filter_complex` graph. Pure string building, so it is unit-tested
//! without running ffmpeg.

use autolad_core::ports::RenderOptions;
use autolad_core::TimeRange;

use crate::error::MediaError;

/// A cut resolved to the index of its `-i` input.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedCut {
    pub input: usize,
    pub range: TimeRange,
}

/// Builds a graph that trims every cut out of its input, conforms it to `opts`
/// (size, frame rate, audio format) and concatenates the results into `[outv][outa]`.
///
/// Each input stream feeds several trims, so it is fanned out with `split`/`asplit`:
/// a filter pad can only be consumed once.
///
/// `audio_inputs[i]` tells whether input `i` has an audio track. A silent input contributes
/// generated silence of the cut's length, so the concat still gets one audio stream per cut.
pub fn build_filter_graph(
    cuts: &[PlacedCut],
    audio_inputs: &[bool],
    opts: &RenderOptions,
) -> Result<String, MediaError> {
    if cuts.is_empty() {
        return Err(MediaError::EmptyEdl);
    }
    let input_count = audio_inputs.len();
    let has_audio = |input: usize| audio_inputs.get(input).copied().unwrap_or(true);

    let mut uses = vec![0usize; input_count];
    // Position of each cut among the cuts of its own input: selects its split output.
    let mut slots = Vec::with_capacity(cuts.len());
    for cut in cuts {
        let count = uses
            .get_mut(cut.input)
            .ok_or_else(|| MediaError::UnknownAsset(format!("input #{}", cut.input)))?;
        slots.push(*count);
        *count += 1;
    }

    let mut parts: Vec<String> = Vec::new();
    for (input, &n) in uses.iter().enumerate().filter(|(_, n)| **n > 0) {
        let vs: String = (0..n).map(|j| format!("[sv{input}_{j}]")).collect();
        parts.push(format!("[{input}:v]split={n}{vs}"));
        if has_audio(input) {
            let asp: String = (0..n).map(|j| format!("[sa{input}_{j}]")).collect();
            parts.push(format!("[{input}:a]asplit={n}{asp}"));
        }
    }

    let RenderOptions { width, height, fps } = *opts;
    for (i, (cut, slot)) in cuts.iter().zip(&slots).enumerate() {
        let (s, e) = (cut.range.start, cut.range.end);
        let input = cut.input;
        parts.push(format!(
            "[sv{input}_{slot}]trim=start={s:.6}:end={e:.6},setpts=PTS-STARTPTS,\
             scale={width}:{height}:force_original_aspect_ratio=decrease,\
             pad={width}:{height}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps={fps},format=yuv420p[v{i}]"
        ));
        if has_audio(input) {
            parts.push(format!(
                "[sa{input}_{slot}]atrim=start={s:.6}:end={e:.6},asetpts=PTS-STARTPTS,\
                 aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo[a{i}]"
            ));
        } else {
            let length = e - s;
            parts.push(format!(
                "anullsrc=channel_layout=stereo:sample_rate=48000:duration={length:.6},\
                 asetpts=PTS-STARTPTS,aformat=sample_fmts=fltp:channel_layouts=stereo[a{i}]"
            ));
        }
    }

    let pads: String = (0..cuts.len()).map(|i| format!("[v{i}][a{i}]")).collect();
    parts.push(format!("{pads}concat=n={}:v=1:a=1[outv][outa]", cuts.len()));
    Ok(parts.join(";\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cut(input: usize, s: f64, e: f64) -> PlacedCut {
        PlacedCut {
            input,
            range: TimeRange::new(s, e).unwrap(),
        }
    }

    const OPTS: RenderOptions = RenderOptions {
        width: 1280,
        height: 720,
        fps: 30.0,
    };

    #[test]
    fn empty_edl_is_rejected() {
        assert_eq!(
            build_filter_graph(&[], &[true], &OPTS),
            Err(MediaError::EmptyEdl)
        );
    }

    #[test]
    fn out_of_range_input_is_rejected() {
        assert!(matches!(
            build_filter_graph(&[cut(3, 0.0, 1.0)], &[true], &OPTS),
            Err(MediaError::UnknownAsset(_))
        ));
    }

    #[test]
    fn two_cuts_of_one_input_split_it_twice() {
        let g = build_filter_graph(&[cut(0, 0.0, 1.0), cut(0, 2.5, 4.0)], &[true], &OPTS).unwrap();
        assert!(g.contains("[0:v]split=2[sv0_0][sv0_1]"));
        assert!(g.contains("[0:a]asplit=2[sa0_0][sa0_1]"));
        assert!(g.contains("[sv0_0]trim=start=0.000000:end=1.000000"));
        assert!(g.contains("[sv0_1]trim=start=2.500000:end=4.000000"));
        assert!(g.contains("[v0][a0][v1][a1]concat=n=2:v=1:a=1[outv][outa]"));
    }

    #[test]
    fn cuts_are_conformed_to_output_format() {
        let g = build_filter_graph(&[cut(0, 0.0, 1.0)], &[true], &OPTS).unwrap();
        assert!(g.contains("scale=1280:720:force_original_aspect_ratio=decrease"));
        assert!(g.contains("pad=1280:720:(ow-iw)/2:(oh-ih)/2"));
        assert!(g.contains("fps=30,"));
        assert!(g.contains("aresample=48000"));
    }

    #[test]
    fn interleaved_inputs_keep_edl_order_and_own_slots() {
        let cuts = [cut(0, 0.0, 1.0), cut(1, 0.0, 2.0), cut(0, 5.0, 6.0)];
        let g = build_filter_graph(&cuts, &[true, true], &OPTS).unwrap();
        // Cut #2 is the second use of input 0, so it reads split output 1.
        assert!(g.contains("[sv0_1]trim=start=5.000000:end=6.000000"));
        assert!(g.contains("[sv1_0]trim=start=0.000000:end=2.000000"));
        assert!(g.contains("[v0][a0][v1][a1][v2][a2]concat=n=3"));
    }

    #[test]
    fn unused_inputs_get_no_split() {
        let g = build_filter_graph(&[cut(1, 0.0, 1.0)], &[true, true], &OPTS).unwrap();
        assert!(!g.contains("[0:v]"));
        assert!(g.contains("[1:v]split=1[sv1_0]"));
    }

    #[test]
    fn a_silent_input_gets_generated_silence_instead_of_audio_filters() {
        let g = build_filter_graph(&[cut(0, 1.0, 3.5)], &[false], &OPTS).unwrap();
        assert!(!g.contains("[0:a]"));
        assert!(!g.contains("atrim"));
        assert!(g.contains("anullsrc=channel_layout=stereo:sample_rate=48000:duration=2.500000"));
        assert!(g.contains("[v0][a0]concat=n=1:v=1:a=1[outv][outa]"));
    }

    #[test]
    fn silent_and_sounding_inputs_can_be_joined() {
        let cuts = [cut(0, 0.0, 1.0), cut(1, 0.0, 2.0)];
        let g = build_filter_graph(&cuts, &[true, false], &OPTS).unwrap();
        assert!(g.contains("[0:a]asplit=1[sa0_0]"));
        assert!(!g.contains("[1:a]"));
        assert!(g.contains("duration=2.000000"));
        assert!(g.contains("[v0][a0][v1][a1]concat=n=2"));
    }
}
