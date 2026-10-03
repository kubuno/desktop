//! The texts Kubuno shows for printing (the preview dialog, the Save dialog of print to file), in the
//! user's UI language: French or English.

use crate::native::ui_is_french;

fn t(en: &str, fr: &str) -> String {
    if ui_is_french() { fr } else { en }.to_string()
}

pub(crate) fn preview_title() -> String {
    t("Print preview", "Aperçu avant impression")
}

pub(crate) fn print() -> String {
    t("Print", "Imprimer")
}

pub(crate) fn close() -> String {
    t("Close", "Fermer")
}

pub(crate) fn page() -> String {
    t("Page", "Page")
}

pub(crate) fn of_pages(count: usize) -> String {
    if ui_is_french() {
        format!("sur {count}")
    } else {
        format!("of {count}")
    }
}

pub(crate) fn zoom_auto() -> String {
    t("Auto", "Automatique")
}

pub(crate) fn pages_tooltip(pages: u32) -> String {
    match pages {
        1 => t("One page", "Une page"),
        2 => t("Two pages", "Deux pages"),
        3 => t("Three pages", "Trois pages"),
        4 => t("Four pages", "Quatre pages"),
        _ => t("Six pages", "Six pages"),
    }
}

pub(crate) fn zoom_tooltip() -> String {
    t("Zoom", "Zoom")
}

pub(crate) fn generating() -> String {
    t("Generating the preview…", "Génération de l'aperçu…")
}

pub(crate) fn no_pages() -> String {
    t("No pages to preview.", "Aucune page à afficher.")
}

pub(crate) fn preview_failed(message: &str) -> String {
    if ui_is_french() {
        format!("L'aperçu n'a pas pu être généré : {message}")
    } else {
        format!("The preview could not be generated: {message}")
    }
}

pub(crate) fn all_files() -> String {
    t("All files (*.*)", "Tous les fichiers (*.*)")
}

pub(crate) fn print_to_file_title() -> String {
    t("Print to file", "Imprimer dans un fichier")
}
