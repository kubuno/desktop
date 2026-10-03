//! Extending another app's ribbon (`vskubuno/docs/RIBBON.md` §8): a fragment of tabs, groups and
//! controls merged into a ribbon it names, without the host naming the extension.
//!
//! A module registers its fragment for a target ribbon (the ribbon's `x:Name`) with
//! [`register`]; the ribbon applies every fragment registered for it each frame ([`apply`]) and
//! hands the clicks on the fragment's controls back to it ([`dispatch`]). Dropping (or
//! [`MergeHandle::remove`]) the handle takes the fragment out again.
//!
//! - A [`TabMerge`] whose `merge` names an existing tab (by the id the ribbon gives it — its
//!   `x:Name` for a `.kbview` ribbon) adds its groups to that tab, else it adds a new tab.
//! - Each group goes before / after the group named by `insert_before` / `insert_after`
//!   (customUI's `insertAfterMso`), else at the end.

use std::cell::RefCell;
use std::rc::Rc;

use super::{RibbonGroup, RibbonTab};

/// One tab of a fragment: an existing tab to extend (`merge`), or a new tab (`tab`).
#[derive(Clone)]
pub struct TabMerge {
    /// The tab to add the groups to (its id), or `None` for a new tab.
    pub merge: Option<String>,
    /// The new tab, when `merge` is `None` (its own groups are added as they are).
    pub tab: Option<RibbonTab>,
    /// Groups added to the merged tab, each with its position.
    pub groups: Vec<GroupMerge>,
    /// A new tab goes before / after the tab with this id (else at the end).
    pub insert_before: Option<String>,
    pub insert_after: Option<String>,
}

/// One group added by a fragment, and where.
#[derive(Clone)]
pub struct GroupMerge {
    pub group: RibbonGroup,
    pub insert_before: Option<String>,
    pub insert_after: Option<String>,
}

impl GroupMerge {
    pub fn new(group: RibbonGroup) -> Self {
        Self { group, insert_before: None, insert_after: None }
    }

    pub fn after(mut self, id: impl Into<String>) -> Self {
        self.insert_after = Some(id.into());
        self
    }

    pub fn before(mut self, id: impl Into<String>) -> Self {
        self.insert_before = Some(id.into());
        self
    }
}

impl TabMerge {
    /// Adds `groups` to the existing tab `tab_id`.
    pub fn into_tab(tab_id: impl Into<String>, groups: Vec<GroupMerge>) -> Self {
        Self { merge: Some(tab_id.into()), tab: None, groups, insert_before: None, insert_after: None }
    }

    /// Adds `tab` as a new tab.
    pub fn new_tab(tab: RibbonTab) -> Self {
        Self { merge: None, tab: Some(tab), groups: Vec::new(), insert_before: None, insert_after: None }
    }
}

/// What runs when a fragment's control is clicked (the control's id).
pub type ClickHandler = Rc<dyn Fn(&str)>;

/// A fragment: its tabs, and what runs when one of its controls is clicked (the control's id).
#[derive(Clone)]
pub struct RibbonExtension {
    pub tabs: Vec<TabMerge>,
    pub on_click: Option<ClickHandler>,
}

impl RibbonExtension {
    pub fn new(tabs: Vec<TabMerge>) -> Self {
        Self { tabs, on_click: None }
    }

    pub fn on_click(mut self, f: impl Fn(&str) + 'static) -> Self {
        self.on_click = Some(Rc::new(f));
        self
    }
}

/// Inserts `item` into `list` before / after the element whose id `id_of` gives, else at the end.
fn insert_at<T>(list: &mut Vec<T>, item: T, before: Option<&str>, after: Option<&str>, id_of: impl Fn(&T) -> &str) {
    let at = before
        .and_then(|b| list.iter().position(|x| id_of(x) == b))
        .or_else(|| after.and_then(|a| list.iter().position(|x| id_of(x) == a).map(|i| i + 1)))
        .unwrap_or(list.len());
    list.insert(at, item);
}

/// Merges `ext` into `tabs` (pure).
pub fn merge_into(tabs: &mut Vec<RibbonTab>, ext: &RibbonExtension) {
    merge_into_named(tabs, ext, &|name| name.to_string());
}

/// [`merge_into`] where the fragment names tabs and groups by a name `id_of` turns into the ribbon's ids
/// (a `.kbview` ribbon: an `x:Name` to the element's stable id).
pub fn merge_into_named(tabs: &mut Vec<RibbonTab>, ext: &RibbonExtension, id_of: &dyn Fn(&str) -> String) {
    let named = |o: &Option<String>| o.as_deref().map(id_of);
    for t in &ext.tabs {
        match (&t.merge, &t.tab) {
            (Some(target), _) => {
                // An unknown target tab: the fragment's groups are not shown (the host may not have it).
                let target = id_of(target);
                let Some(tab) = tabs.iter_mut().find(|x| x.id == target) else { continue };
                for g in &t.groups {
                    insert_at(&mut tab.groups, g.group.clone(), named(&g.insert_before).as_deref(), named(&g.insert_after).as_deref(), |x: &RibbonGroup| x.id.as_str());
                }
            }
            (None, Some(tab)) => {
                let mut tab = tab.clone();
                for g in &t.groups {
                    insert_at(&mut tab.groups, g.group.clone(), g.insert_before.as_deref(), g.insert_after.as_deref(), |x: &RibbonGroup| x.id.as_str());
                }
                insert_at(tabs, tab, named(&t.insert_before).as_deref(), named(&t.insert_after).as_deref(), |x: &RibbonTab| x.id.as_str());
            }
            (None, None) => {}
        }
    }
}

type Registered = (u64, String, RibbonExtension);

