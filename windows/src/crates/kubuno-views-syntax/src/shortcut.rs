//! The keyboard shortcut grammar of `ShortcutKeys` and `Shortcut` (`vskubuno/docs/MENUS.md` §4):
//! modifiers then one key, joined by `+` — `Ctrl+S`, `Ctrl+Shift+N`, `Alt+F4`, `F5`, `Ctrl++`,
//! `Shift+Delete`. Platform-neutral: the desktop maps a [`Key`] to its virtual-key code, the web to
//! `KeyboardEvent.key`, the language server checks the text and finds two commands sharing one
//! shortcut.
//!
//! Modifier names are case-insensitive and have their usual aliases (`Ctrl`/`Control`, `Shift`/`Maj`,
//! `Alt`); key names are the WinForms `Keys` names a Windows developer writes (`Delete`, `PageDown`,
//! `OemPlus` is spelled `Plus`…), case-insensitive, with the common short forms (`Del`, `Ins`, `Esc`,
//! `PgUp`, `PgDn`). A single printable character is that key (`Ctrl+,`).

use std::fmt;

/// One key of a shortcut, without its modifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    /// `A`…`Z` (always upper case).
    Letter(char),
    /// `0`…`9` (the top row).
    Digit(u8),
    /// `F1`…`F24`.
    Function(u8),
    Delete,
    Insert,
    Home,
    End,
    PageUp,
    PageDown,
    Up,
    Down,
    Left,
    Right,
    Enter,
    Escape,
    Space,
    Tab,
    Backspace,
    /// `+` (the `=`/`+` key, `OemPlus`).
    Plus,
    /// `-` (`OemMinus`).
    Minus,
    /// `,` (`OemComma`).
    Comma,
    /// `.` (`OemPeriod`).
    Period,
    /// `/` (`OemQuestion` on US layouts).
    Slash,
    /// `;` (`OemSemicolon`).
    Semicolon,
    /// The context-menu key (`Apps`).
    Apps,
}

impl Key {
    /// The key a name designates (`"Del"` → [`Key::Delete`]), case-insensitive.
    pub fn parse(name: &str) -> Option<Key> {
        let n = name.trim();
        let mut chars = n.chars();
        if let (Some(c), None) = (chars.next(), chars.next()) {
            return match c {
                'a'..='z' | 'A'..='Z' => Some(Key::Letter(c.to_ascii_uppercase())),
                '0'..='9' => Some(Key::Digit(c as u8 - b'0')),
                '+' | '=' => Some(Key::Plus),
                '-' => Some(Key::Minus),
                ',' => Some(Key::Comma),
                '.' => Some(Key::Period),
                '/' => Some(Key::Slash),
                ';' => Some(Key::Semicolon),
                _ => None,
            };
        }
        let lower = n.to_ascii_lowercase();
        if let Some(number) = lower.strip_prefix('f') {
            if let Ok(f) = number.parse::<u8>() {
                return (1..=24).contains(&f).then_some(Key::Function(f));
            }
        }
        if let Some(d) = lower.strip_prefix('d').and_then(|d| d.parse::<u8>().ok()).filter(|d| *d <= 9) {
            // WinForms spells the top-row digits `D0`…`D9`.
            return Some(Key::Digit(d));
        }
        Some(match lower.as_str() {
            "delete" | "del" => Key::Delete,
            "insert" | "ins" => Key::Insert,
            "home" => Key::Home,
            "end" => Key::End,
            "pageup" | "pgup" | "prior" => Key::PageUp,
            "pagedown" | "pgdn" | "next" => Key::PageDown,
            "up" => Key::Up,
            "down" => Key::Down,
            "left" => Key::Left,
            "right" => Key::Right,
            "enter" | "return" => Key::Enter,
            "escape" | "esc" => Key::Escape,
            "space" | "spacebar" => Key::Space,
            "tab" => Key::Tab,
            "backspace" | "back" => Key::Backspace,
            "plus" | "oemplus" | "add" => Key::Plus,
            "minus" | "oemminus" | "subtract" => Key::Minus,
            "comma" | "oemcomma" => Key::Comma,
            "period" | "oemperiod" => Key::Period,
            "slash" | "oemquestion" => Key::Slash,
            "semicolon" | "oemsemicolon" => Key::Semicolon,
            "apps" | "menu" => Key::Apps,
            _ => return None,
        })
    }

