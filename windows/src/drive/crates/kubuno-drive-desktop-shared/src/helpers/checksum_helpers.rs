//! Port of `Files.Shared/Helpers/ChecksumHelpers.cs`.
//!
//! Hex casing mirrors the C# implementation: CRC32 and path checksums are
//! upper-case, the cryptographic hashes are lower-case.

use md5::{Digest, Md5};
use sha1::Sha1;
use sha2::{Sha256, Sha384, Sha512};
use tokio::io::{AsyncRead, AsyncReadExt};

const BUFFER_SIZE: usize = 64 * 1024;

/// MD5 checksum of a path string, upper-case hex.
pub fn calculate_checksum_for_path(path: &str) -> String {
    let hash = Md5::digest(path.as_bytes());
    hex::encode_upper(hash)
}

/// CRC32 of a stream, byte-reversed, upper-case hex (mirrors the C# byte reversal).
pub async fn create_crc32<R: AsyncRead + Unpin>(stream: &mut R) -> std::io::Result<String> {
    let mut hasher = crc32fast::Hasher::new();
    let mut buf = vec![0u8; BUFFER_SIZE];
    loop {
        let n = stream.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    // C# reverses GetCurrentHash() (little-endian) — equivalent to big-endian bytes.
    Ok(hex::encode_upper(hasher.finalize().to_be_bytes()))
}

macro_rules! hash_stream_fn {
    ($name:ident, $hasher:ty) => {
        pub async fn $name<R: AsyncRead + Unpin>(stream: &mut R) -> std::io::Result<String> {
            let mut hasher = <$hasher>::new();
            let mut buf = vec![0u8; BUFFER_SIZE];
            loop {
                let n = stream.read(&mut buf).await?;
                if n == 0 {
                    break;
                }
                hasher.update(&buf[..n]);
            }
            Ok(hex::encode(hasher.finalize()))
        }
    };
}

hash_stream_fn!(create_md5, Md5);
hash_stream_fn!(create_sha1, Sha1);
hash_stream_fn!(create_sha256, Sha256);
hash_stream_fn!(create_sha384, Sha384);
hash_stream_fn!(create_sha512, Sha512);

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn known_vectors() {
        // Matches .NET: MD5("") = D41D8CD98F00B204E9800998ECF8427E
        assert_eq!(calculate_checksum_for_path(""), "D41D8CD98F00B204E9800998ECF8427E");

        let mut data: &[u8] = b"hello";
        assert_eq!(create_md5(&mut data).await.unwrap(), "5d41402abc4b2a76b9719d911017c592");

        let mut data: &[u8] = b"hello";
        // CRC32("hello") = 0x3610A686
        assert_eq!(create_crc32(&mut data).await.unwrap(), "3610A686");

        let mut data: &[u8] = b"hello";
        assert_eq!(
            create_sha256(&mut data).await.unwrap(),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }
}
