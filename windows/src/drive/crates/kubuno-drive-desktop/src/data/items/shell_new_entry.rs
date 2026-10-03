//! ShellNewEntry (mirrors ShellNewEntry.cs)
//!
//! Port of `Files.App/Data/Items/ShellNewEntry.cs`.
//!
//! `ShellNewKind` is a UNIFICATION specific to the port of the scattered
//! fields of `ShellNewEntry.cs`: it stays with the entry.

/// The four forms of the `ShellNew` registry key (cf. `ParseShellNewRegistryEntry`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShellNewKind {
    /// `NullFile` value: create an empty file with the desired extension.
    NullFile,
    /// `FileName` value: copy this template (resolved in the Templates folders).
    FileName(String),
    /// `Data` value: write these bytes (binary, or a string converted to UTF-8).
    Data(Vec<u8>),
    /// `Command` value: shell command to run with substitution — not
    /// ported (requires expanding `%1`-style placeholders).
    Command(String),
}

/// Port of `Data/Items/ShellNewEntry.cs`. The original keeps `Extension`,
/// `Name`, `Command`, `IconBase64`, `Data` and `Template`; here the form is
/// unified into `kind` and the icon is the ProgID's `DefaultIcon` reference
/// (the original made a base64 thumbnail via WinRT — out of scope for this
/// module).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellNewEntry {
    /// The extension, dot included (".txt", ".docx"…).
    pub extension: String,
    /// The name shown in the list (shell type, e.g. "Document texte").
    pub display_name: String,
    /// Icon reference (`HKCR\<ProgID>\DefaultIcon`), if present.
    pub icon: Option<String>,
    /// The template's form as read from `ShellNew`.
    pub kind: ShellNewKind,
}
