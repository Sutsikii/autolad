use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use crate::error::TranscribeError;

fn fail(e: impl std::fmt::Display) -> TranscribeError {
    TranscribeError::Download(e.to_string())
}

/// Downloads `url` to `dest`, verifying its SHA-256 before the file appears at `dest`.
///
/// Data goes to `<dest>.part` first, so an interrupted or corrupt download never
/// leaves something that looks like a valid model.
pub async fn download_verified(
    url: &str,
    dest: &Path,
    expected_sha256: &str,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<(), TranscribeError> {
    if let Some(parent) = dest.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(fail)?;
    }
    let part = part_path(dest);

    let result = fetch_to(url, &part, expected_sha256, progress).await;
    if result.is_err() {
        // Best effort: a stale .part is harmless and gets overwritten next time.
        let _ = tokio::fs::remove_file(&part).await;
        return result;
    }
    tokio::fs::rename(&part, dest).await.map_err(fail)
}

async fn fetch_to(
    url: &str,
    part: &Path,
    expected_sha256: &str,
    progress: &(dyn Fn(u64, u64) + Send + Sync),
) -> Result<(), TranscribeError> {
    let mut response = reqwest::get(url)
        .await
        .and_then(|r| r.error_for_status())
        .map_err(fail)?;
    let total = response.content_length().unwrap_or(0);

    let mut file = tokio::fs::File::create(part).await.map_err(fail)?;
    let mut hasher = Sha256::new();
    let mut done = 0u64;
    while let Some(chunk) = response.chunk().await.map_err(fail)? {
        hasher.update(&chunk);
        file.write_all(&chunk).await.map_err(fail)?;
        done += chunk.len() as u64;
        progress(done, total);
    }
    file.flush().await.map_err(fail)?;

    let actual = hex(&hasher.finalize());
    if actual.eq_ignore_ascii_case(expected_sha256) {
        Ok(())
    } else {
        Err(TranscribeError::HashMismatch {
            expected: expected_sha256.to_owned(),
            actual,
        })
    }
}

fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.file_name().unwrap_or_default().to_os_string();
    name.push(".part");
    dest.with_file_name(name)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    /// Serves `body` to every request on a loopback port; no external network needed.
    async fn serve(body: &'static [u8]) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0u8; 1024];
                let _ = socket.read(&mut buf).await;
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = socket.write_all(head.as_bytes()).await;
                let _ = socket.write_all(body).await;
            }
        });
        format!("http://{addr}/model.bin")
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("autolad-dl-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    // sha256("hello world")
    const HELLO_SHA: &str = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";

    #[tokio::test]
    async fn verified_download_lands_at_destination() {
        let url = serve(b"hello world").await;
        let dest = scratch("ok").join("m.bin");
        let calls = std::sync::atomic::AtomicU64::new(0);

        download_verified(&url, &dest, HELLO_SHA, &|done, _| {
            calls.store(done, std::sync::atomic::Ordering::Relaxed);
        })
        .await
        .unwrap();

        assert_eq!(std::fs::read(&dest).unwrap(), b"hello world");
        assert_eq!(calls.load(std::sync::atomic::Ordering::Relaxed), 11);
        assert!(!part_path(&dest).exists());
    }

    #[tokio::test]
    async fn hash_mismatch_leaves_nothing_behind() {
        let url = serve(b"tampered").await;
        let dest = scratch("bad").join("m.bin");

        let err = download_verified(&url, &dest, HELLO_SHA, &|_, _| {})
            .await
            .unwrap_err();

        assert!(matches!(err, TranscribeError::HashMismatch { .. }));
        assert!(!dest.exists());
        assert!(!part_path(&dest).exists());
    }

    #[tokio::test]
    async fn unreachable_server_is_a_download_error() {
        let dest = scratch("down").join("m.bin");
        let err = download_verified("http://127.0.0.1:1/x", &dest, HELLO_SHA, &|_, _| {})
            .await
            .unwrap_err();
        assert!(matches!(err, TranscribeError::Download(_)));
    }
}
