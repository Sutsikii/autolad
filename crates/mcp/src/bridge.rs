//! Lets `autolad --mcp` (started by an agent) talk to the desktop app that is already open,
//! so the agent edits the very project the user is looking at.
//!
//! The app listens on a loopback port and publishes `bridge.json` (port + secret) in the data
//! folder. `--mcp` reads it, connects, and simply pipes stdin/stdout to the app, which serves
//! the MCP protocol itself. When no app is running the file is missing or stale, the connection
//! fails, and `--mcp` falls back to a standalone engine.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};
use std::time::Duration;

use rmcp::ServiceExt;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

use crate::error::EngineError;
use crate::server::AutoladServer;

const INFO_FILE: &str = "bridge.json";
const ACK: &[u8] = b"ok\n";
const MAX_TOKEN_LINE: usize = 128;
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Serialize, Deserialize)]
struct BridgeInfo {
    port: u16,
    token: String,
    pid: u32,
}

pub fn info_path(data_dir: &Path) -> PathBuf {
    data_dir.join(INFO_FILE)
}

/// Removes the published address. Called when the app exits so agents stop trying it.
pub fn clear_info(data_dir: &Path) {
    // Best effort: a leftover file is harmless, `connect` validates it anyway.
    let _ = std::fs::remove_file(info_path(data_dir));
}

/// A secret only this machine's user can read (it lives in their local app data).
fn new_token() -> String {
    let random = |salt: u64| {
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(salt);
        hasher.finish()
    };
    format!("{:016x}{:016x}", random(1), random(2))
}

/// Running bridge: dropping it does not stop it, call [`Bridge::shutdown`].
pub struct Bridge {
    task: JoinHandle<()>,
    data_dir: PathBuf,
}

impl Bridge {
    pub fn shutdown(self) {
        self.task.abort();
        clear_info(&self.data_dir);
    }
}

/// Starts accepting agent connections, each served by a clone of `server` (so they all share
/// the same engine and project).
pub async fn host(server: AutoladServer, data_dir: &Path) -> Result<Bridge, EngineError> {
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| EngineError::Io(format!("cannot open the agent bridge: {e}")))?;
    let port = listener
        .local_addr()
        .map_err(|e| EngineError::Io(e.to_string()))?
        .port();
    let token = new_token();
    write_info(
        data_dir,
        &BridgeInfo {
            port,
            token: token.clone(),
            pid: std::process::id(),
        },
    )?;

    let task = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                continue;
            };
            let server = server.clone();
            let token = token.clone();
            tokio::spawn(async move {
                let Some(stream) = authenticate(stream, &token).await else {
                    return;
                };
                if let Ok(service) = server.serve(stream).await {
                    // The agent disconnecting is the normal end of the session.
                    let _ = service.waiting().await;
                }
            });
        }
    });
    Ok(Bridge {
        task,
        data_dir: data_dir.to_path_buf(),
    })
}

fn write_info(data_dir: &Path, info: &BridgeInfo) -> Result<(), EngineError> {
    let io = |e: std::io::Error| EngineError::Io(format!("{}: {e}", data_dir.display()));
    std::fs::create_dir_all(data_dir).map_err(io)?;
    let json = serde_json::to_vec(info).map_err(|e| EngineError::Io(e.to_string()))?;
    // Written then renamed so a reader never sees half a file.
    let tmp = data_dir.join(format!("{INFO_FILE}.tmp"));
    std::fs::write(&tmp, json).map_err(io)?;
    std::fs::rename(&tmp, info_path(data_dir)).map_err(io)
}

/// Reads the secret line, answers `ok`. Anything else closes the connection.
async fn authenticate(mut stream: TcpStream, token: &str) -> Option<TcpStream> {
    let line = tokio::time::timeout(HANDSHAKE_TIMEOUT, read_line(&mut stream))
        .await
        .ok()??;
    if line != token {
        return None;
    }
    stream.write_all(ACK).await.ok()?;
    Some(stream)
}

/// Byte by byte on purpose: a buffered reader would swallow the start of the MCP stream.
async fn read_line(stream: &mut TcpStream) -> Option<String> {
    let mut line = Vec::new();
    loop {
        let byte = stream.read_u8().await.ok()?;
        if byte == b'\n' {
            return String::from_utf8(line).ok();
        }
        line.push(byte);
        if line.len() > MAX_TOKEN_LINE {
            return None;
        }
    }
}

/// Connects to a running app, or `None` when there is none (or its address is stale).
pub async fn connect(data_dir: &Path) -> Option<TcpStream> {
    let bytes = tokio::fs::read(info_path(data_dir)).await.ok()?;
    let info: BridgeInfo = serde_json::from_slice(&bytes).ok()?;
    let mut stream = tokio::time::timeout(
        HANDSHAKE_TIMEOUT,
        TcpStream::connect(("127.0.0.1", info.port)),
    )
    .await
    .ok()?
    .ok()?;
    stream
        .write_all(format!("{}\n", info.token).as_bytes())
        .await
        .ok()?;
    let mut ack = [0u8; 3];
    tokio::time::timeout(HANDSHAKE_TIMEOUT, stream.read_exact(&mut ack))
        .await
        .ok()?
        .ok()?;
    (ack == ACK).then_some(stream)
}

/// Pipes the agent's stdin/stdout to the app until either side closes.
pub async fn pipe_stdio(stream: TcpStream) -> Result<(), EngineError> {
    let (mut from_app, mut to_app) = stream.into_split();
    let mut stdin = tokio::io::stdin();
    let mut stdout = tokio::io::stdout();
    let up = async {
        let copied = tokio::io::copy(&mut stdin, &mut to_app).await;
        // Tell the app the agent is gone so its session ends.
        let _ = to_app.shutdown().await;
        copied
    };
    let down = tokio::io::copy(&mut from_app, &mut stdout);
    tokio::select! {
        result = up => result,
        result = down => result,
    }
    .map(|_| ())
    .map_err(|e| EngineError::Io(format!("agent bridge closed: {e}")))
}
