//! BranchItem (mirrors BranchItem.cs)
//!
//! Port of `Files.App/Data/Items/BranchItem.cs` (`public record BranchItem`).

/// Port of `Files.App/Data/Items/BranchItem.cs`.
#[derive(Debug, Clone)]
pub struct BranchItem {
    pub name: String,
    pub is_head: bool,
    pub is_remote: bool,
    pub ahead_by: usize,
    pub behind_by: usize,
}
