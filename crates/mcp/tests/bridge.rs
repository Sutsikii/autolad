//! The agent bridge: an agent connecting to an engine hosted elsewhere (the desktop app)
//! edits that engine's project, and the host is told what happens.

// clippy's allow-unwrap-in-tests doesn't cover helper fns in integration-test files.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use autolad_mcp::agent::{AgentEvent, AgentLink, AgentPhase};
use autolad_mcp::bridge::{clear_info, connect, host, info_path};
use autolad_mcp::{AutoladServer, Engine};
use autolad_media::Binaries;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

fn binaries() -> Binaries {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../src-tauri/binaries");
    let triple = "x86_64-pc-windows-msvc";
    Binaries {
        ffmpeg: dir.join(format!("ffmpeg-{triple}.exe")),
        ffprobe: dir.join(format!("ffprobe-{triple}.exe")),
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("autolad-bridge-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn make_clip(path: &Path) {
    let status = Command::new(binaries().ffmpeg)
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
        ])
        .arg("testsrc=size=320x240:rate=25:duration=2")
        .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=2"])
        .args([
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-pix_fmt",
            "yuv420p",
        ])
        .args(["-c:a", "aac", "-shortest"])
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success());
}

struct Rpc {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
    next_id: u64,
}

impl Rpc {
    async fn send(&mut self, message: Value) {
        let mut line = message.to_string();
        line.push('\n');
        self.writer.write_all(line.as_bytes()).await.unwrap();
    }

    async fn call(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await;
        loop {
            let mut line = String::new();
            let read =
                tokio::time::timeout(Duration::from_secs(30), self.reader.read_line(&mut line))
                    .await
                    .expect("the server stopped answering")
                    .unwrap();
            assert!(read > 0, "the server closed the connection");
            let message: Value = serde_json::from_str(&line).unwrap();
            if message["id"] == json!(id) {
                return message;
            }
        }
    }
}

async fn open_session(data: &Path) -> Rpc {
    let stream = connect(data)
        .await
        .expect("the bridge should accept the agent");
    let (read, write) = stream.into_split();
    let mut rpc = Rpc {
        reader: BufReader::new(read),
        writer: write,
        next_id: 0,
    };
    rpc.call(
        "initialize",
        json!({
            "protocolVersion": "2025-03-26",
            "capabilities": {},
            "clientInfo": { "name": "bridge-test", "version": "0" }
        }),
    )
    .await;
    rpc.send(json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
        .await;
    rpc
}

#[tokio::test]
async fn an_agent_edits_the_hosts_project_and_the_host_sees_it() {
    let dir = scratch("session");
    let data = dir.join("data");
    let clip = dir.join("clip.mp4");
    make_clip(&clip);

    let engine = Arc::new(Engine::new(binaries(), data.clone()));
    let seen: Arc<Mutex<Vec<AgentEvent>>> = Arc::default();
    let sink = Arc::clone(&seen);
    let link = AgentLink::new(
        Arc::new(move |event| sink.lock().unwrap().push(event)),
        Duration::from_millis(5),
    );
    let bridge = host(
        AutoladServer::shared(Arc::clone(&engine), Some(link)),
        &data,
    )
    .await
    .unwrap();

    let mut rpc = open_session(&data).await;
    let imported = rpc
        .call(
            "tools/call",
            json!({ "name": "import_media", "arguments": { "path": clip } }),
        )
        .await;
    assert_ne!(imported["result"]["isError"], json!(true), "{imported}");

    // The host's own engine now holds what the agent imported.
    assert_eq!(engine.project_status().assets.len(), 1);

    let events = seen.lock().unwrap().clone();
    let phases: Vec<AgentPhase> = events
        .iter()
        .filter(|e| e.tool == "import_media")
        .map(|e| e.phase)
        .collect();
    assert_eq!(phases, vec![AgentPhase::Started, AgentPhase::Finished]);
    assert!(events[0].label.contains("clip.mp4"));
    assert!(events[0].changes_project);

    bridge.shutdown();
    assert!(!info_path(&data).exists());
}

#[tokio::test]
async fn nobody_answers_when_no_app_is_running() {
    let data = scratch("absent").join("data");
    assert!(connect(&data).await.is_none());

    // A stale file from a crashed app points at a dead port.
    let engine = Arc::new(Engine::new(binaries(), data.clone()));
    let bridge = host(AutoladServer::shared(engine, None), &data)
        .await
        .unwrap();
    let published = std::fs::read(info_path(&data)).unwrap();
    bridge.shutdown();
    std::fs::write(info_path(&data), published).unwrap();
    assert!(connect(&data).await.is_none());
    clear_info(&data);
}

#[tokio::test]
async fn a_wrong_secret_is_turned_away() {
    let data = scratch("secret").join("data");
    let engine = Arc::new(Engine::new(binaries(), data.clone()));
    let bridge = host(AutoladServer::shared(engine, None), &data)
        .await
        .unwrap();

    let info: Value = serde_json::from_slice(&std::fs::read(info_path(&data)).unwrap()).unwrap();
    let port = u16::try_from(info["port"].as_u64().unwrap()).unwrap();
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
        .await
        .unwrap();
    stream.write_all(b"not-the-secret\n").await.unwrap();
    let mut answer = Vec::new();
    let read = tokio::io::AsyncReadExt::read_to_end(&mut stream, &mut answer).await;
    assert!(read.is_ok());
    assert!(answer.is_empty(), "no acknowledgement for a wrong secret");
    bridge.shutdown();
}
