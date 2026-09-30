//! Registers this executable as an MCP server in the Claude apps, so nobody has to edit JSON
//! by hand. Claude Desktop reads a config file we can merge into; Claude Code rewrites its own
//! file constantly, so it is only read here and changed through its CLI.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Map, Value};

use crate::error::EngineError;

/// Name of the server in the Claude configs.
pub const SERVER_NAME: &str = "autolad";
const DESKTOP_CONFIG: &str = "claude_desktop_config.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum ClaudeApp {
    Desktop,
    Code,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct AppConnection {
    /// The app is installed (Desktop: its config folder exists; Code: its CLI was found).
    pub installed: bool,
    /// AutoLad is registered there and points to this executable.
    pub connected: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ClaudeSetup {
    pub desktop: AppConnection,
    pub code: AppConnection,
    /// Command that registers AutoLad in Claude Code by hand.
    pub code_command: String,
}

/// Where the Claude apps and this executable live. Resolved from the environment in
/// production, from temp folders in tests.
pub struct Locations {
    pub exe: PathBuf,
    pub app_data: Option<PathBuf>,
    pub local_app_data: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub claude_cli: Option<PathBuf>,
}

impl Locations {
    pub fn discover() -> Result<Self, EngineError> {
        let exe = std::env::current_exe()
            .map_err(|e| EngineError::Io(format!("cannot locate autolad.exe: {e}")))?;
        let env_dir = |name: &str| std::env::var_os(name).map(PathBuf::from);
        let home = env_dir("USERPROFILE");
        Ok(Self {
            exe,
            app_data: env_dir("APPDATA"),
            local_app_data: env_dir("LOCALAPPDATA"),
            claude_cli: find_claude_cli(std::env::var_os("PATH"), home.as_deref()),
            home,
        })
    }

    /// Claude Desktop config folders: the classic installer's, and the Microsoft Store
    /// package's (its roaming folder is redirected).
    fn desktop_dirs(&self) -> Vec<PathBuf> {
        let mut dirs = Vec::new();
        if let Some(app_data) = &self.app_data {
            dirs.push(app_data.join("Claude"));
        }
        if let Some(local) = &self.local_app_data {
            let packages = std::fs::read_dir(local.join("Packages"))
                .into_iter()
                .flatten();
            for package in packages.flatten() {
                if package.file_name().to_string_lossy().starts_with("Claude_") {
                    dirs.push(package.path().join("LocalCache/Roaming/Claude"));
                }
            }
        }
        dirs.retain(|d| d.is_dir());
        dirs
    }
}

pub fn status(at: &Locations) -> ClaudeSetup {
    let desktop_configs: Vec<Option<String>> = at
        .desktop_dirs()
        .iter()
        .map(|dir| std::fs::read_to_string(dir.join(DESKTOP_CONFIG)).ok())
        .collect();
    let code_config = at
        .home
        .as_ref()
        .and_then(|home| std::fs::read_to_string(home.join(".claude.json")).ok());
    ClaudeSetup {
        desktop: AppConnection {
            installed: !desktop_configs.is_empty(),
            connected: !desktop_configs.is_empty()
                && desktop_configs
                    .iter()
                    .all(|json| json.as_deref().is_some_and(|j| points_to(j, &at.exe))),
        },
        code: AppConnection {
            installed: at.claude_cli.is_some(),
            connected: code_config.is_some_and(|j| points_to(&j, &at.exe)),
        },
        code_command: code_command(&at.exe),
    }
}

/// Registers AutoLad in `app`. Claude Desktop must be restarted to load it; Claude Code picks
/// it up in new sessions.
pub async fn connect(app: ClaudeApp, at: &Locations) -> Result<ClaudeSetup, EngineError> {
    match app {
        ClaudeApp::Desktop => connect_desktop(at).await?,
        ClaudeApp::Code => connect_code(at).await?,
    }
    Ok(status(at))
}

async fn connect_desktop(at: &Locations) -> Result<(), EngineError> {
    let dirs = at.desktop_dirs();
    if dirs.is_empty() {
        return Err(EngineError::Invalid(
            "Claude Desktop is not installed (no Claude folder in AppData)".into(),
        ));
    }
    for dir in dirs {
        let path = dir.join(DESKTOP_CONFIG);
        let current = match tokio::fs::read_to_string(&path).await {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(EngineError::Io(format!("{}: {e}", path.display()))),
        };
        let updated = with_server(&current, &at.exe)
            .map_err(|e| EngineError::Invalid(format!("{}: {e}", path.display())))?;
        let tmp = path.with_extension("json.tmp");
        tokio::fs::write(&tmp, updated)
            .await
            .map_err(|e| EngineError::Io(format!("{}: {e}", tmp.display())))?;
        tokio::fs::rename(&tmp, &path)
            .await
            .map_err(|e| EngineError::Io(format!("{}: {e}", path.display())))?;
    }
    Ok(())
}

async fn connect_code(at: &Locations) -> Result<(), EngineError> {
    let cli = at.claude_cli.as_ref().ok_or_else(|| {
        EngineError::Invalid(format!(
            "the claude command was not found; run this in a terminal instead: {}",
            code_command(&at.exe)
        ))
    })?;
    // `add` refuses an existing name, and an old entry may point to a moved executable.
    let _ = run_cli(cli, &["mcp", "remove", "--scope", "user", SERVER_NAME]).await;
    let exe = at.exe.to_string_lossy();
    let args = [
        "mcp",
        "add",
        "--scope",
        "user",
        SERVER_NAME,
        "--",
        &exe,
        "--mcp",
    ];
    let output = run_cli(cli, &args).await?;
    if output.status.success() {
        Ok(())
    } else {
        Err(EngineError::Io(format!(
            "claude mcp add failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

async fn run_cli(cli: &Path, args: &[&str]) -> Result<std::process::Output, EngineError> {
    let mut command = tokio::process::Command::new(cli);
    command.args(args).kill_on_drop(true);
    #[cfg(windows)]
    {
        // No console window flashing from the desktop app.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
        .output()
        .await
        .map_err(|e| EngineError::Io(format!("{}: {e}", cli.display())))
}

/// `claude.exe` (native install) or `claude.cmd` (npm install), on the PATH or in the native
/// installer's folder.
fn find_claude_cli(path: Option<std::ffi::OsString>, home: Option<&Path>) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = path
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    if let Some(home) = home {
        dirs.push(home.join(".local/bin"));
    }
    dirs.iter()
        .flat_map(|dir| ["claude.exe", "claude.cmd"].map(|name| dir.join(name)))
        .find(|candidate| candidate.is_file())
}

fn code_command(exe: &Path) -> String {
    format!(
        "claude mcp add --scope user {SERVER_NAME} -- \"{}\" --mcp",
        exe.display()
    )
}

/// `config` (a Claude Desktop config, possibly empty) with the AutoLad server set to `exe`.
/// Every other setting and server is kept. Refuses a file that is not a JSON object rather
/// than overwrite what the user wrote.
pub fn with_server(config: &str, exe: &Path) -> Result<String, String> {
    let mut root: Value = if config.trim().is_empty() {
        json!({})
    } else {
        serde_json::from_str(config).map_err(|e| format!("not valid JSON ({e})"))?
    };
    let root_map = root
        .as_object_mut()
        .ok_or("the config is not a JSON object")?;
    let servers = root_map
        .entry("mcpServers")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or("mcpServers is not a JSON object")?;
    servers.insert(
        SERVER_NAME.to_owned(),
        json!({ "command": exe, "args": ["--mcp"] }),
    );
    serde_json::to_string_pretty(&root).map_err(|e| e.to_string())
}

/// Whether a Claude config (Desktop's, or Claude Code's `~/.claude.json`) runs this executable
/// as the AutoLad server.
pub fn points_to(config: &str, exe: &Path) -> bool {
    let Ok(root) = serde_json::from_str::<Value>(config) else {
        return false;
    };
    root.get("mcpServers")
        .and_then(|servers| servers.get(SERVER_NAME))
        .and_then(|server| server.get("command"))
        .and_then(Value::as_str)
        .is_some_and(|command| same_path(Path::new(command), exe))
}

/// Windows paths are case-insensitive and accept both slashes.
fn same_path(a: &Path, b: &Path) -> bool {
    let normalize = |p: &Path| p.to_string_lossy().replace('/', "\\").to_lowercase();
    normalize(a) == normalize(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exe() -> PathBuf {
        PathBuf::from(r"C:\Program Files\AutoLad\autolad.exe")
    }

    #[test]
    fn an_empty_config_gets_the_server() {
        let json = with_server("", &exe()).unwrap();
        assert!(points_to(&json, &exe()));
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["mcpServers"]["autolad"]["args"], json!(["--mcp"]));
    }

    #[test]
    fn other_settings_and_servers_are_kept() {
        let config = r#"{"theme":"dark","mcpServers":{"files":{"command":"npx"},
            "autolad":{"command":"C:/old/autolad.exe"}}}"#;
        let json = with_server(config, &exe()).unwrap();
        let value: Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["theme"], "dark");
        assert_eq!(value["mcpServers"]["files"]["command"], "npx");
        assert!(points_to(&json, &exe()));
    }

    #[test]
    fn a_broken_config_is_refused_not_overwritten() {
        assert!(with_server("{oops", &exe()).is_err());
        assert!(with_server("[1, 2]", &exe()).is_err());
        assert!(with_server(r#"{"mcpServers": 3}"#, &exe()).is_err());
    }

    #[test]
    fn an_entry_for_another_executable_is_not_a_connection() {
        let other = with_server("", Path::new(r"D:\old\autolad.exe")).unwrap();
        assert!(!points_to(&other, &exe()));
        assert!(!points_to("{}", &exe()));
        assert!(!points_to("nonsense", &exe()));
        let spelled_differently = r#"{"mcpServers":{"autolad":
            {"command":"c:/program files/autolad/AUTOLAD.EXE"}}}"#;
        assert!(points_to(spelled_differently, &exe()));
    }

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("autolad-claude-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn locations(root: &Path) -> Locations {
        Locations {
            exe: exe(),
            app_data: Some(root.join("Roaming")),
            local_app_data: Some(root.join("Local")),
            home: Some(root.join("home")),
            claude_cli: None,
        }
    }

    #[tokio::test]
    async fn desktop_is_connected_in_every_install_found() {
        let root = scratch("desktop");
        let at = locations(&root);
        assert!(!status(&at).desktop.installed);
        assert!(connect(ClaudeApp::Desktop, &at).await.is_err());

        let classic = root.join("Roaming/Claude");
        let store = root.join("Local/Packages/Claude_pzs8sxrjxfjjc/LocalCache/Roaming/Claude");
        std::fs::create_dir_all(&classic).unwrap();
        std::fs::create_dir_all(&store).unwrap();
        std::fs::write(classic.join(DESKTOP_CONFIG), r#"{"keep":1}"#).unwrap();
        let before = status(&at);
        assert!(before.desktop.installed && !before.desktop.connected);

        let after = connect(ClaudeApp::Desktop, &at).await.unwrap();
        assert!(after.desktop.connected);
        let classic_json = std::fs::read_to_string(classic.join(DESKTOP_CONFIG)).unwrap();
        assert!(classic_json.contains("\"keep\": 1"));
        assert!(store.join(DESKTOP_CONFIG).is_file());
    }

    #[tokio::test]
    async fn code_without_its_cli_explains_the_manual_command() {
        let root = scratch("code");
        let at = locations(&root);
        let setup = status(&at);
        assert!(!setup.code.installed && !setup.code.connected);
        assert!(setup
            .code_command
            .contains("claude mcp add --scope user autolad"));
        let err = connect(ClaudeApp::Code, &at).await.unwrap_err();
        assert!(err.to_string().contains("claude mcp add"));

        // Its user config is read to tell whether AutoLad is registered.
        std::fs::create_dir_all(root.join("home")).unwrap();
        let config = with_server(r#"{"projects":{}}"#, &exe()).unwrap();
        std::fs::write(root.join("home/.claude.json"), config).unwrap();
        assert!(status(&at).code.connected);
    }

    #[test]
    fn the_cli_is_found_on_the_path_or_in_the_native_install_folder() {
        let root = scratch("cli");
        assert_eq!(find_claude_cli(None, None), None);
        let npm = root.join("npm");
        std::fs::create_dir_all(&npm).unwrap();
        std::fs::write(npm.join("claude.cmd"), "").unwrap();
        let path = std::env::join_paths([root.join("empty"), npm.clone()]).unwrap();
        assert_eq!(
            find_claude_cli(Some(path), None),
            Some(npm.join("claude.cmd"))
        );

        let native = root.join("home/.local/bin");
        std::fs::create_dir_all(&native).unwrap();
        std::fs::write(native.join("claude.exe"), "").unwrap();
        assert_eq!(
            find_claude_cli(None, Some(&root.join("home"))),
            Some(native.join("claude.exe"))
        );
    }
}
