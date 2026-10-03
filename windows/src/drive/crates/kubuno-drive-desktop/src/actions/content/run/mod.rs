//! Port of `Files.App/Actions/Content/Run/`: elevation shell verbs.

pub mod base_run_as_action;
pub mod run_as_admin_action;
pub mod run_as_another_user_action;
pub mod run_with_powershell_action;

pub use run_as_admin_action::RunAsAdmin;
pub use run_as_another_user_action::RunAsAnotherUser;
pub use run_with_powershell_action::RunWithPowershell;
