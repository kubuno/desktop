//! CloudProvider (mirrors CloudProvider.cs)
//!
//! Port of `Files.App/Utils/Cloud/CloudProvider.cs` (`public sealed class
//! CloudProvider : ICloudProvider`). The Rust model is reduced (name +
//! sync_folder) vs the C# (ID, IconData, Equals/GetHashCode).

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudProvider {
    pub name: String,
    pub sync_folder: String,
}
