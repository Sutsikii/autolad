use crate::binaries::Binaries;
use crate::process::command;

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

    /// Video encoding arguments, tuned to look close to `libx264 -crf 20`.
    pub fn args(self) -> Vec<&'static str> {
        match self {
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
            ],
            Encoder::Qsv => vec!["-c:v", "h264_qsv", "-global_quality", "21"],
            Encoder::Amf => vec![
                "-c:v", "h264_amf", "-quality", "quality", "-rc", "cqp", "-qp_i", "21", "-qp_p",
                "21",
            ],
            Encoder::X264 => vec!["-c:v", "libx264", "-preset", "medium", "-crf", "20"],
        }
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
    fn every_encoder_selects_its_codec() {
        for (enc, name) in [
            (Encoder::Nvenc, "h264_nvenc"),
            (Encoder::Qsv, "h264_qsv"),
            (Encoder::Amf, "h264_amf"),
            (Encoder::X264, "libx264"),
        ] {
            let args = enc.args();
            let pos = args.iter().position(|a| *a == "-c:v").unwrap();
            assert_eq!(args[pos + 1], name);
        }
    }
}
