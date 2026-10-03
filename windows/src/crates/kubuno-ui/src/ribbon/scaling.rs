//! Adaptive layout of a ribbon tab (`vskubuno/docs/RIBBON.md` §5): the four size levels of a
//! group ([`GroupSize`]), how its controls look at each level ([`SizeDefinition`]: `Auto`, a
//! Windows Ribbon Framework template, or an explicit per-level definition), and the order in which
//! a tab shrinks its groups when it does not fit ([`ScalingPolicy`]).
//!
//! Everything here is pure (the model in, the model out) and unit-tested; the ribbon measures the
//! result with its ordinary layout functions. The default — no policy, every group `Auto` — is
//! exactly the web port's behaviour: groups stay `Large`, and the right-most ones fold, one by one,
//! into their chip ([`plan`] reproduces that loop, see its tests).

use std::borrow::Cow;

use super::{GalleryDisplay, ItemKind, ItemSize, RibbonGroup, RibbonItem};

/// A group's size level, from the ideal (`Large`) to the folded chip (`Collapsed`). Ordered: a
/// larger value is a smaller group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub enum GroupSize {
    /// The ideal look: the controls as declared.
    #[default]
    Large,
    /// Small controls lose their labels (their icon stays, the label becomes the tooltip).
    Medium,
    /// Large buttons become small ones (their label kept); small ones show their icon only.
    Small,
    /// The group is a chip that opens the whole group in a popover.
    Collapsed,
}

impl GroupSize {
    /// Every level, largest first.
    pub const ALL: [GroupSize; 4] = [GroupSize::Large, GroupSize::Medium, GroupSize::Small, GroupSize::Collapsed];

    /// The `.kbview` spelling (`"Large"`, `"Medium"`, `"Small"`, `"Collapsed"`).
    pub fn name(self) -> &'static str {
        match self {
            GroupSize::Large => "Large",
            GroupSize::Medium => "Medium",
            GroupSize::Small => "Small",
            GroupSize::Collapsed => "Collapsed",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.name().eq_ignore_ascii_case(text.trim()))
    }
}

/// How a control's image shows at one level of an explicit definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageSize {
    /// A large button (icon over its label).
    Large,
    /// A small control.
    Small,
    /// Not shown at this level.
    Hidden,
}

impl ImageSize {
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim() {
            "Large" => Some(ImageSize::Large),
            "Small" => Some(ImageSize::Small),
            "Hidden" | "Collapsed" => Some(ImageSize::Hidden),
            _ => None,
        }
    }
}

/// `<ControlSize Control="bold" ImageSize="Small" IsLabelVisible="false" Width="54"/>`: one control
/// of an explicit level definition. A `<Row/>` (or `ColumnBreak="true"`) before a control starts a
/// new column with it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ControlSize {
    /// The control's id (its `x:Name`).
    pub control: String,
    pub image: Option<ImageSize>,
    pub label: Option<bool>,
    pub width: Option<f32>,
    pub column_break: bool,
}

/// `<GroupSizeDefinition Size="Medium">…</GroupSizeDefinition>`.
#[derive(Debug, Clone, PartialEq)]
pub struct LevelDefinition {
    pub size: GroupSize,
    pub controls: Vec<ControlSize>,
}

/// The Windows Ribbon Framework size templates Kubuno implements (`RIBBON.md` §5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Template {
    OneButton,
    TwoButtons,
    ThreeButtons,
    ThreeButtonsOneBigAndTwoSmall,
    ThreeButtonsAndOneCheckBox,
    /// Four to eleven buttons: two large, the rest small with their labels.
    Buttons(u8),
    BigButtonsAndSmallButtonsOrInputs,
    InRibbonGalleryAndBigButton,
    InRibbonGalleryAndThreeButtons,
    ButtonGroups,
    ButtonGroupsAndInputs,
}

