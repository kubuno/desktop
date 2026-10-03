//! The language of the messages the tools show (vskubuno `docs/DESIGNER.md` §17): the diagnostics of
//! the parser, the validator and the builders, the design surface's own texts. They are written in
//! English in the code; Visual Studio tells its processes (the language server, the design surface)
//! its UI language through the `KUBUNO_UI_LANG` environment variable (`fr`, `en`), and every message
//! shown to the user goes through [`localize`] on its way out.
//!
//! The French texts are a table of templates (the English message with `{}` for its variable parts),
//! one place for every producer, so a message keeps its exact English shape everywhere else (tests,
//! logs). A message the table does not know is shown as written. Identifiers between backquotes
//! (element, attribute and value names) are never translated.

use std::sync::OnceLock;

/// The environment variable carrying the UI language of the host (Visual Studio).
pub const UI_LANGUAGE_VARIABLE: &str = "KUBUNO_UI_LANG";

static FRENCH: OnceLock<bool> = OnceLock::new();

/// Whether the tools speak French ([`UI_LANGUAGE_VARIABLE`] starts with `fr`), read once.
pub fn is_french() -> bool {
    *FRENCH.get_or_init(|| std::env::var(UI_LANGUAGE_VARIABLE).is_ok_and(|v| v.trim().to_ascii_lowercase().starts_with("fr")))
}

/// `english` or `french`, in the tools' language.
pub fn tr(english: &str, french: &str) -> String {
    if is_french() { french } else { english }.to_string()
}

/// `message` (an English diagnostic) in the tools' language.
pub fn localize(message: &str) -> String {
    if is_french() {
        to_french(message)
    } else {
        message.to_string()
    }
}

