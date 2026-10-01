use std::ffi::OsString;
use std::path::PathBuf;

/// Same folder Tauri uses for the app's local data (bundle identifier), so the desktop
/// UI and the MCP mode share downloaded models.
const APP_DIR: &str = "com.autolad.app";

/// Where models and scratch files live. `AUTOLAD_HOME` overrides everything (tests, portable installs).
pub fn data_dir() -> PathBuf {
    resolve_data_dir(std::env::var_os("AUTOLAD_HOME"), local_data_base())
}

/// The OS folder Tauri's `$APPLOCALDATA` sits in: the asset protocol only serves proxies,
/// thumbnails and waveforms from there.
fn local_data_base() -> Option<OsString> {
    if cfg!(target_os = "macos") {
        let home = PathBuf::from(std::env::var_os("HOME")?);
        Some(home.join("Library/Application Support").into_os_string())
    } else {
        std::env::var_os("LOCALAPPDATA")
    }
}

fn resolve_data_dir(home: Option<OsString>, local_app_data: Option<OsString>) -> PathBuf {
    match (home, local_app_data) {
        (Some(home), _) if !home.is_empty() => PathBuf::from(home),
        (_, Some(local)) if !local.is_empty() => PathBuf::from(local).join(APP_DIR),
        _ => std::env::temp_dir().join(APP_DIR),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_home_wins() {
        let dir = resolve_data_dir(Some("D:/x".into()), Some("C:/Local".into()));
        assert_eq!(dir, PathBuf::from("D:/x"));
    }

    #[test]
    fn falls_back_to_local_app_data_then_temp() {
        assert_eq!(
            resolve_data_dir(None, Some("C:/Local".into())),
            PathBuf::from("C:/Local").join(APP_DIR)
        );
        assert_eq!(
            resolve_data_dir(Some("".into()), None),
            std::env::temp_dir().join(APP_DIR)
        );
    }
}