const COUNT_NAMES: [&str; 8] = ["FourButtons", "FiveButtons", "SixButtons", "SevenButtons", "EightButtons", "NineButtons", "TenButtons", "ElevenButtons"];

impl Template {
    /// Every template name, in the order the designer offers them.
    pub fn names() -> Vec<&'static str> {
        let mut out = vec!["OneButton", "TwoButtons", "ThreeButtons", "ThreeButtons-OneBigAndTwoSmall", "ThreeButtonsAndOneCheckBox"];
        out.extend(COUNT_NAMES);
        out.extend(["BigButtonsAndSmallButtonsOrInputs", "InRibbonGalleryAndBigButton", "InRibbonGalleryAndThreeButtons", "ButtonGroups", "ButtonGroupsAndInputs"]);
        out
    }

    pub fn name(self) -> &'static str {
        match self {
            Template::OneButton => "OneButton",
            Template::TwoButtons => "TwoButtons",
            Template::ThreeButtons => "ThreeButtons",
            Template::ThreeButtonsOneBigAndTwoSmall => "ThreeButtons-OneBigAndTwoSmall",
            Template::ThreeButtonsAndOneCheckBox => "ThreeButtonsAndOneCheckBox",
            Template::Buttons(n) => COUNT_NAMES.get((n as usize).saturating_sub(4)).copied().unwrap_or("FourButtons"),
            Template::BigButtonsAndSmallButtonsOrInputs => "BigButtonsAndSmallButtonsOrInputs",
            Template::InRibbonGalleryAndBigButton => "InRibbonGalleryAndBigButton",
            Template::InRibbonGalleryAndThreeButtons => "InRibbonGalleryAndThreeButtons",
            Template::ButtonGroups => "ButtonGroups",
            Template::ButtonGroupsAndInputs => "ButtonGroupsAndInputs",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        let t = text.trim();
        Some(match t {
            "OneButton" => Template::OneButton,
            "TwoButtons" => Template::TwoButtons,
            "ThreeButtons" => Template::ThreeButtons,
            "ThreeButtons-OneBigAndTwoSmall" => Template::ThreeButtonsOneBigAndTwoSmall,
            "ThreeButtonsAndOneCheckBox" => Template::ThreeButtonsAndOneCheckBox,
            "BigButtonsAndSmallButtonsOrInputs" => Template::BigButtonsAndSmallButtonsOrInputs,
            "InRibbonGalleryAndBigButton" => Template::InRibbonGalleryAndBigButton,
            "InRibbonGalleryAndThreeButtons" => Template::InRibbonGalleryAndThreeButtons,
            "ButtonGroups" => Template::ButtonGroups,
            "ButtonGroupsAndInputs" => Template::ButtonGroupsAndInputs,
            _ => Template::Buttons(COUNT_NAMES.iter().position(|n| *n == t)? as u8 + 4),
        })
    }

    /// The number of controls the template lays out, when it is fixed.
    pub fn expected_count(self) -> Option<usize> {
        match self {
            Template::OneButton => Some(1),
            Template::TwoButtons => Some(2),
            Template::ThreeButtons | Template::ThreeButtonsOneBigAndTwoSmall => Some(3),
            Template::ThreeButtonsAndOneCheckBox => Some(4),
            Template::Buttons(n) => Some(n as usize),
            Template::InRibbonGalleryAndBigButton => Some(2),
            Template::InRibbonGalleryAndThreeButtons => Some(4),
            _ => None,
        }
    }

    /// Why `items` do not match the template (a count or a kind), `None` when they do — the
    /// language server's and the designer's diagnostic.
    pub fn check(self, items: &[RibbonItem]) -> Option<String> {
        let controls: Vec<&RibbonItem> = items.iter().filter(|i| i.kind != ItemKind::Separator).collect();
        if let Some(n) = self.expected_count() {
            if controls.len() != n {
                return Some(format!("the size template {} lays out {n} control(s), the group has {}", self.name(), controls.len()));
            }
        }
        let gallery_first = matches!(self, Template::InRibbonGalleryAndBigButton | Template::InRibbonGalleryAndThreeButtons);
        if gallery_first && controls.first().is_none_or(|c| c.kind != ItemKind::Gallery) {
            return Some(format!("the size template {} starts with an in-ribbon gallery", self.name()));
        }
        if self == Template::ThreeButtonsAndOneCheckBox && controls.last().is_none_or(|c| c.kind != ItemKind::CheckBox) {
            return Some("the size template ThreeButtonsAndOneCheckBox ends with a check box".to_string());
        }
        None
    }
}