    /// The key's canonical name (`Delete`, `F5`, `Plus`).
    pub fn name(self) -> String {
        match self {
            Key::Letter(c) => c.to_string(),
            Key::Digit(d) => d.to_string(),
            Key::Function(f) => format!("F{f}"),
            Key::Delete => "Delete".into(),
            Key::Insert => "Insert".into(),
            Key::Home => "Home".into(),
            Key::End => "End".into(),
            Key::PageUp => "PageUp".into(),
            Key::PageDown => "PageDown".into(),
            Key::Up => "Up".into(),
            Key::Down => "Down".into(),
            Key::Left => "Left".into(),
            Key::Right => "Right".into(),
            Key::Enter => "Enter".into(),
            Key::Escape => "Escape".into(),
            Key::Space => "Space".into(),
            Key::Tab => "Tab".into(),
            Key::Backspace => "Backspace".into(),
            Key::Plus => "Plus".into(),
            Key::Minus => "Minus".into(),
            Key::Comma => "Comma".into(),
            Key::Period => "Period".into(),
            Key::Slash => "Slash".into(),
            Key::Semicolon => "Semicolon".into(),
            Key::Apps => "Apps".into(),
        }
    }

    /// Whether the key without Ctrl or Alt types text or moves in it, so that a shortcut made of it
    /// would steal it from every text box (`Shift+A`, `Space`, `Home`…). The F-keys, Escape, Apps,
    /// Delete and Insert are fine alone or with Shift (WinForms accepts `Delete` and `Shift+Delete`).
    pub fn types_text(self) -> bool {
        !matches!(self, Key::Function(_) | Key::Escape | Key::Apps | Key::Delete | Key::Insert)
    }
}

/// A parsed shortcut: its modifiers and its key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Shortcut {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub key: Key,
}

impl fmt::Display for Shortcut {
    /// The canonical spelling: `Ctrl+Shift+Alt+Key` (`Ctrl+Plus` for `Ctrl++`).
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.ctrl {
            f.write_str("Ctrl+")?;
        }
        if self.shift {
            f.write_str("Shift+")?;
        }
        if self.alt {
            f.write_str("Alt+")?;
        }
        f.write_str(&self.key.name())
    }
}

/// Why a shortcut does not parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ShortcutError {
    /// Nothing written.
    Empty,
    /// `name` is neither a modifier nor a key.
    UnknownKey(String),
    /// Only modifiers (`Ctrl+Shift`).
    NoKey,
    /// Two keys (`Ctrl+A+B`).
    TwoKeys,
    /// The key alone, or with Shift only, types text: it would be taken from every text box.
    TypesText(String),
}

impl fmt::Display for ShortcutError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShortcutError::Empty => f.write_str("the shortcut is empty"),
            ShortcutError::UnknownKey(k) => write!(f, "`{k}` is not a key: write modifiers then one key, such as Ctrl+S, Ctrl+Shift+N, Alt+F4 or F5"),
            ShortcutError::NoKey => f.write_str("the shortcut has no key: write modifiers then one key, such as Ctrl+S"),
            ShortcutError::TwoKeys => f.write_str("a shortcut has one key only: write modifiers then one key, such as Ctrl+S"),
            ShortcutError::TypesText(s) => write!(f, "`{s}` types text: add Ctrl or Alt, or use a function key"),
        }
    }
}

