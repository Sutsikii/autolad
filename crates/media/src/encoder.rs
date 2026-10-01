use crate::binaries::Binaries;
use crate::process::command;

/// Bits per pixel and frame allowed at most: 12 Mb/s for 1080p30, 25 Mb/s for 1080p60.
/// Measured on grainy 1080p30 footage, capping libx264 at 12 Mb/s instead of 40 cut the
/// file by more than three for a VMAF loss under half a point.
const MAX_BITS_PER_PIXEL: f64 = 0.2;
/// The cap never drops below this (tiny frames), nor above the top: YouTube asks 35-68 Mb/s
/// for 4K60 uploads.
const BITRATE_RANGE_KBPS: (u32, u32) = (2_000, 40_000);

/// Highest video bitrate of a render, in kb/s: proportional to the pixels per second, since
/// a flat cap either starves 4K or lets grainy 1080p take three times what it needs.
pub fn max_bitrate_kbps(width: u32, height: u32, fps: f64) -> u32 {
    let bits = f64::from(width) * f64::from(height) * fps * MAX_BITS_PER_PIXEL;
    let (min, max) = BITRATE_RANGE_KBPS;
    // Saturating float-to-int cast; a NaN fps lands on the floor.
    ((bits / 1000.0) as u32).clamp(min, max)
}

/// H.264 encoder. Hardware first (much faster), libx264 as the universal fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoder {
    Nvenc,
    Qsv,
    Amf,
    X264,
}

impl Encoder {
    const PREFERENCE: [Encoder; 3] = [Encoder::Nvenc, Encoder::Qsv, Encoder::Amf];

