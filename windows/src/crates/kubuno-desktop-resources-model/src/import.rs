//! Import of .NET resource files (`.resx`, and the `.resw` of UWP/WinUI apps — the same format):
//! their string entries (`<data name="X" xml:space="preserve"><value>…</value><comment>…</comment></data>`)
//! become `String` entries. Non-string data (`type="System.Drawing.Bitmap…"`, `mimetype=…`) and
//! file references (`System.Resources.ResXFileRef`) are reported, not converted.

use crate::format::{Entry, Kind, ResourceFile};
use crate::xml;

/// The result of an import.
#[derive(Debug, Default)]
pub struct Imported {
    pub file: ResourceFile,
    /// Entries left out, with the reason (`name: reason`).
    pub skipped: Vec<String>,
}

/// Converts the text of a `.resx`/`.resw` file.
pub fn import_resx(text: &str) -> Result<Imported, String> {
    let root = xml::parse(text).map_err(|e| e.to_string())?;
    if root.name != "root" {
        return Err(format!("not a .resx/.resw file (root element `<{}>`)", root.name));
    }
    let mut out = Imported::default();
    for data in root.elements().filter(|e| e.name == "data") {
        let Some(name) = data.attr("name") else { continue };
        if data.attr("type").is_some() || data.attr("mimetype").is_some() {
            out.skipped.push(format!("{name}: not a string (type/mimetype)"));
            continue;
        }
        if !crate::names::is_valid_name(name) {
            out.skipped.push(format!("{name}: not a valid resource name"));
            continue;
        }
        if out.file.get(name).is_some() {
            out.skipped.push(format!("{name}: duplicate"));
            continue;
        }
        let value = data.elements().find(|e| e.name == "value").map(|v| v.text()).unwrap_or_default();
        let comment = data.elements().find(|e| e.name == "comment").map(|c| c.text()).unwrap_or_default();
        out.file.entries.push(Entry::text(Kind::String, name, value).with_comment(comment));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn imports_string_data() {
        let resw = r#"<?xml version="1.0" encoding="utf-8"?>
<root>
  <xsd:schema id="root"><xsd:element name="root"/></xsd:schema>
  <resheader name="resmimetype"><value>text/microsoft-resx</value></resheader>
  <data name="Home" xml:space="preserve"><value>Accueil</value></data>
  <data name="PropertiesCreated.Text" xml:space="preserve"><value>Créé :</value><comment>Label</comment></data>
  <data name="Logo" type="System.Drawing.Bitmap, System.Drawing" mimetype="application/x-microsoft.net.object.bytearray.base64"><value>AAAA</value></data>
  <data name="Bad Name" xml:space="preserve"><value>x</value></data>
</root>"#;
        let imported = import_resx(resw).unwrap();
        assert_eq!(imported.file.entries.len(), 2);
        assert_eq!(imported.file.get("Home").unwrap().as_text(), Some("Accueil"));
        assert_eq!(imported.file.get("PropertiesCreated.Text").unwrap().comment.as_deref(), Some("Label"));
        assert_eq!(imported.skipped.len(), 2);
        assert!(import_resx("<Resources/>").is_err());
    }
}