/// `SizeDefinition` of a group: how its controls look at each [`GroupSize`].
#[derive(Debug, Clone, PartialEq, Default)]
pub enum SizeDefinition {
    /// Today's rules (see [`GroupSize`]'s variants).
    #[default]
    Auto,
    Template(Template),
    /// Explicit levels; a level it does not define falls back to `Auto`.
    Custom(Vec<LevelDefinition>),
}

impl SizeDefinition {
    /// Reads the attribute: `Auto`, `Custom`, or a template name (`None` for an unknown one).
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim() {
            "" | "Auto" => Some(SizeDefinition::Auto),
            "Custom" => Some(SizeDefinition::Custom(Vec::new())),
            t => Template::parse(t).map(SizeDefinition::Template),
        }
    }
}

/// `ScalePolicy` step: shrink `group` to `size`.
#[derive(Debug, Clone, PartialEq)]
pub struct ScaleStep {
    pub group: String,
    pub size: GroupSize,
}

/// `<RibbonTab.ScalingPolicy>`: the ideal sizes (groups not named start `Large`) and the ordered
/// steps. An empty policy is `Auto`: right-most group first, straight to `Collapsed`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ScalingPolicy {
    pub ideal: Vec<ScaleStep>,
    pub steps: Vec<ScaleStep>,
}

impl ScalingPolicy {
    pub fn is_auto(&self) -> bool {
        self.ideal.is_empty() && self.steps.is_empty()
    }
}

/// The diagnostics of a policy against the groups of its tab: a step naming an unknown group, and
/// a group that goes back up in size.
pub fn policy_diagnostics(groups: &[&str], policy: &ScalingPolicy) -> Vec<String> {
    let mut out = Vec::new();
    let mut reached: Vec<(String, GroupSize)> = Vec::new();
    for s in policy.ideal.iter().chain(&policy.steps) {
        if !groups.contains(&s.group.as_str()) {
            out.push(format!("the scaling policy names `{}`, which is not a group of this tab", s.group));
        }
    }
    for s in &policy.steps {
        match reached.iter_mut().find(|(g, _)| *g == s.group) {
            Some((_, before)) if s.size < *before => {
                out.push(format!("the scaling policy makes `{}` larger again ({} after {})", s.group, s.size.name(), before.name()));
            }
            Some((_, before)) => *before = s.size,
            None => reached.push((s.group.clone(), s.size)),
        }
    }
    out
}

/// The sizes of the groups of a tab: the ideal sizes, then the policy's steps in order, then (so a
/// tab always ends up fitting) the automatic tail — the right-most group not yet collapsed folds,
/// one by one — until `fits` accepts the sizes. A step never makes a group larger.
pub fn plan(groups: &[RibbonGroup], policy: &ScalingPolicy, fits: impl Fn(&[GroupSize]) -> bool) -> Vec<GroupSize> {
    let mut sizes = vec![GroupSize::Large; groups.len()];
    for s in &policy.ideal {
        if let Some(i) = groups.iter().position(|g| g.id == s.group) {
            sizes[i] = s.size;
        }
    }
    if fits(&sizes) {
        return sizes;
    }
    for s in &policy.steps {
        let Some(i) = groups.iter().position(|g| g.id == s.group) else { continue };
        if s.size > sizes[i] {
            sizes[i] = s.size;
            if fits(&sizes) {
                return sizes;
            }
        }
    }
    for i in (0..groups.len()).rev() {
        if sizes[i] != GroupSize::Collapsed {
            sizes[i] = GroupSize::Collapsed;
            if fits(&sizes) {
                return sizes;
            }
        }
    }
    sizes
}

