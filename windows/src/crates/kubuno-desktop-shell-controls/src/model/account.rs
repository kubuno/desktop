//! What the account panel ([`crate::AccountMenu`]) shows, and the host's [`AccountService`] it comes
//! from (vskubuno `docs/SHELL-CONTROLS.md` §2: the same shapes as the web's `AccountMenu`).

/// The active account.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AccountUser {
    /// The display name (« Camille Martin »): the greeting uses its first word.
    pub name: String,
    pub email: String,
    /// What the avatar shows without a photo (« CM »).
    pub initials: String,
    /// The profile photo, cached on disk; `None` keeps the initials.
    pub avatar: Option<String>,
}

/// Another account the panel lists.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AccountEntry {
    /// What `OpenAccount` / `RemoveAccount` report.
    pub id: String,
    /// The account's name (or its instance's label).
    pub name: String,
    pub email: String,
    /// The instance's host (`kubuno.asso-exemple.org`), shown under the name.
    pub server: String,
    pub initials: Option<String>,
    pub avatar: Option<String>,
    /// A usable session; `false` shows « Déconnecté » with « Connexion » (raises `OpenAccount`) and
    /// « Supprimer » (`RemoveAccount`).
    pub connected: bool,
    /// An account of ANOTHER Kubuno instance: its server pill, « Ouvrir » (`OpenAccount`) and
    /// « Supprimer » (`RemoveAccount`) instead of a one-click switch.
    pub remote: bool,
    /// That account's unread notifications (a badge on its row; none for 0).
    pub unread: u32,
}

/// What the user picked in the account panel: its events, as the [`crate::AccountButton`] hands
/// them to its service ([`AccountService::act`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccountAction {
    ManageAccount,
    OpenAccount(String),
    RemoveAccount(String),
    AddAccount,
    OpenLabels,
    OpenAdmin,
    SignOut,
    ChangeAvatar,
}

/// The account panel's data, provided by the app that places an [`crate::AccountMenu`]
/// ([`crate::AccountMenu::set_service`]) or an [`crate::AccountButton`]. Called on the UI thread: it
/// hands back what it holds.
pub trait AccountService {
    /// The active account.
    fn user(&self) -> AccountUser;
    /// The other accounts, in the order to list them (the active one excluded).
    fn accounts(&self) -> Vec<AccountEntry>;
    /// Whether « Administration » is listed (the account may enter the console).
    fn can_administer(&self) -> bool;
    /// What the user picked in an [`crate::AccountButton`]'s panel (an [`crate::AccountMenu`]
    /// placed in a view raises events instead). Nothing by default.
    fn act(&self, action: AccountAction) {
        let _ = action;
    }
}

/// « Bonjour Camille ! »: the first name only, as the web greets; `template` holds `{0}`.
pub fn greeting(template: &str, name: &str) -> String {
    let first = name.split_whitespace().next().unwrap_or(name);
    template.replace("{0}", first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_greeting_takes_the_first_name() {
        assert_eq!(greeting("Bonjour {0} !", "Camille Martin"), "Bonjour Camille !");
        assert_eq!(greeting("Hello, {0}!", "camille"), "Hello, camille!");
    }
}
