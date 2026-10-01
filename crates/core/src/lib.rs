//! Domain types and pure editing algorithms. No infrastructure dependency:
//! ffmpeg, whisper and Tauri live in other crates and implement [`ports`].

pub mod domain;
pub mod edl;
pub mod edl_edit;
pub mod error;
pub mod fcpxml;
pub mod history;
pub mod ports;
pub mod segments;
pub mod subtitles;
pub mod transcript;

pub use domain::{Asset, AssetId, Cut, Edl, Project, TimeRange};
pub use error::CoreError;
