//! MCP server exposing AutoLad's editing engine to AI agents over stdio.
//!
//! `engine` holds all behaviour and is tested directly; `server` is a thin rmcp
//! adapter (argument parsing, error mapping) on top of it.

pub mod agent;
pub mod base64;
pub mod bridge;
pub mod engine;
pub mod error;
pub mod jobs;
pub mod paths;
pub mod project_file;
pub mod server;

pub use engine::Engine;
pub use error::EngineError;
pub use server::{run_stdio, serve_stdio, AutoladServer};
