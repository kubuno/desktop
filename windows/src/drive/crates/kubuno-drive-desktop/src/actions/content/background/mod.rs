//! Port of `Files.App/Actions/Content/Background/` (`BaseSetAsAction` +
//! `SetAsWallpaper/Lockscreen/Slideshow/AppBackground`).
//!
//! "Set as background": desktop, lock screen, slideshow, or app background
//! (`AppThemeBackgroundImageSource` setting).

pub mod base_set_as_action;
pub mod set_as_app_background_action;
pub mod set_as_lockscreen_background_action;
pub mod set_as_slideshow_background_action;
pub mod set_as_wallpaper_background_action;

pub use set_as_app_background_action::SetAsAppBackground;
pub use set_as_lockscreen_background_action::SetAsLockscreenBackground;
pub use set_as_slideshow_background_action::SetAsSlideshowBackground;
pub use set_as_wallpaper_background_action::SetAsWallpaperBackground;
