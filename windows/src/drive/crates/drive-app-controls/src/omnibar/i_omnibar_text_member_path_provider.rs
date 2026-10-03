//! `IOmnibarTextMemberPathProvider` (mirror of
//! `Files.App.Controls/Omnibar/IOmnibarTextMemberPathProvider.cs`).

/// Provides the text member path of `OmnibarMode.ItemsSource`.
pub trait IOmnibarTextMemberPathProvider {
    /// Returns the text member path as a string. This path can be used to
    /// identify the location of the text member.
    fn get_text_member_path(&self, text_member_path: &str) -> String;
}
