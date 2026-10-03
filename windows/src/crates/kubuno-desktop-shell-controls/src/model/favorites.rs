//! The waffle's favourites: the list being edited, and what gets written back
//! to the server (by the host's [`crate::LauncherService::save_favorites`]).
//!
//! The list is the SAME one the web shows — `preferences.waffle_favorites` on
//! the account, an ordered array of app ids. This module therefore has one
//! non-obvious duty: never destroy what it does not understand.

/// `FAV_MAX` on the web: the white card holds a 3x3 grid.
pub const MAX: usize = 9;

/// The favourites being edited, plus the ids this build cannot resolve.
pub struct Draft {
    /// Ids of apps we know, in display order — what the user reorders.
    pub known: Vec<String>,
    /// Ids kept verbatim from the server that match no installed app here.
    unknown: Vec<String>,
}

impl Draft {
    /// Splits the server's list into what this build can show and what it must
    /// merely carry.
    ///
    /// The web's own edit path copies its FILTERED list, so confirming an edit
    /// there drops any id it cannot resolve. Doing the same here would be far
    /// worse: the desktop knows a different set of apps from the web (a module
    /// whose frontend bundle is not loaded still has sidebar items), so a
    /// confirm from the desktop would silently delete the web's favourites.
    pub fn new(saved: &[String], installed: &[String]) -> Self {
        let (known, unknown) = saved
            .iter()
            .cloned()
            .partition(|id| installed.iter().any(|a| a == id));
        Self { known, unknown }
    }

    pub fn contains(&self, id: &str) -> bool {
        self.known.iter().any(|x| x == id)
    }

    /// Adds or removes, like the web's single `toggleDraft`.
    pub fn toggle(&mut self, id: &str) {
        if let Some(i) = self.known.iter().position(|x| x == id) {
            self.known.remove(i);
        } else {
            self.known.push(id.to_string());
            // The web caps AFTER pushing, so a tenth favourite added at the end
            // simply drops itself — no message, no disabled button. Reproduced
            // as it stands, asymmetry included: an insertion at a position
            // evicts the last one instead.
            self.known.truncate(MAX);
        }
    }

    /// Moves `id` in front of `before`, the reorder the web does by dragging.
    pub fn move_before(&mut self, id: &str, before: &str) {
        let Some(from) = self.known.iter().position(|x| x == id) else { return };
        let Some(to) = self.known.iter().position(|x| x == before) else { return };
        if from == to {
            return;
        }
        let moved = self.known.remove(from);
        let to = self.known.iter().position(|x| x == before).unwrap_or(to.min(self.known.len()));
        self.known.insert(to, moved);
    }

    /// The list to persist: what is shown, then what was carried through.
    ///
    /// The unknown ids go last because their original positions no longer mean
    /// anything once the known ones have been reordered — and appending is the
    /// only placement that cannot silently reorder someone else's list.
    pub fn to_saved(&self) -> Vec<String> {
        let mut out = self.known.clone();
        out.extend(self.unknown.iter().cloned());
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn installed() -> Vec<String> {
        ["drive", "mail", "notes", "maps"].iter().map(|s| s.to_string()).collect()
    }

    /// An id this build cannot resolve survives an edit — the whole point.
    #[test]
    fn unknown_ids_are_never_lost() {
        let saved: Vec<String> = ["drive", "office-whiteboard", "mail"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let mut d = Draft::new(&saved, &installed());
        assert_eq!(d.known, ["drive", "mail"]);

        d.toggle("notes");
        let out = d.to_saved();
        assert!(out.contains(&"office-whiteboard".to_string()), "the web's favourite vanished");
        assert!(out.contains(&"notes".to_string()));
        assert_eq!(out.len(), 4);
    }

    /// Toggling adds then removes, and only the id asked for.
    #[test]
    fn toggling_adds_then_removes() {
        let mut d = Draft::new(&[], &installed());
        d.toggle("drive");
        d.toggle("mail");
        assert_eq!(d.known, ["drive", "mail"]);
        d.toggle("drive");
        assert_eq!(d.known, ["mail"]);
        assert!(!d.contains("drive"));
    }

    /// The cap holds, and an addition beyond it drops ITSELF — the web's exact
    /// behaviour when appending.
    #[test]
    fn the_cap_drops_the_newcomer() {
        let all: Vec<String> = (0..12).map(|i| format!("app{i}")).collect();
        let mut d = Draft::new(&[], &all);
        for id in &all {
            d.toggle(id);
        }
        assert_eq!(d.known.len(), MAX);
        assert_eq!(d.known[0], "app0", "the first favourites stay");
        assert!(!d.contains("app9"), "the tenth dropped itself");
    }

    /// Reordering puts the moved item in front of its target.
    #[test]
    fn reordering_inserts_before_the_target() {
        let saved: Vec<String> = ["drive", "mail", "notes"].iter().map(|s| s.to_string()).collect();
        let mut d = Draft::new(&saved, &installed());
        d.move_before("notes", "drive");
        assert_eq!(d.known, ["notes", "drive", "mail"]);
        d.move_before("notes", "mail");
        assert_eq!(d.known, ["drive", "notes", "mail"]);
    }

    /// A move to itself, or with an id that is not there, changes nothing.
    #[test]
    fn a_meaningless_move_is_a_no_op() {
        let saved: Vec<String> = ["drive", "mail"].iter().map(|s| s.to_string()).collect();
        let mut d = Draft::new(&saved, &installed());
        d.move_before("drive", "drive");
        d.move_before("absent", "mail");
        d.move_before("drive", "absent");
        assert_eq!(d.known, ["drive", "mail"]);
    }
}
