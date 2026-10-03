//! Names derived from database names: row structs, fields, functions.

/// `customers` → `Customer`, `order_lines` → `OrderLine`, `categories` → `Category`,
/// `Person` → `Person`, `addresses` → `Address`.
pub fn row_struct_name(table: &str) -> String {
    let base = table.rsplit('.').next().unwrap_or(table);
    let words: Vec<&str> = base.split(['_', ' ', '-']).filter(|w| !w.is_empty()).collect();
    let count = words.len();
    let mut out = String::new();
    for (i, w) in words.iter().enumerate() {
        let w = if i + 1 == count { singular(w) } else { (*w).to_string() };
        let mut chars = w.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert_str(0, "Row");
    }
    out
}

/// `customers_by_city` → `CustomersByCity` (no singularisation): the row type of a named query.
pub fn pascal_case(name: &str) -> String {
    let mut out = String::new();
    for w in name.split(['_', ' ', '-', '.']).filter(|w| !w.is_empty()) {
        let mut chars = w.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert_str(0, "Row");
    }
    out
}

/// A very small English singulariser, enough for table names.
pub fn singular(word: &str) -> String {
    let lower = word.to_ascii_lowercase();
    if lower.ends_with("ies") && word.len() > 3 {
        format!("{}y", &word[..word.len() - 3])
    } else if (lower.ends_with("sses") || lower.ends_with("xes") || lower.ends_with("ches") || lower.ends_with("shes")) && word.len() > 4 {
        word[..word.len() - 2].to_string()
    } else if lower.ends_with("ss") || lower.ends_with("us") || lower.ends_with("is") {
        word.to_string()
    } else if lower.ends_with('s') && word.len() > 1 {
        word[..word.len() - 1].to_string()
    } else {
        word.to_string()
    }
}

/// A database name as a Rust field / function identifier: lower snake case, keywords raw
/// (`type` → `r#type`), other characters replaced by `_`.
pub fn field_name(column: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for c in column.chars() {
        if c.is_ascii_uppercase() {
            if prev_lower {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
            prev_lower = false;
        } else if c.is_ascii_alphanumeric() {
            out.push(c);
            prev_lower = c.is_ascii_lowercase() || c.is_ascii_digit();
        } else {
            out.push('_');
            prev_lower = false;
        }
    }
    if out.is_empty() || out.starts_with(|c: char| c.is_ascii_digit()) {
        out.insert(0, '_');
    }
    if matches!(out.as_str(), "self" | "super" | "crate") {
        out.push('_');
    } else if is_keyword(&out) {
        out.insert_str(0, "r#");
    }
    out
}

fn is_keyword(s: &str) -> bool {
    matches!(
        s,
        "as" | "break"
            | "const"
            | "continue"
            | "else"
            | "enum"
            | "extern"
            | "false"
            | "fn"
            | "for"
            | "if"
            | "impl"
            | "in"
            | "let"
            | "loop"
            | "match"
            | "mod"
            | "move"
            | "mut"
            | "pub"
            | "ref"
            | "return"
            | "static"
            | "struct"
            | "trait"
            | "true"
            | "type"
            | "unsafe"
            | "use"
            | "where"
            | "while"
            | "async"
            | "await"
            | "dyn"
            | "abstract"
            | "become"
            | "box"
            | "do"
            | "final"
            | "macro"
            | "override"
            | "priv"
            | "typeof"
            | "unsized"
            | "virtual"
            | "yield"
            | "try"
            | "gen"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_names() {
        assert_eq!(row_struct_name("customers"), "Customer");
        assert_eq!(row_struct_name("order_lines"), "OrderLine");
        assert_eq!(row_struct_name("categories"), "Category");
        assert_eq!(row_struct_name("addresses"), "Address");
        assert_eq!(row_struct_name("status"), "Status");
        assert_eq!(row_struct_name("shop.boxes"), "Box");
        assert_eq!(row_struct_name("2024"), "Row2024");
        assert_eq!(pascal_case("customers_by_city"), "CustomersByCity");
        assert_eq!(pascal_case("top10"), "Top10");
    }

    #[test]
    fn field_names() {
        assert_eq!(field_name("CustomerId"), "customer_id");
        assert_eq!(field_name("birth date"), "birth_date");
        assert_eq!(field_name("type"), "r#type");
        assert_eq!(field_name("self"), "self_");
        assert_eq!(field_name("1st"), "_1st");
    }
}
