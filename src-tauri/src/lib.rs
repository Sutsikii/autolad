//! Composition root: wires `core` and its implementations to the Tauri commands.

mod commands;
mod error;
mod state;

use tauri::Manager;
use tauri_specta::{collect_commands, Builder};

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

    let result = runtime.block_on(async {
        let engine = autolad_mcp::Engine::discover()?;
        autolad_mcp::serve_stdio(engine).await
    });
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
    Builder::<tauri::Wry>::new().commands(collect_commands![
        commands::system::ping,
        commands::automation::build_silence_edl,
        commands::editor::import_media,
        commands::editor::project_status,
        commands::editor::auto_cut,
        commands::editor::edit_edl,
        commands::editor::preview_frame,
        commands::editor::prepare_proxy,
        commands::editor::prepare_thumbnails,
        commands::editor::prepare_waveform,
        commands::editor::render_start,
        commands::editor::render_status,
        commands::editor::render_cancel,
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
        .invoke_handler(builder.invoke_handler())
        .setup(|app| {
            app.manage(AppState::new());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                app.state::<AppState>().shutdown.cancel();
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
