//! EDL → ffmpeg `filter_complex` graph. Pure string building, so it is unit-tested
//! without running ffmpeg.

use autolad_core::ports::RenderOptions;
use autolad_core::TimeRange;

use crate::error::MediaError;

/// Length of the fades at each audio cut. A cut in the middle of a waveform clicks; 10 ms of
/// fade removes the click without being heard as a fade.
pub const CUT_FADE: f64 = 0.01;

/// Loudness range and true-peak ceiling used with the loudness target (streaming defaults).
const LOUDNESS_RANGE: f64 = 11.0;
const TRUE_PEAK_DB: f64 = -1.5;

/// Two cuts closer than this in the same source play as one continuous take.
const CONTIGUOUS: f64 = 0.001;

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
///
/// Audio fades in and out at every join where the sound jumps (not between two cuts that
/// continue the same take), and the mix is normalized when `opts.loudness` is set.
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

    let RenderOptions {
        width,
        height,
        fps,
        loudness,
    } = *opts;
    for (i, (cut, slot)) in cuts.iter().zip(&slots).enumerate() {
        let (s, e) = (cut.range.start, cut.range.end);
        let input = cut.input;
        parts.push(format!(
            "[sv{input}_{slot}]trim=start={s:.6}:end={e:.6},setpts=PTS-STARTPTS,\
             scale=iw*sar:ih,\
             scale={width}:{height}:force_original_aspect_ratio=decrease,\
             pad={width}:{height}:(ow-iw)/2:(oh-ih)/2,setsar=1,fps={fps},format=yuv420p[v{i}]"
        ));
        if has_audio(input) {
            let fades = fades(cuts, i);
            parts.push(format!(
                "[sa{input}_{slot}]atrim=start={s:.6}:end={e:.6},asetpts=PTS-STARTPTS,\
                 aresample=48000,aformat=sample_fmts=fltp:channel_layouts=stereo{fades}[a{i}]"
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
    let n = cuts.len();
    match loudness {
        None => parts.push(format!("{pads}concat=n={n}:v=1:a=1[outv][outa]")),
        Some(target) => {
            parts.push(format!("{pads}concat=n={n}:v=1:a=1[outv][mix]"));
            // loudnorm works at 192 kHz internally: bring it back to the output rate.
            parts.push(format!(
                "[mix]loudnorm=I={target}:TP={TRUE_PEAK_DB}:LRA={LOUDNESS_RANGE},\
                 aresample=48000[outa]"
            ));
        }
    }
    Ok(parts.join(";\n"))
}

/// `afade` filters for cut `i`: in unless it continues the previous cut, out unless the next
/// cut continues it. Short cuts get shorter fades so the two never overlap.
fn fades(cuts: &[PlacedCut], i: usize) -> String {
    let cut = cuts[i];
    let continues = |a: &PlacedCut, b: &PlacedCut| {
        a.input == b.input && (a.range.end - b.range.start).abs() < CONTIGUOUS
    };
    let fade = CUT_FADE.min(cut.range.duration() / 2.0);
    let mut out = String::new();
    if i == 0 || !continues(&cuts[i - 1], &cut) {
        out.push_str(&format!(",afade=t=in:st=0:d={fade:.6}"));
    }
    if cuts.get(i + 1).is_none_or(|next| !continues(&cut, next)) {
        let start = cut.range.duration() - fade;
        out.push_str(&format!(",afade=t=out:st={start:.6}:d={fade:.6}"));
    }
    out
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
        loudness: None,
    };

    /// The filter chain that produces pad `[a{i}]`.
    fn audio_chain(graph: &str, i: usize) -> String {
        let pad = format!("[a{i}]");
        graph
            .split(";\n")
            .find(|part| part.ends_with(&pad))
            .unwrap()
            .to_owned()
    }

    #[test]
    fn audio_fades_at_jumps_but_not_inside_a_continuous_take() {
        // Cuts 0 and 1 continue each other (a split); cut 2 jumps elsewhere in the source.
        let cuts = [cut(0, 0.0, 2.0), cut(0, 2.0, 3.0), cut(0, 5.0, 6.0)];
        let g = build_filter_graph(&cuts, &[true], &OPTS).unwrap();
        assert!(audio_chain(&g, 0).contains("afade=t=in:st=0:d=0.010000"));
        assert!(!audio_chain(&g, 0).contains("afade=t=out"));
        assert!(!audio_chain(&g, 1).contains("afade=t=in"));
        assert!(audio_chain(&g, 1).contains("afade=t=out:st=0.990000:d=0.010000"));
        let last = audio_chain(&g, 2);
        assert!(last.contains("afade=t=in") && last.contains("afade=t=out"));
    }

    #[test]
    fn a_tiny_cut_gets_fades_that_do_not_overlap() {
        let g = build_filter_graph(&[cut(0, 1.0, 1.01)], &[true], &OPTS).unwrap();
        assert!(g.contains("afade=t=in:st=0:d=0.005000"));
        assert!(g.contains("afade=t=out:st=0.005000:d=0.005000"));
    }

    #[test]
    fn loudness_target_normalizes_the_mix() {
        let opts = RenderOptions {
            loudness: Some(-14.0),
            ..OPTS
        };
        let g = build_filter_graph(&[cut(0, 0.0, 1.0)], &[true], &opts).unwrap();
        assert!(g.contains("concat=n=1:v=1:a=1[outv][mix]"));
        assert!(g.contains("[mix]loudnorm=I=-14:TP=-1.5:LRA=11,aresample=48000[outa]"));
        let plain = build_filter_graph(&[cut(0, 0.0, 1.0)], &[true], &OPTS).unwrap();
        assert!(!plain.contains("loudnorm"));
    }

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
