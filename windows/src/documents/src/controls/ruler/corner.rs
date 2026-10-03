//! `RulerCorner` — the 20 × 20 DIP square where the two rulers meet (the web's `CornerCell`): it
//! shows the kind of tab stop a click on the horizontal ruler adds (⌞ ⊥ ⌟ ⊿ │), and each click
//! moves to the next kind. It blends with the rulers: the same grey, a border on its right and
//! bottom edges, a lighter grey under the pointer.

use kubuno::prelude::*;
use kubuno::ui::graphics::{Brush, StringFormat};
use kubuno::ui::{Canvas, Rect, Size};
use kubuno::views::component::{Control, ControlCore, EventCx, PaintEventCx};
use kubuno::views::events::{ChangeSource, Event};

use super::{rule, RulerColors, TabKind, RULER_SZ};

/// The tab-type selector between the rulers (see the module doc).
#[derive(kubuno::views::component::Component)]
#[kubuno(extends = Control, overrides(Control))]
#[category("Documents")]
#[toolbox(icon = "square-dashed")]
#[default_event("TabTypeChanged")]
pub struct RulerCorner {
    base: ControlCore,
    /// The kind of tab stop selected: `left`, `center`, `right`, `decimal` or `bar`.
    #[property(bindable)]
    #[category("Behavior")]
    pub tab_type: String,
    /// Occurs when a click selects the next kind (`e.new`: its name).
    #[event]
    #[category("Action")]
    pub tab_type_changed: Event<TextChangedEventArgs>,
}

impl Default for RulerCorner {
    fn default() -> Self {
        Self { base: ControlCore::default(), tab_type: "left".to_string(), tab_type_changed: Event::default() }
    }
}

impl Control for RulerCorner {
    fn get_preferred_size(&self, _canvas: &dyn Canvas, _proposed: Size) -> Size {
        Size { width: RULER_SZ, height: RULER_SZ }
    }

    fn on_paint_background(&mut self, e: &mut PaintEventCx<'_>) {
        let b = e.clip_rectangle;
        let g = e.graphics;
        let col = RulerColors::of(&g.theme_colors());
        let fill = if e.state.hot { col.hover } else { col.band };
        rule(g, fill, Rect::new(b.left, b.top, b.right - 1.0, b.bottom - 1.0));
        rule(g, col.border, Rect::new(b.right - 1.0, b.top, b.right, b.bottom));
        rule(g, col.border, Rect::new(b.left, b.bottom - 1.0, b.right - 1.0, b.bottom));
    }

    fn on_paint(&mut self, e: &mut PaintEventCx<'_>) {
        let b = e.clip_rectangle;
        let font = kubuno::ui::graphics::Font::with_dip("Arial", 13.0, kubuno::ui::graphics::FontStyle::REGULAR);
        let symbol = TabKind::parse(&self.tab_type).symbol();
        let ink = RulerColors::of(&e.graphics.theme_colors()).tab_ink;
        e.graphics.draw_string(symbol, &font, Brush::solid(ink), Rect::new(b.left, b.top, b.right - 1.0, b.bottom - 1.0), &StringFormat::centered());
        e.raise(self, "OnPaint");
    }

    fn tool_tip_at(&self, _x: f32, _y: f32) -> Option<String> {
        Some(TabKind::parse(&self.tab_type).label())
    }

    fn on_click(&mut self, e: &mut EventCx<'_, MouseEventArgs>) {
        let old = TabKind::parse(&self.tab_type);
        let new = old.next();
        self.tab_type = new.key().to_string();
        self.raise_tab_type_changed(TextChangedEventArgs::new(old.key().to_string(), new.key().to_string(), ChangeSource::User));
        self.invalidate();
        e.raise(&*self, "OnClick");
    }
}
