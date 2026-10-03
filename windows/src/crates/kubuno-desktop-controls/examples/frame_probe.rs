//! Prints what the host actually hands a paint callback, and what the window
//! actually is, so a disagreement between the two is a measurement instead of
//! an inference.
//!
//! The gallery's layout was packing more groups per row than its window could
//! hold, which points at `Frame.size` and the painter's scale disagreeing. That
//! is exactly the kind of claim worth checking directly: run this, compare the
//! three lines, and the culprit names itself.
//!
//!   cargo run -p kubuno-desktop-controls --example frame_probe

use kubuno_drive_desktop_app_controls::Theme;
use kubuno_desktop_controls::host::{self, Frame};

fn main() -> windows::core::Result<()> {
    let mut printed = 0u32;
    host::run("frame probe", 1500, 1000, Theme::light(), move |c, f: &Frame| {
        // A few frames, not one: the first paint can precede the DPI settling,
        // and a difference between frame 1 and frame 3 is itself the finding.
        if printed < 3 {
            printed += 1;
            eprintln!(
                "[frame {printed}] size = {:.1} x {:.1} DIP | frame.scale = {:.3} | canvas.scale = {:.3} | implied px = {:.0} x {:.0}",
                f.size.0,
                f.size.1,
                f.scale,
                c.scale(),
                f.size.0 * f.scale,
                f.size.1 * f.scale,
            );
            let v = c.visuals();
            eprintln!(
                "         font = {:?} {} pt | scrollbar = {} DIP",
                v.fonts.family, v.fonts.point_size, v.metrics.vertical_scroll_width
            );
        }
    })
}
