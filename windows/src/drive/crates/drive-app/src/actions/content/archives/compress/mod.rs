//! Port de `Files.App/Actions/Content/Archives/Compress/`.

pub mod base_compress_archive_action;
pub mod compress_into_archive_action;
pub mod compress_into_seven_zip_action;
pub mod compress_into_zip_action;

pub use compress_into_archive_action::CompressIntoArchive;
pub use compress_into_seven_zip_action::CompressIntoSevenZip;
pub use compress_into_zip_action::CompressIntoZip;
