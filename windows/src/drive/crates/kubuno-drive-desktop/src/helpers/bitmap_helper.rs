//! Port of `Files.App/Helpers/BitmapHelper.cs` (rotation part).
//!
//! Reimplements `RotateAsync`: open the file for read/write, decode with
//! `BitmapDecoder`, re-encode via transcoding (`CreateForTranscoding
//! Async`) into a memory stream applying the rotation to each frame, then
//! write the memory stream back over the file. Transcoding preserves
//! metadata, like the original. WinRT calls are blocked via `.join()` (the
//! counterpart to `await` here, cf. `utils/share.rs`).

use windows::core::{Interface, Result, HSTRING};
use windows::Graphics::Imaging::{BitmapDecoder, BitmapEncoder, BitmapRotation};
use windows::Storage::Streams::{InMemoryRandomAccessStream, RandomAccessStream};
use windows::Storage::{FileAccessMode, StorageFile};

/// Rotates the image at the given path in place (`BitmapHelper.RotateAsync`).
pub fn rotate(path: &str, rotation: BitmapRotation) -> Result<()> {
    if path.is_empty() {
        return Ok(());
    }
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path))?.join()?;
    let file_stream = file.OpenAsync(FileAccessMode::ReadWrite)?.join()?;

    let decoder = BitmapDecoder::CreateAsync(&file_stream)?.join()?;
    let mem_stream = InMemoryRandomAccessStream::new()?;
    let encoder = BitmapEncoder::CreateForTranscodingAsync(&mem_stream, &decoder)?.join()?;

    // Apply the rotation to every frame (including multi-frame GIF/TIFF).
    let transform = encoder.BitmapTransform()?;
    let frame_count = decoder.FrameCount()?;
    for _ in 0..frame_count.saturating_sub(1) {
        transform.SetRotation(rotation)?;
        encoder.GoToNextFrameAsync()?.join()?;
    }
    transform.SetRotation(rotation)?;
    encoder.FlushAsync()?.join()?;

    // Write the memory stream back over the file (truncated to 0 first).
    mem_stream.Seek(0)?;
    file_stream.Seek(0)?;
    file_stream.SetSize(0)?;
    RandomAccessStream::CopyAsync(
        &mem_stream.cast::<windows::Storage::Streams::IInputStream>()?,
        &file_stream.cast::<windows::Storage::Streams::IOutputStream>()?,
    )?
    .join()?;
    Ok(())
}