/// The templates: the English message, its French text. `{}` is a variable part copied as is; `{*}`
/// (at most one, last) is a nested message, translated in turn.
const TEMPLATES: &[(&str, &str)] = &[
    // A suggestion (`crate::validate`'s « did you mean »): first, it ends any other message.
    ("{*}; did you mean `{}`?", "{*} ; vouliez-vous écrire `{}` ?"),
    // The tolerant preview (`crate::tolerant`).
    ("{*} (ignored in the preview)", "{*} (ignoré dans l'aperçu)"),
    ("{*} (not shown in the preview)", "{*} (non affiché dans l'aperçu)"),
    // The validator (`crate::validate`).
    ("attribute `{}`: {*}", "attribut `{}` : {*}"),
    ("unknown element `<{}>`", "élément inconnu `<{}>`"),
    // The language server's note on an undeclared `x:` / `d:` prefix (`VIEWS-SPEC.md` §3).
    ("namespace prefix `{}` is not declared: add `{}` to the root element", "le préfixe d'espace de noms `{}` n'est pas déclaré : ajoutez `{}` à l'élément racine"),
    ("unknown attribute `{}` on `<{}>`", "attribut inconnu `{}` sur `<{}>`"),
    ("mismatched closing tag: expected `</{}>`, found `</{}>`", "balise fermante incorrecte : `</{}>` attendu, `</{}>` trouvé"),
    ("`{}` is a design-time attribute, only valid on the root element", "`{}` est un attribut de conception, valide uniquement sur l'élément racine"),
    ("`<{}>` does not accept children (found {})", "`<{}>` n'accepte pas d'enfant ({} trouvé(s))"),
    ("`<{}>` accepts at most one child, found {}", "`<{}>` accepte au plus un enfant ({} trouvés)"),
    ("`<{}>` is not valid inside `<{}>` here; valid only directly inside {}", "`<{}>` n'est pas valide ici dans `<{}>` ; valide uniquement directement dans {}"),
    ("`<{}>` is not valid inside `<{}>` here", "`<{}>` n'est pas valide ici dans `<{}>`"),
    ("a list is set with a binding (`{Binding Path}`), not a literal value", "une liste se définit par une liaison (`{Binding Path}`), pas par une valeur littérale"),
    ("a value of this type is set with a binding (`{Binding Path}`), not a literal value", "une valeur de ce type se définit par une liaison (`{Binding Path}`), pas par une valeur littérale"),
    ("expected `true` or `false`, found `{}`", "`true` ou `false` attendu, `{}` trouvé"),
    ("expected a number, found `{}`", "nombre attendu, `{}` trouvé"),
    ("`{}` is not valid here; expected one of: {}", "`{}` n'est pas valide ici ; valeurs possibles : {}"),
    ("`{}` is not an opacity: write a percentage from 0 to 100", "`{}` n'est pas une opacité : indiquez un pourcentage de 0 à 100"),
    ("`{}` is not one character", "`{}` n'est pas un seul caractère"),
    ("`{}` only applies to a child of a Dock/Anchor container such as `<Panel>`; it is ignored here", "`{}` ne s'applique qu'à un enfant d'un conteneur Dock/Anchor comme `<Panel>` ; il est ignoré ici"),
    ("`{}` is an older name for `{}`", "`{}` est un ancien nom de `{}`"),
    ("the file `{}` is not found beside the view", "le fichier `{}` est introuvable à côté de la vue"),
    // The builders (`crate::props`).
    ("malformed binding expression", "expression de liaison mal formée"),
    // Values with a grammar of their own (`crate::style`, `crate::icon`).
    ("`{}` is not a colour: write a theme colour such as Primary or Surface, #RRGGBB, a web colour name or a system colour name", "`{}` n'est pas une couleur : indiquez une couleur du thème comme Primary ou Surface, #RRGGBB, un nom de couleur web ou de couleur système"),
    ("`{}` is not a font style: use Bold, Italic, Underline or Strikeout", "`{}` n'est pas un style de police : utilisez Bold, Italic, Underline ou Strikeout"),
    ("`{}` is not a font size", "`{}` n'est pas une taille de police"),
    ("`{}` is not part of a font: write a family, a size in pt, then style=Bold, Italic…", "`{}` ne fait pas partie d'une police : indiquez une famille, une taille en pt, puis style=Bold, Italic…"),
    ("`{}` is not a spacing: write left, top, right, bottom in pixels (or one number)", "`{}` n'est pas un espacement : indiquez gauche, haut, droite, bas en pixels (ou un seul nombre)"),
    ("`{}` is not a size: write width, height in pixels", "`{}` n'est pas une taille : indiquez largeur, hauteur en pixels"),
    ("`{}` is not an icon of the Kubuno icon set; did you mean `{}`?", "`{}` n'est pas une icône du jeu d'icônes Kubuno ; vouliez-vous dire `{}` ?"),
    ("`{}` is not an icon of the Kubuno icon set (Save, FolderOpen, ChevronDown…) nor an image file", "`{}` n'est ni une icône du jeu d'icônes Kubuno (Save, FolderOpen, ChevronDown…) ni un fichier image"),
    // Menus and shortcuts (`crate::menus`, `kubuno_desktop_views_syntax::shortcut`).
    ("`{}` is not a key: write modifiers then one key, such as Ctrl+S, Ctrl+Shift+N, Alt+F4 or F5", "`{}` n'est pas une touche : indiquez des touches de modification puis une touche, comme Ctrl+S, Ctrl+Maj+N, Alt+F4 ou F5"),
    ("the shortcut has no key: write modifiers then one key, such as Ctrl+S", "le raccourci n'a pas de touche : indiquez des touches de modification puis une touche, comme Ctrl+S"),
    ("a shortcut has one key only: write modifiers then one key, such as Ctrl+S", "un raccourci n'a qu'une touche : indiquez des touches de modification puis une touche, comme Ctrl+S"),
    ("`{}` types text: add Ctrl or Alt, or use a function key", "`{}` saisit du texte : ajoutez Ctrl ou Alt, ou utilisez une touche de fonction"),
    ("the shortcut is empty", "le raccourci est vide"),
    ("the shortcut `{}` is already used on line {}", "le raccourci `{}` est déjà utilisé ligne {}"),
    ("the access key `{}` is already used by `{}` in this menu", "la touche d'accès `{}` est déjà utilisée par `{}` dans ce menu"),
    ("no `<{} x:Name=\"{}\">` in this view", "aucun `<{} x:Name=\"{}\">` dans cette vue"),
    // The parser (`crate::syntax`).
    ("expected the root element", "élément racine attendu"),
    ("empty document: expected a root element", "document vide : un élément racine est attendu"),
    ("unexpected content after the root element", "contenu inattendu après l'élément racine"),
    ("unterminated processing instruction, expected `?>`", "instruction de traitement non terminée, `?>` attendu"),
    ("expected an element name after `<`", "nom d'élément attendu après `<`"),
    ("unterminated tag, expected `>` or `/>`", "balise non terminée, `>` ou `/>` attendu"),
    ("expected a closing tag `</…>`", "balise fermante `</…>` attendue"),
    ("expected an element name after `</`", "nom d'élément attendu après `</`"),
    ("unterminated closing tag, expected `>`", "balise fermante non terminée, `>` attendu"),
    ("expected an attribute or the tag's closing `>`", "attribut ou `>` fermant la balise attendu"),
    ("expected a quoted attribute value after `=`", "valeur d'attribut entre guillemets attendue après `=`"),
    ("expected `=` after attribute name", "`=` attendu après le nom de l'attribut"),
    ("unterminated string literal", "chaîne non terminée"),
    ("unexpected content", "contenu inattendu"),
];

/// `message` in French when a template matches it, else as written.
pub fn to_french(message: &str) -> String {
    for (english, french) in TEMPLATES {
        if let Some(parts) = match_template(english, message) {
            return fill(french, &parts);
        }
    }
    message.to_string()
}

