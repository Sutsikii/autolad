//! Drives the real `autolad.exe --mcp` over stdio with raw JSON-RPC, the way an
//! MCP client (Claude Code) does: handshake, list tools, then edit a video.
//! Set AUTOLAD_MCP_EXE to test another build (e.g. the release exe).

// clippy's allow-unwrap-in-tests doesn't cover helper fns in integration-test files.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const TIMEOUT: Duration = Duration::from_secs(60);

struct Client {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    next_id: u64,
}

impl Client {
    fn start(exe: &Path, home: &Path) -> Self {
        let mut child = Command::new(exe)
            .arg("--mcp")
            .env("AUTOLAD_HOME", home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("cannot start autolad --mcp");
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();

        // Reading on a thread lets every wait have a timeout instead of hanging the test.
        let (tx, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            stdin,
            lines,
            next_id: 0,
        }
    }

    fn send(&mut self, message: &Value) {
        writeln!(self.stdin, "{message}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));

        let deadline = Instant::now() + TIMEOUT;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let line = self
                .lines
                .recv_timeout(remaining)
                .unwrap_or_else(|_| panic!("no response to {method} within {TIMEOUT:?}"));
            let message: Value = serde_json::from_str(&line)
                .unwrap_or_else(|_| panic!("stdout must carry only JSON-RPC, got: {line}"));
            if message["id"] == json!(id) {
                assert!(message.get("error").is_none(), "{method} failed: {message}");
                return message["result"].clone();
            }
        }
    }

    /// Calls a tool and returns the whole `CallToolResult`.
    fn call(&mut self, name: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({"name": name, "arguments": arguments}))
    }

    /// Calls a tool that must succeed and returns its structured content.
    fn call_ok(&mut self, name: &str, arguments: Value) -> Value {
        let result = self.call(name, arguments);
        assert_ne!(result["isError"], json!(true), "{name} failed: {result}");
        result["structuredContent"].clone()
    }
}

impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("autolad-stdio-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn sidecar(tool: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("binaries")
        .join(format!("{tool}-x86_64-pc-windows-msvc.exe"));
    assert!(path.is_file(), "run `pwsh scripts/fetch-ffmpeg.ps1` first");
    path
}

fn make_clip(path: &Path) {
    let status = Command::new(sidecar("ffmpeg"))
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args([
            "-f",
            "lavfi",
            "-i",
            "testsrc=size=320x240:rate=25:duration=3",
        ])
        .args(["-f", "lavfi", "-i", "sine=frequency=440:duration=3"])
        .args(["-af", "volume=enable='between(t,1,2)':volume=0"])
        .args([
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-c:a",
            "aac",
            "-shortest",
        ])
        .arg(path)
        .status()
        .unwrap();
    assert!(status.success());
}

fn exe() -> PathBuf {
    std::env::var_os("AUTOLAD_MCP_EXE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_BIN_EXE_autolad")))
}

fn initialize(client: &mut Client) -> Value {
    let result = client.request(
        "initialize",
        json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {"name": "autolad-test", "version": "0"}
        }),
    );
    client.send(&json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    result
}

#[test]
fn handshake_and_tool_catalogue() {
    let mut client = Client::start(&exe(), &scratch("catalogue"));
    let init = initialize(&mut client);
    assert_eq!(init["serverInfo"]["name"], "autolad");
    assert!(init["instructions"]
        .as_str()
        .unwrap()
        .contains("import_media"));

    let tools = client.request("tools/list", json!({}));
    let names: Vec<&str> = tools["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    for expected in [
        "import_media",
        "project_status",
        "detect_silences",
        "transcribe",
        "build_silence_edl",
        "get_edl",
        "edit_edl",
        "undo",
        "redo",
        "get_edit_transcript",
        "cut_text",
        "remove_fillers",
        "remove_retakes",
        "preview_frame",
        "save_project",
        "open_project",
        "render_start",
        "render_status",
        "render_cancel",
        "export_subtitles",
    ] {
        assert!(
            names.contains(&expected),
            "missing tool {expected}: {names:?}"
        );
    }
    for tool in tools["tools"].as_array().unwrap() {
        assert_eq!(tool["inputSchema"]["type"], "object", "{}", tool["name"]);
        assert!(!tool["description"].as_str().unwrap().is_empty());
    }
}

#[test]
fn an_agent_can_edit_and_render_a_video() {
    let dir = scratch("edit");
    let clip = dir.join("talk.mp4");
    make_clip(&clip);
    let mut client = Client::start(&exe(), &dir.join("home"));
    initialize(&mut client);

    let asset = client.call_ok("import_media", json!({"path": clip}));
    let id = asset["id"].as_str().unwrap().to_owned();
    assert_eq!(asset["width"], 320);

    let silences = client.call_ok("detect_silences", json!({"asset_id": id}));
    assert_eq!(silences["silences"].as_array().unwrap().len(), 1);

    let built = client.call_ok(
        "build_silence_edl",
        json!({"asset_id": id, "max_gap": 0.0, "margin": 0.0, "min_segment": 0.0}),
    );
    assert_eq!(built["edl"]["cuts"].as_array().unwrap().len(), 2);

    let edited = client.call_ok("edit_edl", json!({"ops": [{"op": "delete", "index": 1}]}));
    assert_eq!(edited["cuts"].as_array().unwrap().len(), 1);

    let undone = client.call_ok("undo", json!({}));
    assert_eq!(undone["change"], "Delete clip 2");
    assert_eq!(undone["edl"]["cuts"].as_array().unwrap().len(), 2);
    let redone = client.call_ok("redo", json!({}));
    assert_eq!(redone["edl"]["cuts"].as_array().unwrap().len(), 1);

    // The image comes back as MCP image content the agent can look at.
    let frame = client.call(
        "preview_frame",
        json!({"timeline_time": 0.5, "max_width": 160}),
    );
    let content = frame["content"].as_array().unwrap();
    let image = content
        .iter()
        .find(|c| c["type"] == "image")
        .expect("image content");
    assert_eq!(image["mimeType"], "image/png");
    assert!(
        image["data"].as_str().unwrap().starts_with("iVBOR"),
        "PNG base64 signature"
    );

    let out = dir.join("out.mp4");
    let job = client.call_ok("render_start", json!({"output": out, "draft": true}));
    let job_id = job["job_id"].as_str().unwrap().to_owned();
    let deadline = Instant::now() + TIMEOUT;
    let status = loop {
        let status = client.call_ok("render_status", json!({"job_id": job_id}));
        if status["state"] != "running" {
            break status;
        }
        assert!(Instant::now() < deadline, "render never finished");
        std::thread::sleep(Duration::from_millis(100));
    };
    assert_eq!(status["state"], "done", "{status}");
    assert!(out.metadata().unwrap().len() > 0);
}

#[test]
fn tool_errors_reach_the_agent_as_readable_results() {
    let mut client = Client::start(&exe(), &scratch("errors"));
    initialize(&mut client);

    let result = client.call("detect_silences", json!({"asset_id": "ghost"}));
    assert_eq!(result["isError"], json!(true));
    let text = result["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("list_assets")
            || text.contains("project_status")
            || text.contains("unknown asset"),
        "{text}"
    );

    let result = client.call("preview_frame", json!({}));
    assert_eq!(result["isError"], json!(true));
}