    fn name(self) -> &'static str {
        match self {
            Encoder::Nvenc => "h264_nvenc",
            Encoder::Qsv => "h264_qsv",
            Encoder::Amf => "h264_amf",
            Encoder::X264 => "libx264",
        }
    }

    /// Hardware encoders only accept frames inside a range (NVENC refuses tiny ones and
    /// H.264 tops out at 4096); libx264 takes any even size, so it covers the rest.
    pub fn for_size(self, width: u32, height: u32) -> Encoder {
        const HARDWARE_MIN: (u32, u32) = (256, 144);
        const HARDWARE_MAX: u32 = 4096;
        let fits = |w: u32, h: u32| {
            w >= HARDWARE_MIN.0 && h >= HARDWARE_MIN.1 && w <= HARDWARE_MAX && h <= HARDWARE_MAX
        };
        if self == Encoder::X264 || fits(width, height) {
            self
        } else {
            Encoder::X264
        }
    }

    pub fn is_hardware(self) -> bool {
        self != Encoder::X264
    }

    /// Video encoding arguments, tuned to look close to `libx264 -crf 20`.
    ///
    /// Constant quality alone lets grainy high-resolution footage (foliage, 4K phone or
    /// action-cam rushes) climb to 100 Mb/s; NVENC and libx264 are capped at `max_kbps`
    /// (see [`max_bitrate_kbps`]) with a two-second buffer, room for detailed scenes without
    /// overshooting for long. QSV and AMF keep pure constant quality: their capped modes
    /// could not be checked on real hardware.
    pub fn args(self, max_kbps: u32) -> Vec<String> {
        let maxrate = format!("{max_kbps}k");
        let bufsize = format!("{}k", max_kbps.saturating_mul(2));
        let args: Vec<&str> = match self {
            Encoder::Nvenc => vec![
                "-c:v",
                "h264_nvenc",
                "-preset",
                "p5",
                "-rc",
                "vbr",
                "-cq",
                "21",
                "-b:v",
                "0",
                "-maxrate",
                &maxrate,
                "-bufsize",
                &bufsize,
            ],
            Encoder::Qsv => vec!["-c:v", "h264_qsv", "-global_quality", "21"],
            Encoder::Amf => vec![
                "-c:v", "h264_amf", "-quality", "quality", "-rc", "cqp", "-qp_i", "21", "-qp_p",
                "21",
            ],
            Encoder::X264 => vec![
                "-c:v", "libx264", "-preset", "medium", "-crf", "20", "-maxrate", &maxrate,
                "-bufsize", &bufsize,
            ],
        };
        args.into_iter().map(str::to_owned).collect()
    }

    /// Picks the first hardware encoder that really works on this machine.
    /// `-encoders` lists what was compiled in, not what the GPU supports, so each
    /// candidate is exercised with a tiny throwaway encode.
    pub async fn detect(binaries: &Binaries) -> Encoder {
        for candidate in Self::PREFERENCE {
            if candidate.works(binaries).await {
                return candidate;
            }
        }
        Encoder::X264
    }

    async fn works(self, binaries: &Binaries) -> bool {
        let mut cmd = command(&binaries.ffmpeg);
        cmd.args(["-hide_banner", "-v", "error", "-f", "lavfi", "-i"])
            .arg("color=c=black:s=256x256:d=0.2")
            .args(["-frames:v", "2", "-c:v", self.name(), "-f", "null", "-"]);
        matches!(cmd.output().await, Ok(o) if o.status.success())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hardware_is_kept_only_for_sizes_it_can_encode() {
        let hw = Encoder::Nvenc;
        assert_eq!(hw.for_size(1920, 1080), hw);
        assert_eq!(hw.for_size(1080, 1920), hw);
        assert_eq!(hw.for_size(4096, 2160), hw);
        // Too small for NVENC, too big for H.264 hardware: software takes over.
        assert_eq!(hw.for_size(96, 54), Encoder::X264);
        assert_eq!(hw.for_size(54, 96), Encoder::X264);
        assert_eq!(hw.for_size(7680, 4320), Encoder::X264);
        assert_eq!(hw.for_size(4320, 7680), Encoder::X264);
        // Software has no such limit.
        assert_eq!(Encoder::X264.for_size(96, 54), Encoder::X264);
    }

    #[test]
    fn only_the_software_encoder_is_not_hardware() {
        assert!(
            Encoder::Nvenc.is_hardware()
                && Encoder::Qsv.is_hardware()
                && Encoder::Amf.is_hardware()
        );
        assert!(!Encoder::X264.is_hardware());
    }

    #[test]
    fn nvenc_and_x264_cap_the_bitrate() {
        for enc in [Encoder::Nvenc, Encoder::X264] {
            let args = enc.args(12_000);
            let value = |flag: &str| {
                let pos = args.iter().position(|a| a == flag).unwrap();
                args[pos + 1].clone()
            };
            assert_eq!(value("-maxrate"), "12000k");
            assert_eq!(value("-bufsize"), "24000k");
        }
    }

    #[test]
    fn the_cap_follows_the_pixel_rate() {
        assert_eq!(max_bitrate_kbps(1920, 1080, 30.0), 12_441);
        assert_eq!(max_bitrate_kbps(1920, 1080, 60.0), 24_883);
        assert_eq!(max_bitrate_kbps(1080, 1920, 30.0), 12_441);
        // 4K and up hit the ceiling, tiny drafts the floor.
        assert_eq!(max_bitrate_kbps(3840, 2160, 30.0), 40_000);
        assert_eq!(max_bitrate_kbps(7680, 4320, 60.0), 40_000);
        assert_eq!(max_bitrate_kbps(640, 360, 30.0), 2_000);
        assert_eq!(max_bitrate_kbps(1920, 1080, f64::NAN), 2_000);
    }

    #[test]
    fn every_encoder_selects_its_codec() {
        for (enc, name) in [
            (Encoder::Nvenc, "h264_nvenc"),
            (Encoder::Qsv, "h264_qsv"),
            (Encoder::Amf, "h264_amf"),
            (Encoder::X264, "libx264"),
        ] {
            let args = enc.args(12_000);
            let pos = args.iter().position(|a| a == "-c:v").unwrap();
            assert_eq!(args[pos + 1], name);
        }
    }
}
