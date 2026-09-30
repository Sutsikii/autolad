// Prevents an extra console window on Windows in release.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() -> std::process::ExitCode {
    // `autolad.exe --mcp` runs the MCP server on stdio with no window, so a single
    // installed executable serves both the desktop app and AI agents.
    if std::env::args().skip(1).any(|arg| arg == "--mcp") {
        return autolad_lib::run_mcp();
    }
    autolad_lib::run();
    std::process::ExitCode::SUCCESS
}