/// The group's items as they look at `size` — borrowed unchanged when nothing changes (`Large`
/// with the `Auto` definition, the look every ribbon had before size levels existed). A
/// `Collapsed` group is its chip; its popover shows the `Large` look.
pub fn items_at(group: &RibbonGroup, size: GroupSize) -> Cow<'_, [RibbonItem]> {
    let size = if size == GroupSize::Collapsed { GroupSize::Large } else { size };
    match (&group.size, size) {
        (SizeDefinition::Auto, GroupSize::Large) => Cow::Borrowed(&group.items),
        (SizeDefinition::Auto, s) => Cow::Owned(auto(&group.items, s)),
        (SizeDefinition::Template(t), s) => Cow::Owned(template(*t, &group.items, s)),
        (SizeDefinition::Custom(levels), s) => match levels.iter().find(|l| l.size == s) {
            Some(level) => Cow::Owned(custom(&group.items, level)),
            None if s == GroupSize::Large => Cow::Borrowed(&group.items),
            None => Cow::Owned(auto(&group.items, s)),
        },
    }
}

/// The label moves into the tooltip, so the icon is still named.
fn hide_label(it: &mut RibbonItem) {
    if it.icon.is_none() {
        return;
    }
    if let Some(label) = it.label.take() {
        if it.tooltip.is_none() {
            it.tooltip = Some(label);
        }
    }
}

fn narrower_gallery(it: &mut RibbonItem, size: GroupSize) {
    if let Some(g) = it.gallery.as_mut().filter(|g| g.display == GalleryDisplay::InRibbon) {
        match size {
            GroupSize::Medium => g.columns = g.columns.div_ceil(2).max(1),
            GroupSize::Small => g.display = GalleryDisplay::DropDown,
            _ => {}
        }
    }
}

fn auto(items: &[RibbonItem], size: GroupSize) -> Vec<RibbonItem> {
    items
        .iter()
        .map(|it| {
            let mut it = it.clone();
            match size {
                GroupSize::Medium => {
                    if it.size == ItemSize::Small {
                        hide_label(&mut it);
                    }
                }
                GroupSize::Small => {
                    if it.size == ItemSize::Large {
                        it.size = ItemSize::Small;
                    } else {
                        hide_label(&mut it);
                    }
                }
                _ => {}
            }
            narrower_gallery(&mut it, size);
            if matches!(it.kind, ItemKind::ControlGroup | ItemKind::Box) {
                it.children = auto(&it.children, size);
            }
            it
        })
        .collect()
}

/// Whether an item is one of the "buttons" a template counts (not a separator, not hidden).
fn counted(it: &RibbonItem) -> bool {
    it.kind != ItemKind::Separator && it.visible
}