/// Parses a shortcut (see the module doc). `Ctrl++` and `Ctrl+-` name the plus and minus keys.
pub fn parse(text: &str) -> Result<Shortcut, ShortcutError> {
    let text = text.trim();
    if text.is_empty() {
        return Err(ShortcutError::Empty);
    }
    // A trailing `+` after a separator is the plus key (`Ctrl++`); a lone `+` too.
    let (body, plus) = match text.strip_suffix("++") {
        Some(b) => (b, true),
        None if text == "+" => ("", true),
        None => (text, false),
    };
    let (mut ctrl, mut shift, mut alt) = (false, false, false);
    let mut key = plus.then_some(Key::Plus);
    for part in body.split('+').map(str::trim).filter(|p| !p.is_empty() || !plus) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" | "ctl" | "strg" => ctrl = true,
            "shift" | "maj" => shift = true,
            "alt" => alt = true,
            "" => return Err(ShortcutError::UnknownKey(String::new())),
            _ => {
                let k = Key::parse(part).ok_or_else(|| ShortcutError::UnknownKey(part.to_string()))?;
                if key.is_some() {
                    return Err(ShortcutError::TwoKeys);
                }
                key = Some(k);
            }
        }
    }
    let key = key.ok_or(ShortcutError::NoKey)?;
    let shortcut = Shortcut { ctrl, shift, alt, key };
    if !ctrl && !alt && key.types_text() {
        return Err(ShortcutError::TypesText(shortcut.to_string()));
    }
    Ok(shortcut)
}

/// The mnemonic letter of a text (`&File` → `f`, lower case), as a menu, a button or a label reads
/// it: the character after the first single `&` (`&&` is a literal ampersand). `None` without one.
pub fn mnemonic_key(text: &str) -> Option<char> {
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '&' {
            match chars.next() {
                Some('&') => {}
                Some(next) if !next.is_whitespace() => return next.to_lowercase().next(),
                _ => return None,
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modifiers_and_keys_parse_in_any_case() {
        let s = parse("ctrl+shift+s").unwrap();
        assert!(s.ctrl && s.shift && !s.alt);
        assert_eq!(s.key, Key::Letter('S'));
        assert_eq!(s.to_string(), "Ctrl+Shift+S");
        assert_eq!(parse("Control+Maj+N").unwrap().to_string(), "Ctrl+Shift+N");
        assert_eq!(parse("Alt+F4").unwrap().to_string(), "Alt+F4");
        assert_eq!(parse("F5").unwrap().key, Key::Function(5));
        assert_eq!(parse("F24").unwrap().key, Key::Function(24));
        assert_eq!(parse("Shift+Del").unwrap().to_string(), "Shift+Delete");
        assert_eq!(parse("Ctrl+D1").unwrap().key, Key::Digit(1));
        assert_eq!(parse("Ctrl+PgDn").unwrap().key, Key::PageDown);
        assert_eq!(parse("Escape").unwrap().key, Key::Escape);
    }

    #[test]
    fn plus_minus_and_punctuation_are_keys() {
        assert_eq!(parse("Ctrl++").unwrap().to_string(), "Ctrl+Plus");
        assert_eq!(parse("Ctrl+-").unwrap().key, Key::Minus);
        assert_eq!(parse("Ctrl+,").unwrap().key, Key::Comma);
        assert_eq!(parse("Ctrl+Plus").unwrap(), parse("Ctrl++").unwrap());
    }

    #[test]
    fn mistakes_are_explained() {
        assert_eq!(parse(""), Err(ShortcutError::Empty));
        assert_eq!(parse("Ctrl+Shift"), Err(ShortcutError::NoKey));
        assert_eq!(parse("Ctrl+A+B"), Err(ShortcutError::TwoKeys));
        assert_eq!(parse("Ctrl+Foo"), Err(ShortcutError::UnknownKey("Foo".into())));
        assert_eq!(parse("F25"), Err(ShortcutError::UnknownKey("F25".into())));
        assert_eq!(parse("Shift+A"), Err(ShortcutError::TypesText("Shift+A".into())));
        assert_eq!(parse("Space"), Err(ShortcutError::TypesText("Space".into())));
        assert_eq!(parse("Delete").unwrap().key, Key::Delete);
        assert!(parse("Ctrl+Delete").is_ok());
    }

    #[test]
    fn mnemonic_keys_skip_double_ampersands() {
        assert_eq!(mnemonic_key("&File"), Some('f'));
        assert_eq!(mnemonic_key("Save &As…"), Some('a'));
        assert_eq!(mnemonic_key("Fish && &Chips"), Some('c'));
        assert_eq!(mnemonic_key("Plain"), None);
        assert_eq!(mnemonic_key("End &"), None);
    }
}
