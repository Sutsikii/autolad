//! Composition root: wires `core` and its implementations to the Tauri commands.

mod agent;
mod commands;
mod error;
mod state;

use tauri::Manager;
use tauri_specta::{collect_commands, collect_events, Builder};

use state::AppState;

#[cfg(any(debug_assertions, test))]
const BINDINGS_PATH: &str = "../src/ipc/bindings.ts";

/// MCP mode: no window, protocol on stdout, diagnostics on stderr only.
pub fn run_mcp() -> std::process::ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("autolad --mcp: cannot start async runtime: {e}");
            return std::process::ExitCode::FAILURE;
        }
    };

    // Connects to the open app when there is one, else serves a standalone engine.
    let result = runtime.block_on(autolad_mcp::run_stdio());
    match result {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("autolad --mcp: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

// The generated file has unused helpers (events, channels) that strict tsc rejects.
// Sizes and indices stay far below 2^53, so `number` is safe for 64-bit integers.
#[cfg(any(debug_assertions, test))]
fn ts_exporter() -> specta_typescript::Typescript {
    specta_typescript::Typescript::default()
        .header("// @ts-nocheck")
        .bigint(specta_typescript::BigIntExportBehavior::Number)
}

fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .events(collect_events![agent::AgentActivity])
        .commands(collect_commands![
            commands::system::ping,
            commands::automation::build_silence_edl,
            commands::editor::import_media,
            commands::editor::project_status,
            commands::editor::auto_cut,
            commands::editor::edit_edl,
            commands::editor::undo,
            commands::editor::redo,
            commands::editor::preview_frame,
            commands::editor::prepare_proxy,
            commands::editor::prepare_thumbnails,
            commands::editor::prepare_waveform,
            commands::editor::transcribe,
            commands::editor::cached_transcript,
            commands::editor::edit_transcript,
            commands::editor::cut_phrase,
            commands::editor::remove_fillers,
            commands::editor::remove_retakes,
            commands::editor::save_project,
            commands::editor::open_project,
            commands::editor::new_project,
            commands::editor::render_start,
            commands::editor::render_status,
            commands::editor::render_cancel,
            commands::editor::export_subtitles,
            commands::claude::claude_setup,
            commands::claude::connect_claude,
        ])
}

// Startup failures are unrecoverable and Tauri's own templates end with `expect`.
#[allow(clippy::expect_used)]
pub fn run() {
    let builder = specta_builder();

    #[cfg(debug_assertions)]
    builder
        .export(ts_exporter(), BINDINGS_PATH)
        .expect("failed to export TypeScript bindings");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            let state = AppState::new();
            if let Some(engine) = state.shared_engine() {
                agent::start_bridge(app.handle().clone(), engine);
            }
            app.manage(state);
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                app.state::<AppState>().shutdown.cancel();
                agent::stop_bridge();
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Keeps the committed bindings in sync with the Rust signatures.
    /// Regenerate with `UPDATE_BINDINGS=1 cargo test -p autolad bindings`.
    #[test]
    fn bindings_are_up_to_date() {
        if std::env::var_os("UPDATE_BINDINGS").is_some() {
            specta_builder()
                .export(ts_exporter(), BINDINGS_PATH)
                .unwrap();
        }
        let out = std::env::temp_dir().join("autolad-bindings-check.ts");
        specta_builder().export(ts_exporter(), &out).unwrap();
        let generated = std::fs::read_to_string(&out).unwrap();
        let committed = std::fs::read_to_string(BINDINGS_PATH)
            .expect("run `pnpm tauri dev` once to generate src/ipc/bindings.ts");
        assert_eq!(generated, committed, "bindings.ts is stale; regenerate it");
    }
}