fn template(t: Template, items: &[RibbonItem], size: GroupSize) -> Vec<RibbonItem> {
    let mut out: Vec<RibbonItem> = items.to_vec();
    let slots: Vec<usize> = (0..out.len()).filter(|&i| counted(&out[i])).collect();
    let set = |it: &mut RibbonItem, large: bool, label: bool| {
        if matches!(it.kind, ItemKind::Button | ItemKind::Toggle | ItemKind::Split | ItemKind::Menu | ItemKind::ColorPicker) {
            it.size = if large { ItemSize::Large } else { ItemSize::Small };
        }
        if !label {
            hide_label(it);
        }
    };
    // Levels shared by every template below Large: small with labels (Medium), icons only (Small).
    let generic = |out: &mut Vec<RibbonItem>, size: GroupSize| {
        for &i in &slots {
            match size {
                GroupSize::Medium => set(&mut out[i], false, true),
                GroupSize::Small => set(&mut out[i], false, false),
                _ => {}
            }
            narrower_gallery(&mut out[i], size);
        }
    };
    match (t, size) {
        (_, GroupSize::Collapsed) => {}
        (Template::OneButton, GroupSize::Large | GroupSize::Medium) => slots.iter().for_each(|&i| set(&mut out[i], true, true)),
        (Template::OneButton, _) => slots.iter().for_each(|&i| set(&mut out[i], false, true)),
        (Template::TwoButtons | Template::ThreeButtons, GroupSize::Large) => slots.iter().for_each(|&i| set(&mut out[i], true, true)),
        (Template::ThreeButtonsOneBigAndTwoSmall, GroupSize::Large) => {
            for (n, &i) in slots.iter().enumerate() {
                set(&mut out[i], n == 0, true);
            }
        }
        (Template::ThreeButtonsAndOneCheckBox | Template::Buttons(_), GroupSize::Large) => {
            for (n, &i) in slots.iter().enumerate() {
                set(&mut out[i], n < 2, true);
            }
        }
        (Template::BigButtonsAndSmallButtonsOrInputs | Template::ButtonGroups | Template::ButtonGroupsAndInputs, GroupSize::Large) => {}
        (Template::InRibbonGalleryAndBigButton | Template::InRibbonGalleryAndThreeButtons, GroupSize::Large) => {
            let big = t == Template::InRibbonGalleryAndBigButton;
            for (n, &i) in slots.iter().enumerate().skip(1) {
                set(&mut out[i], big && n == 1, true);
            }
        }
        (Template::BigButtonsAndSmallButtonsOrInputs, s) => {
            for &i in &slots {
                let large = out[i].size == ItemSize::Large;
                if s == GroupSize::Medium {
                    set(&mut out[i], false, true);
                } else {
                    set(&mut out[i], false, false);
                }
                let _ = large;
            }
        }
        (Template::ButtonGroups | Template::ButtonGroupsAndInputs, s) => {
            let inputs = t == Template::ButtonGroupsAndInputs;
            out = auto(&out, s);
            if inputs && s == GroupSize::Small {
                for it in out.iter_mut().filter(|it| matches!(it.kind, ItemKind::ComboBox | ItemKind::Dropdown | ItemKind::TextBox | ItemKind::NumericField)) {
                    it.width = it.width.map(|w| (w * 0.75).round());
                }
            }
        }
        (_, s) => generic(&mut out, s),
    }
    out
}

