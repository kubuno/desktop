//! Custom controls reached from a form's code: `#[control]` fields typed with their class
//! (`Custom<T>`), `Control::with::<T>`, and list / Rust-value properties (`Rows`, `Shared<T>`)
//! bound from the form.

use kubuno_desktop::prelude::*;
use kubuno_desktop::ui::graphics::testing::RecordingCanvas;
use kubuno_desktop::views::component::ControlCore;
use kubuno_desktop::views::runtime::Runtime;

/// A control of the application with a list property and a Rust-value property.
#[derive(kubuno_desktop::views::component::Component, Default)]
#[kubuno(extends = Control)]
pub struct Thread {
    base: ControlCore,
    /// The messages (a list).
    #[property]
    pub messages: Rows,
    /// The author's settings (any Rust value).
    #[property]
    pub settings: Shared<Settings>,
    /// Counted by code through `with`.
    pub appended: u32,
}

#[derive(Debug, Default, PartialEq)]
pub struct Settings {
    pub compact: bool,
}

impl Thread {
    pub fn append(&mut self) {
        self.appended += 1;
    }
}

#[kubuno_desktop::view(xml = r#"
<Panel DesignWidth="300" DesignHeight="200" OnLoad="loaded">
  <Thread x:Name="thread" Messages="{Binding Messages}" Settings="{Binding Settings}" X="0" Y="0" Width="300" Height="200"/>
</Panel>"#)]
pub struct Chat {
    #[control]
    thread: Custom<Thread>,
    #[bind]
    messages: Rows,
    #[bind]
    settings: Shared<Settings>,
    seen: Vec<String>,
}

impl Chat {
    fn new() -> Self {
        let mut v = Self::default();
        v.initialize_component();
        v
    }

    fn loaded(&mut self) {
        // Typed access to the control's instance, from the form's code.
        let appended = self.thread.with(|t| {
            t.append();
            t.appended
        });
        self.seen.push(format!("appended={appended:?}"));
        let count = self.thread.as_control().with_ref(|t: &Thread| t.messages.len());
        self.seen.push(format!("messages={count:?}"));
        let compact = self.thread.with_ref(|t| t.settings.compact);
        self.seen.push(format!("compact={compact:?}"));
    }
}

fn frame() -> kubuno_desktop::controls::host::Frame {
    use kubuno_desktop::controls::host;
    host::Frame {
        size: (300.0, 200.0),
        mouse: (host::POINTER_AWAY, host::POINTER_AWAY),
        mouse_down: false,
        right_down: false,
        middle_down: false,
        dismiss: false,
        scale: 1.0,
        client_origin: (0.0, 0.0),
        work_area: (0.0, 0.0, 300.0, 200.0),
        chrome_top: 0.0,
        mods: host::Modifiers::NONE,
        wheel: (0.0, 0.0),
        click_count: 0,
        window_focused: true,
    }
}

#[test]
fn a_typed_custom_control_field_reaches_its_instance_and_its_list_and_object_properties() {
    let mut view = Chat::new();
    view.messages = vec![Row::new().with("Text", Value::Str("Salut".into())), Row::new().with("Text", Value::Str("Ça va ?".into()))].into();
    view.settings = Shared::new(Settings { compact: true });
    assert_eq!(view.thread.get_name(), "thread");
    let text = kubuno_desktop::__private::compose_text(view.form());
    let mut runtime = Runtime::new();
    assert!(runtime.reload_from_text(&text), "{:?}", runtime.diagnostics());
    let bounds = kubuno_desktop::ui::Rect::new(0.0, 0.0, 300.0, 200.0);
    for _ in 0..2 {
        let canvas = RecordingCanvas::new();
        runtime.frame_model(&canvas, &frame(), &mut view, bounds);
    }
    assert_eq!(runtime.with_component::<Thread, _>("thread", |t| (t.messages.len(), t.settings.compact, t.appended)), Some((2, true, 1)));
    assert!(view.seen.contains(&"appended=Some(1)".to_string()), "{:?}", view.seen);
}

#[kubuno_desktop::view(xml = r#"
<Panel DesignWidth="900" DesignHeight="600">
  <Tabs x:Name="pages" Dock="Fill" SelectedIndex="0">
    <TabItem Header="Cartes">
      <Stack Direction="TopDown" Gap="12" Padding="16">
        <Label Text="Étiquettes" Role="Heading" Height="24"/>
        <Stack x:Name="tags" Direction="LeftToRight" WrapContents="true" CrossAlign="Center" Gap="8" Height="72">
          <Badge Text="Rust"/>
          <Badge Text="Kubuno"/>
        </Stack>
        <Stack Direction="LeftToRight" CrossAlign="Center" Gap="8" Height="40">
          <SearchField x:Name="search" Placeholder="Rechercher" Stack.Fill="true"/>
          <Button x:Name="add_card" Text="Ajouter" Variant="Secondary"/>
        </Stack>
      </Stack>
    </TabItem>
  </Tabs>
</Panel>"#)]
pub struct Cards {}

#[test]
fn a_named_wrapping_stack_shows_its_children_through_the_form() {
    let mut view = Cards::default();
    view.initialize_component();
    let text = kubuno_desktop::__private::compose_text(view.form());
    let mut runtime = Runtime::new();
    assert!(runtime.reload_from_text(&text), "{:?}", runtime.diagnostics());
    let canvas = RecordingCanvas::new();
    runtime.frame_model(&canvas, &frame(), &mut view, kubuno_desktop::ui::Rect::new(0.0, 0.0, 900.0, 600.0));
    let texts: Vec<String> = canvas.calls().into_iter().filter(|c| c.starts_with("text(")).collect();
    assert!(texts.iter().any(|t| t.contains("Kubuno")) && texts.iter().any(|t| t.contains("Ajouter")), "{text}\n{texts:?}");
}
