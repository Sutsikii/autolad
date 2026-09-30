/// Health check used to validate the IPC bridge end to end.
#[tauri::command]
#[specta::specta]
pub fn ping() -> String {
    "pong".to_owned()
}

#[cfg(test)]
mod tests {
    #[test]
    fn ping_answers_pong() {
        assert_eq!(super::ping(), "pong");
    }
}
