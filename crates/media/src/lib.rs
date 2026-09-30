//! ffmpeg/ffprobe implementation of the `core` ports.
//! Text parsing and filter-graph building are pure and unit-tested; the process
//! wrappers are covered by integration tests against the real sidecar binaries.

pub mod analyze;
pub mod audio;
pub mod binaries;
pub mod encoder;
pub mod error;
pub mod filtergraph;
pub mod frame;
pub mod hash;
pub mod parse;
pub mod probe;
pub mod render;

mod process;

pub use analyze::FfmpegAnalyzer;
pub use binaries::Binaries;
pub use encoder::Encoder;
pub use error::MediaError;
pub use probe::FfprobeProbe;
pub use render::FfmpegRenderer;
