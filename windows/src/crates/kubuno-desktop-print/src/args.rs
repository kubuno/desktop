//! The args of the printing events (WinForms `PrintEventArgs`, `QueryPageSettingsEventArgs`,
//! `PrintPageEventArgs`).

use kubuno_desktop_ui::graphics::{Graphics, GraphicsSlot};
use kubuno_desktop_ui::Rect;
use kubuno_desktop_views::events::{CancelEventArgs, EventArgs};

use crate::settings::{PageSettings, PrintAction};

/// `BeginPrint` / `EndPrint` args (WinForms `PrintEventArgs`): what the job does, and `cancel` (in
/// `BeginPrint`: nothing is printed).
#[derive(EventArgs, Debug, Clone, Default, PartialEq)]
#[args(extends = CancelEventArgs, cancel)]
pub struct PrintEventArgs {
    pub cancel: bool,
    pub print_action: PrintAction,
}

/// `QueryPageSettings` args (WinForms `QueryPageSettingsEventArgs`): the settings of the page about
/// to be printed — change them for this page only (orientation, paper, margins). `cancel` ends the
/// job.
#[derive(EventArgs, Debug, Clone, Default, PartialEq)]
#[args(extends = CancelEventArgs, cancel)]
pub struct QueryPageSettingsEventArgs {
    pub cancel: bool,
    pub print_action: PrintAction,
    pub page_settings: PageSettings,
    /// The page's number, from 1 (a Kubuno addition).
    pub page_number: u32,
}

/// `PrintPage` args (WinForms `PrintPageEventArgs`): the page's surface ([`Self::graphics`], the
/// EVT-8 `Graphics`, lent for the event), its bounds, its settings; set `has_more_pages` to be called
/// again for the next page, `cancel` to abandon the job.
///
/// Units: the `Graphics` and the bounds are in DIP (1/96 inch) from the physical page's top-left
/// corner (from the margins' corner with `OriginAtMargins`); `page_settings` keeps Windows Forms'
/// hundredths of an inch.
///
/// ```
/// use kubuno_desktop_print::PrintPageEventArgs;
/// use kubuno_desktop_ui::graphics::{Color, Font, FontStyle, Graphics, StringFormat};
/// use kubuno_desktop_ui::Rect;
///
/// let g = Graphics::recorder();
/// let page = Rect::new(0.0, 0.0, 816.0, 1056.0);
/// let margins = Rect::new(96.0, 96.0, 720.0, 960.0);
/// let more = PrintPageEventArgs::lend(&g, page, margins, Default::default(), 1, |e| {
///     e.graphics().draw_string("Hello", &Font::new("Segoe UI", 12.0, FontStyle::REGULAR), Color::BLACK, e.margin_bounds, &StringFormat::generic_default());
///     e.has_more_pages = false;
///     e.has_more_pages
/// });
/// assert!(!more);
/// assert_eq!(g.recorded().map(|l| l.len()), Some(1));
/// ```
#[derive(EventArgs, Default)]
#[args(extends = CancelEventArgs, cancel)]
pub struct PrintPageEventArgs {
    pub cancel: bool,
    /// Set by the handler: another page follows.
    pub has_more_pages: bool,
    /// The whole page, DIP.
    pub page_bounds: Rect,
    /// The page inside its margins, DIP.
    pub margin_bounds: Rect,
    pub page_settings: PageSettings,
    /// The page's number, from 1 (a Kubuno addition).
    pub page_number: u32,
    graphics: GraphicsSlot,
}

impl std::fmt::Debug for PrintPageEventArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PrintPageEventArgs")
            .field("page_number", &self.page_number)
            .field("page_bounds", &self.page_bounds)
            .field("margin_bounds", &self.margin_bounds)
            .field("has_more_pages", &self.has_more_pages)
            .field("cancel", &self.cancel)
            .finish()
    }
}

impl PrintPageEventArgs {
    /// Args lending `graphics` to `f` (emptied when `f` returns: a copy kept draws nothing).
    pub fn lend<R>(graphics: &Graphics<'_>, page_bounds: Rect, margin_bounds: Rect, page_settings: PageSettings, page_number: u32, f: impl FnOnce(&mut PrintPageEventArgs) -> R) -> R {
        GraphicsSlot::lend(graphics, |slot| {
            let mut args = PrintPageEventArgs { cancel: false, has_more_pages: false, page_bounds, margin_bounds, page_settings, page_number, graphics: slot };
            f(&mut args)
        })
    }

    /// The page's drawing surface (WinForms `e.Graphics`).
    pub fn graphics(&self) -> &Graphics<'_> {
        self.graphics.get()
    }
}

/// Keeps the args types reachable by name for the tooling (`EventArgs` is implemented).
#[allow(dead_code)]
fn _assert_args() {
    fn is_args<A: EventArgs>() {}
    is_args::<PrintEventArgs>();
    is_args::<QueryPageSettingsEventArgs>();
    is_args::<PrintPageEventArgs>();
}