/// The variable parts of `message` when it has the shape of `template` (`{}` matches the shortest run
/// up to the next literal part; `{*}` the rest of the message).
fn match_template<'m>(template: &str, message: &'m str) -> Option<Vec<&'m str>> {
    // The template as literals separated by holes (`{}` and `{*}` alike: both capture).
    let literals: Vec<&str> = template.split("{*}").flat_map(|p| p.split("{}")).collect();
    let (first, rest_literals) = literals.split_first()?;
    let mut rest = message.strip_prefix(first)?;
    let mut parts = Vec::new();
    for (i, literal) in rest_literals.iter().enumerate() {
        let last = i + 1 == rest_literals.len();
        // The hole before `literal`: up to its first occurrence, or, for the last one, up to the end.
        let at = if last {
            rest.strip_suffix(literal)?.len()
        } else if literal.is_empty() {
            return None; // Two adjacent holes: ambiguous, no template has them.
        } else {
            rest.find(literal)?
        };
        parts.push(&rest[..at]);
        rest = &rest[at + literal.len()..];
    }
    rest.is_empty().then_some(parts)
}

/// `template` with its holes filled by `parts` in order (`{*}` translated in turn).
fn fill(template: &str, parts: &[&str]) -> String {
    let mut out = String::new();
    let mut rest = template;
    let mut values = parts.iter();
    loop {
        let next_plain = rest.find("{}");
        let next_nested = rest.find("{*}");
        let (at, nested) = match (next_plain, next_nested) {
            (Some(a), Some(b)) if b < a => (b, true),
            (Some(a), _) => (a, false),
            (None, Some(b)) => (b, true),
            (None, None) => break,
        };
        out.push_str(&rest[..at]);
        let value = values.next().copied().unwrap_or("");
        if nested {
            out.push_str(&to_french(value));
            rest = &rest[at + 3..];
        } else {
            out.push_str(value);
            rest = &rest[at + 2..];
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::to_french;

    #[test]
    fn the_validator_s_messages_read_in_french() {
        assert_eq!(to_french("unknown element `<Frobnicator>`"), "élément inconnu `<Frobnicator>`");
        assert_eq!(to_french("unknown attribute `Text` on `<Switch>`"), "attribut inconnu `Text` sur `<Switch>`");
        assert_eq!(to_french("attribute `Width`: expected a number, found `wide`"), "attribut `Width` : nombre attendu, `wide` trouvé");
        assert_eq!(
            to_french("attribute `Variant`: `Nope` is not valid here; expected one of: Primary, Secondary"),
            "attribut `Variant` : `Nope` n'est pas valide ici ; valeurs possibles : Primary, Secondary"
        );
        assert_eq!(
            to_french("unknown attribute `Text` on `<Switch>` (ignored in the preview)"),
            "attribut inconnu `Text` sur `<Switch>` (ignoré dans l'aperçu)"
        );
        assert_eq!(to_french("`<TabItem>` is not valid inside `<Stack>` here; valid only directly inside `<Tabs>`"), "`<TabItem>` n'est pas valide ici dans `<Stack>` ; valide uniquement directement dans `<Tabs>`");
        assert_eq!(to_french("unterminated tag, expected `>` or `/>`"), "balise non terminée, `>` ou `/>` attendu");
        assert_eq!(to_french("mismatched closing tag: expected `</Label>`, found `</Button>`"), "balise fermante incorrecte : `</Label>` attendu, `</Button>` trouvé");
        assert_eq!(
            to_french("unknown attribute `Binding` on `<Label>`; did you mean `Text=\"{Binding …}\"`?"),
            "attribut inconnu `Binding` sur `<Label>` ; vouliez-vous écrire `Text=\"{Binding …}\"` ?"
        );
        assert_eq!(
            to_french("unknown attribute `Colr` on `<Label>`; did you mean `ForeColor`? (ignored in the preview)"),
            "attribut inconnu `Colr` sur `<Label>` ; vouliez-vous écrire `ForeColor` ? (ignoré dans l'aperçu)"
        );
    }

    #[test]
    fn the_menu_checks_read_in_french() {
        assert_eq!(
            to_french("attribute `ShortcutKeys`: `Shift+A` types text: add Ctrl or Alt, or use a function key"),
            "attribut `ShortcutKeys` : `Shift+A` saisit du texte : ajoutez Ctrl ou Alt, ou utilisez une touche de fonction"
        );
        assert_eq!(to_french("the shortcut `Ctrl+S` is already used on line 4"), "le raccourci `Ctrl+S` est déjà utilisé ligne 4");
        assert_eq!(to_french("the access key `F` is already used by `Fichier` in this menu"), "la touche d'accès `F` est déjà utilisée par `Fichier` dans ce menu");
        assert_eq!(to_french("attribute `Command`: no `<Command x:Name=\"cmd_save\">` in this view"), "attribut `Command` : aucun `<Command x:Name=\"cmd_save\">` dans cette vue");
    }

    #[test]
    fn an_unknown_message_is_kept_as_written() {
        assert_eq!(to_french("something new happened"), "something new happened");
        assert_eq!(to_french("attribute `X`: something new"), "attribut `X` : something new");
    }
}