fn custom(items: &[RibbonItem], level: &LevelDefinition) -> Vec<RibbonItem> {
    let mut out: Vec<RibbonItem> = items.to_vec();
    for cs in &level.controls {
        let Some(it) = out.iter_mut().find(|it| it.id == cs.control) else { continue };
        match cs.image {
            Some(ImageSize::Hidden) => it.visible = false,
            Some(ImageSize::Large) => it.size = ItemSize::Large,
            Some(ImageSize::Small) => it.size = ItemSize::Small,
            None => {}
        }
        if cs.label == Some(false) {
            hide_label(it);
        }
        if cs.width.is_some() {
            it.width = cs.width;
        }
        if cs.column_break {
            it.column_break = true;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn group(id: &str, items: Vec<RibbonItem>) -> RibbonGroup {
        RibbonGroup::new(id, id, items)
    }

    fn b(id: &str) -> RibbonItem {
        RibbonItem::button(id, id, "Copy")
    }

    /// The legacy folding loop of the web port (`layout_row` before size levels): the right-most
    /// `k` groups fold until the rest fits.
    fn legacy_k(natural: &[f32], folded: &[f32], avail: f32) -> usize {
        let n = natural.len();
        let total_for = |k: usize| (0..n).map(|i| if i < n - k { natural[i] } else { folded[i] }).sum::<f32>();
        let mut k = 0;
        while k < n && total_for(k) > avail - 1.0 {
            k += 1;
        }
        k
    }

    #[test]
    fn auto_policy_reproduces_the_legacy_folding_exactly() {
        let groups: Vec<RibbonGroup> = (0..5).map(|i| group(&format!("g{i}"), vec![b("x")])).collect();
        let natural = [120.0, 200.0, 90.0, 150.0, 60.0];
        let folded = [64.0, 64.0, 64.0, 70.0, 64.0];
        for avail in [1000.0, 620.0, 600.0, 500.0, 400.0, 330.0, 200.0, 10.0] {
            let fits = |s: &[GroupSize]| {
                let total: f32 = s.iter().enumerate().map(|(i, s)| if *s == GroupSize::Collapsed { folded[i] } else { natural[i] }).sum();
                total <= avail - 1.0
            };
            let sizes = plan(&groups, &ScalingPolicy::default(), fits);
            let k = sizes.iter().filter(|s| **s == GroupSize::Collapsed).count();
            assert_eq!(k, legacy_k(&natural, &folded, avail), "at {avail}");
            // The folded ones are the right-most, contiguous.
            assert!(sizes.windows(2).all(|w| w[0] <= w[1]), "{sizes:?}");
        }
    }

    #[test]
    fn a_policy_applies_its_steps_in_order_and_never_grows_a_group() {
        let groups = vec![group("clip", vec![b("a")]), group("font", vec![b("b")]), group("styles", vec![b("c")])];
        let policy = ScalingPolicy {
            ideal: vec![],
            steps: vec![
                ScaleStep { group: "styles".into(), size: GroupSize::Medium },
                ScaleStep { group: "font".into(), size: GroupSize::Small },
                ScaleStep { group: "styles".into(), size: GroupSize::Large },
                ScaleStep { group: "styles".into(), size: GroupSize::Collapsed },
            ],
        };
        // Width per level, per group.
        let width = |g: usize, s: GroupSize| -> f32 {
            let base = [100.0, 200.0, 150.0][g];
            match s {
                GroupSize::Large => base,
                GroupSize::Medium => base * 0.8,
                GroupSize::Small => base * 0.6,
                GroupSize::Collapsed => 50.0,
            }
        };
        let run = |avail: f32| plan(&groups, &policy, |s: &[GroupSize]| s.iter().enumerate().map(|(i, s)| width(i, *s)).sum::<f32>() <= avail);
        assert_eq!(run(1000.0), [GroupSize::Large; 3]);
        assert_eq!(run(430.0), [GroupSize::Large, GroupSize::Large, GroupSize::Medium]);
        assert_eq!(run(350.0), [GroupSize::Large, GroupSize::Small, GroupSize::Medium]);
        // The "back to Large" step is skipped; the next one collapses the styles.
        assert_eq!(run(275.0), [GroupSize::Large, GroupSize::Small, GroupSize::Collapsed]);
        // Beyond the policy: the automatic tail folds from the right.
        assert_eq!(run(200.0), [GroupSize::Large, GroupSize::Collapsed, GroupSize::Collapsed]);
    }

    #[test]
    fn ideal_sizes_are_the_starting_point() {
        let groups = vec![group("a", vec![b("x")]), group("b", vec![b("y")])];
        let policy = ScalingPolicy { ideal: vec![ScaleStep { group: "b".into(), size: GroupSize::Medium }], steps: vec![] };
        assert_eq!(plan(&groups, &policy, |_| true), [GroupSize::Large, GroupSize::Medium]);
    }

    #[test]
    fn policy_diagnostics_find_unknown_groups_and_regrowth() {
        let policy = ScalingPolicy {
            ideal: vec![],
            steps: vec![
                ScaleStep { group: "font".into(), size: GroupSize::Small },
                ScaleStep { group: "nope".into(), size: GroupSize::Small },
                ScaleStep { group: "font".into(), size: GroupSize::Medium },
            ],
        };
        let d = policy_diagnostics(&["font", "clip"], &policy);
        assert_eq!(d.len(), 2, "{d:?}");
        assert!(d[0].contains("nope"));
        assert!(d[1].contains("larger again"));
    }

    #[test]
    fn auto_large_is_the_declaration_itself() {
        let g = group("g", vec![b("a"), b("b").large()]);
        assert!(matches!(items_at(&g, GroupSize::Large), Cow::Borrowed(_)));
        assert!(matches!(items_at(&g, GroupSize::Collapsed), Cow::Borrowed(_)));
    }

    #[test]
    fn auto_medium_and_small_follow_the_rules() {
        let g = group("g", vec![b("small"), b("big").large()]);
        let m = items_at(&g, GroupSize::Medium);
        assert!(m[0].label.is_none() && m[0].tooltip.as_deref() == Some("small"), "a small control loses its label into the tooltip");
        assert_eq!((m[1].size, m[1].label.as_deref()), (ItemSize::Large, Some("big")));
        let s = items_at(&g, GroupSize::Small);
        assert_eq!((s[1].size, s[1].label.as_deref()), (ItemSize::Small, Some("big")), "a large button becomes small, labelled");
    }

    #[test]
    fn the_one_big_two_small_template_and_its_checks() {
        let items = vec![b("paste"), b("cut"), b("copy")];
        let g = RibbonGroup { size: SizeDefinition::Template(Template::ThreeButtonsOneBigAndTwoSmall), ..group("clip", items) };
        let l = items_at(&g, GroupSize::Large);
        assert_eq!(l.iter().map(|i| i.size).collect::<Vec<_>>(), [ItemSize::Large, ItemSize::Small, ItemSize::Small]);
        let s = items_at(&g, GroupSize::Small);
        assert!(s.iter().all(|i| i.size == ItemSize::Small && i.label.is_none()));
        assert!(Template::ThreeButtonsOneBigAndTwoSmall.check(&g.items).is_none());
        assert!(Template::TwoButtons.check(&g.items).is_some_and(|m| m.contains("2 control")));
        assert!(Template::InRibbonGalleryAndBigButton.check(&[RibbonItem::gallery("s", vec![]), b("x")]).is_none());
        assert_eq!(Template::parse("SevenButtons"), Some(Template::Buttons(7)));
        assert_eq!(Template::Buttons(7).name(), "SevenButtons");
        for name in Template::names() {
            assert_eq!(Template::parse(name).map(Template::name), Some(name));
        }
    }

    #[test]
    fn a_custom_definition_hides_resizes_and_breaks_columns() {
        let items = vec![b("a"), b("b"), b("c")];
        let level = LevelDefinition {
            size: GroupSize::Medium,
            controls: vec![
                ControlSize { control: "a".into(), image: Some(ImageSize::Hidden), ..Default::default() },
                ControlSize { control: "b".into(), image: Some(ImageSize::Large), label: Some(false), ..Default::default() },
                ControlSize { control: "c".into(), width: Some(80.0), column_break: true, ..Default::default() },
            ],
        };
        let g = RibbonGroup { size: SizeDefinition::Custom(vec![level]), ..group("g", items) };
        let m = items_at(&g, GroupSize::Medium);
        assert!(!m[0].visible);
        assert_eq!(m[1].size, ItemSize::Large);
        assert!(m[1].label.is_none());
        assert!(m[2].column_break && m[2].width == Some(80.0));
        // A level the definition does not give falls back to Auto.
        let s = items_at(&g, GroupSize::Small);
        assert!(s.iter().all(|i| i.visible));
    }

    #[test]
    fn names_round_trip() {
        for s in GroupSize::ALL {
            assert_eq!(GroupSize::parse(s.name()), Some(s));
        }
        assert_eq!(SizeDefinition::parse("Auto"), Some(SizeDefinition::Auto));
        assert_eq!(SizeDefinition::parse("OneButton"), Some(SizeDefinition::Template(Template::OneButton)));
        assert!(SizeDefinition::parse("Nope").is_none());
    }
}
