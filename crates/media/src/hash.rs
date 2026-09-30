//! Content hash used to identify assets and key caches.
//! Hashing multi-GB rushes fully would make import slow, so only the size and the
//! first/last MiB are hashed: enough to tell files apart in practice.

use std::path::Path;

use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::error::MediaError;

const CHUNK: u64 = 1 << 20;

/// Returns the blake3 hex digest (64 chars) of `size + head + tail`.
pub async fn hash_file(path: &Path) -> Result<String, MediaError> {
    let io = |e: std::io::Error| MediaError::Io(format!("{}: {e}", path.display()));

    let mut file = tokio::fs::File::open(path).await.map_err(io)?;
    let len = file.metadata().await.map_err(io)?.len();

    let mut head = vec![0u8; len.min(CHUNK) as usize];
    file.read_exact(&mut head).await.map_err(io)?;

    // The tail starts after the head so no byte is hashed twice on small files.
    let tail_start = len.saturating_sub(CHUNK).max(CHUNK.min(len));
    let mut tail = vec![0u8; (len - tail_start) as usize];
    if !tail.is_empty() {
        file.seek(std::io::SeekFrom::Start(tail_start))
            .await
            .map_err(io)?;
        file.read_exact(&mut tail).await.map_err(io)?;
    }

    let mut hasher = blake3::Hasher::new();
    hasher.update(&len.to_le_bytes());
    hasher.update(&head);
    hasher.update(&tail);
    Ok(hasher.finalize().to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("autolad-hash-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[tokio::test]
    async fn same_content_same_hash_different_content_different_hash() {
        let a = hash_file(&write("a.bin", b"hello")).await.unwrap();
        let a2 = hash_file(&write("a2.bin", b"hello")).await.unwrap();
        let b = hash_file(&write("b.bin", b"hellO")).await.unwrap();
        assert_eq!(a, a2);
        assert_ne!(a, b);
        assert_eq!(a.len(), 64);
    }

    #[tokio::test]
    async fn empty_file_is_hashable() {
        assert_eq!(hash_file(&write("e.bin", b"")).await.unwrap().len(), 64);
    }

    #[tokio::test]
    async fn large_file_hash_sees_head_and_tail_but_not_the_middle() {
        let size = 5 * CHUNK as usize;
        let base = vec![7u8; size];

        let mut middle = base.clone();
        middle[size / 2] = 9;
        let mut tail = base.clone();
        tail[size - 1] = 9;

        let h_base = hash_file(&write("l0.bin", &base)).await.unwrap();
        let h_middle = hash_file(&write("l1.bin", &middle)).await.unwrap();
        let h_tail = hash_file(&write("l2.bin", &tail)).await.unwrap();
        assert_eq!(
            h_base, h_middle,
            "middle bytes are intentionally not hashed"
        );
        assert_ne!(h_base, h_tail);
    }

    #[tokio::test]
    async fn missing_file_is_an_io_error() {
        let err = hash_file(Path::new("definitely-missing.bin"))
            .await
            .unwrap_err();
        assert!(matches!(err, MediaError::Io(_)));
    }
}
