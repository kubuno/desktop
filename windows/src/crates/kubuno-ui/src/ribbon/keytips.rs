//! KeyTips (`vskubuno/docs/RIBBON.md` §2): the letters Office shows over its tabs and commands
//! when Alt is pressed and released. The first level names the tabs (and the quick actions, by
//! digit); typing a tab's letter selects it and shows the second level, over its groups and
//! controls; typing a control's letters activates it. Escape goes back one level.
//!
//! This module holds the pure parts: automatic assignment (an explicit `KeyTip` wins), conflict
//! detection (the designer's diagnostic) and the badge painter.

use drive_app_controls::{Canvas, Rect};
use windows::Win32::Graphics::DirectWrite::IDWriteTextFormat;

/// The letters a label offers, in order: its letters and digits, upper-cased.
fn candidates(label: &str) -> Vec<char> {
    label.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_uppercase).filter(|c| c.is_ascii_alphanumeric()).collect()
}

/// Assigns a KeyTip to every entry: `(explicit, label)`. Explicit tips are kept (upper-cased); the
/// others get the first unused letter of their label, else the first unused letter + digit pair
/// when `two_letters`, else a digit. Tips are unique among the entries.
pub fn assign(entries: &[(Option<&str>, &str)], two_letters: bool) -> Vec<String> {
    let mut used: Vec<String> = entries.iter().filter_map(|(e, _)| e.map(|t| t.trim().to_uppercase())).filter(|t| !t.is_empty()).collect();
    let mut out = Vec::with_capacity(entries.len());
    for (explicit, label) in entries {
        if let Some(t) = explicit.map(|t| t.trim().to_uppercase()).filter(|t| !t.is_empty()) {
            out.push(t);
            continue;
        }
        let letters = candidates(label);
        let free = |t: &String, used: &Vec<String>| !used.iter().any(|u| u == t || u.starts_with(t.as_str()) || t.starts_with(u.as_str()));
        let mut pick: Option<String> = letters.iter().map(|c| c.to_string()).find(|t| free(t, &used));
        if pick.is_none() && two_letters {
            if let Some(first) = letters.first() {
                pick = letters.iter().skip(1).map(|c| format!("{first}{c}")).chain((1..=9).map(|d| format!("{first}{d}"))).find(|t| free(t, &used));
            }
        }
        if pick.is_none() {
            pick = (1..=9).map(|d| d.to_string()).chain(('A'..='Z').flat_map(|a| ('A'..='Z').map(move |b| format!("{a}{b}")))).find(|t| free(t, &used));
        }
        let tip = pick.unwrap_or_default();
        used.push(tip.clone());
        out.push(tip);
    }
    out
}

/// Pairs of explicit tips that collide (equal, or one a prefix of the other): `(a, b, tip)` by
/// entry name — "two KeyTips collide within one tab".
pub fn conflicts(tips: &[(String, String)]) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    for (i, (a, ta)) in tips.iter().enumerate() {
        for (b, tb) in tips.iter().skip(i + 1) {
            let (ta, tb) = (ta.to_uppercase(), tb.to_uppercase());
            if !ta.is_empty() && !tb.is_empty() && (ta.starts_with(&tb) || tb.starts_with(&ta)) {
                out.push((a.clone(), b.clone(), ta.clone()));
            }
        }
    }
    out
}

/// The badge's height and horizontal padding.
pub const BADGE_H: f32 = 16.0;
pub const BADGE_PAD: f32 = 4.0;

/// Paints one KeyTip badge centred on `(cx, cy)`: the inverse of the theme (dark on a light
/// theme), like a tooltip. `dim` fades a tip that no longer matches what was typed.
pub fn paint_badge(c: &dyn Canvas, tip: &str, cx: f32, cy: f32, format: &IDWriteTextFormat, dim: bool) {
    let t = c.theme();
    let w = c.measure(tip, format) + BADGE_PAD * 2.0;
    let w = w.max(BADGE_H);
    let r = Rect::new(cx - w / 2.0, cy - BADGE_H / 2.0, cx + w / 2.0, cy + BADGE_H / 2.0);
    let mut bg = t.text_primary;
    let mut fg = t.layer_background;
    if dim {
        bg.a *= 0.35;
        fg.a *= 0.6;
    }
    c.fill_rounded(&r, 3.0, &bg);
    c.text(tip, &r, format, &fg, true);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_tips_win_and_others_take_their_first_free_letter() {
        let tips = assign(&[(Some("h"), "Accueil"), (None, "Insertion"), (None, "Affichage"), (None, "Accès")], false);
        assert_eq!(tips, ["H", "I", "A", "C"]);
    }

    #[test]
    fn second_level_tips_use_two_letters_before_digits() {
        let tips = assign(&[(None, "Coller"), (None, "Couper"), (None, "Copier"), (None, "Couleur")], true);
        assert_eq!(tips[0], "C");
        // "C" is taken, and a two-letter tip may not start with a used single letter.
        assert!(tips[1..].iter().all(|t| !t.starts_with('C')), "{tips:?}");
        let mut sorted = tips.clone();
        sorted.dedup();
        assert_eq!(sorted.len(), 4);
    }

    #[test]
    fn prefixes_and_duplicates_are_conflicts() {
        let c = conflicts(&[("a".into(), "B".into()), ("b".into(), "BO".into()), ("c".into(), "X".into()), ("d".into(), "x".into())]);
        assert_eq!(c.len(), 2);
        assert_eq!((c[0].0.as_str(), c[0].1.as_str()), ("a", "b"));
        assert_eq!(c[1].2, "X");
    }
}
