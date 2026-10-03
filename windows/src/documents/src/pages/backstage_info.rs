//! Code-behind of the user control `BackstageInfo` (`backstage_info.kbcontrol`): the « Informations »
//! tab of the Backstage. It has no state of its own: the form binds its four properties to what the
//! page canvas reports (`DocumentWindow::page_view_changed`).

use kubuno::views::prelude::*;

/// « Fichier › Informations » (see the module doc).
#[derive(UserControl, Default)]
#[user_control(view = "backstage_info.kbcontrol")]
#[category("Documents")]
pub struct BackstageInfo {
    base: UserControlCore,
    /// The document's title (its file name).
    #[property(bindable)]
    #[category("Data")]
    pub document_title: String,
    /// How many pages it has.
    #[property(bindable)]
    #[category("Data")]
    pub pages: String,
    /// The zoom (« 100 % »).
    #[property(bindable)]
    #[category("Data")]
    pub zoom_text: String,
    /// What the stored document holds that is not drawn (« rien — tout le contenu est à l'écran »).
    #[property(bindable)]
    #[category("Data")]
    pub hidden: String,
}