thread_local! {
    static REGISTRY: RefCell<(u64, Vec<Registered>)> = const { RefCell::new((0, Vec::new())) };
}

/// Takes a fragment out of its ribbon when dropped (or [`MergeHandle::remove`]d).
pub struct MergeHandle {
    id: u64,
}

impl MergeHandle {
    /// Takes the fragment out now.
    pub fn remove(self) {}

    /// Keeps the fragment merged for the rest of the session (the handle is forgotten).
    pub fn keep(self) {
        std::mem::forget(self);
    }
}

impl Drop for MergeHandle {
    fn drop(&mut self) {
        let id = self.id;
        REGISTRY.with(|r| r.borrow_mut().1.retain(|(i, _, _)| *i != id));
        kubuno_desktop_controls::host::request_repaint_after(0);
    }
}

/// Registers `ext` for the ribbon named `target` (on this UI thread).
pub fn register(target: impl Into<String>, ext: RibbonExtension) -> MergeHandle {
    let id = REGISTRY.with(|r| {
        let mut r = r.borrow_mut();
        r.0 += 1;
        let id = r.0;
        r.1.push((id, target.into(), ext));
        id
    });
    kubuno_desktop_controls::host::request_repaint_after(0);
    MergeHandle { id }
}

/// Merges every fragment registered for `target` into `tabs`, in registration order.
pub fn apply(target: &str, tabs: &mut Vec<RibbonTab>) {
    apply_named(target, tabs, &|name| name.to_string());
}

/// [`apply`] with names turned into ids by `id_of` (see [`merge_into_named`]).
pub fn apply_named(target: &str, tabs: &mut Vec<RibbonTab>, id_of: &dyn Fn(&str) -> String) {
    let exts: Vec<RibbonExtension> = REGISTRY.with(|r| r.borrow().1.iter().filter(|(_, t, _)| t == target).map(|(_, _, e)| e.clone()).collect());
    for ext in &exts {
        merge_into_named(tabs, ext, id_of);
    }
}

/// Hands a click on control `id` to the fragment registered for `target` that holds it; false when
/// no fragment does (the control is the ribbon's own).
pub fn dispatch(target: &str, id: &str) -> bool {
    let handler = REGISTRY.with(|r| {
        r.borrow().1.iter().filter(|(_, t, _)| t == target).find(|(_, _, e)| holds(e, id)).and_then(|(_, _, e)| e.on_click.clone())
    });
    match handler {
        Some(h) => {
            h(id);
            true
        }
        None => false,
    }
}

fn holds(ext: &RibbonExtension, id: &str) -> bool {
    let in_group = |g: &RibbonGroup| g.id == id || g.items.iter().any(|it| it.id == id || it.children.iter().any(|c| c.id == id) || it.split_items.iter().any(|c| c.id == id));
    ext.tabs.iter().any(|t| t.groups.iter().any(|g| in_group(&g.group)) || t.tab.as_ref().is_some_and(|tab| tab.id == id || tab.groups.iter().any(in_group)))
}

#[cfg(test)]
mod tests {
    use super::super::{RibbonItem, RibbonTab};
    use super::*;

    fn tabs() -> Vec<RibbonTab> {
        vec![
            RibbonTab::new("home", "Accueil", vec![RibbonGroup::new("clip", "Presse-papiers", vec![]), RibbonGroup::new("font", "Police", vec![])]),
            RibbonTab::new("review", "Révision", vec![RibbonGroup::new("comments", "Commentaires", vec![])]),
        ]
    }

    #[test]
    fn groups_go_where_they_are_told_and_new_tabs_are_added() {
        let mut t = tabs();
        let ext = RibbonExtension::new(vec![
            TabMerge::into_tab("home", vec![GroupMerge::new(RibbonGroup::new("ai", "Assistant", vec![RibbonItem::button("sum", "Résumer", "Sparkles")])).after("clip")]),
            TabMerge::into_tab("missing", vec![GroupMerge::new(RibbonGroup::new("x", "X", vec![]))]),
            TabMerge { insert_before: Some("review".into()), ..TabMerge::new_tab(RibbonTab::new("ai_tab", "IA", vec![])) },
        ]);
        merge_into(&mut t, &ext);
        let home: Vec<&str> = t[0].groups.iter().map(|g| g.id.as_str()).collect();
        assert_eq!(home, vec!["clip", "ai", "font"]);
        let ids: Vec<&str> = t.iter().map(|x| x.id.as_str()).collect();
        assert_eq!(ids, vec!["home", "ai_tab", "review"]);
    }

    #[test]
    fn a_registered_fragment_is_applied_until_its_handle_goes_and_gets_its_clicks() {
        let clicked = Rc::new(RefCell::new(String::new()));
        let seen = clicked.clone();
        let ext = RibbonExtension::new(vec![TabMerge::into_tab("review", vec![GroupMerge::new(RibbonGroup::new("ai", "Assistant", vec![RibbonItem::button("sum", "Résumer", "Sparkles")]))])])
            .on_click(move |id| *seen.borrow_mut() = id.to_string());
        let handle = register("ribbon", ext);
        let mut t = tabs();
        apply("ribbon", &mut t);
        assert_eq!(t[1].groups.len(), 2);
        let mut other = tabs();
        apply("another", &mut other);
        assert_eq!(other[1].groups.len(), 1, "only the target ribbon");
        assert!(dispatch("ribbon", "sum"));
        assert_eq!(*clicked.borrow(), "sum");
        assert!(!dispatch("ribbon", "paste"));
        handle.remove();
        let mut t = tabs();
        apply("ribbon", &mut t);
        assert_eq!(t[1].groups.len(), 1, "removed with its handle");
    }
}
