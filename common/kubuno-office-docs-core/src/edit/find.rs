//! Find (and the matches Replace works on): every occurrence of a string in the body's text, as
//! ProseMirror ranges. A match never crosses a textblock boundary (the web's search works per
//! block too); atoms count as one non-matching character.

use crate::model::Node;
use crate::pm::{node_at, textblocks};

use super::inline;

/// Options of a search.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FindOptions {
    pub match_case: bool,
    pub whole_word: bool,
}

fn fold(u: u16, match_case: bool) -> u16 {
    if match_case {
        return u;
    }
    match char::from_u32(u as u32) {
        Some(c) => {
            let mut l = c.to_lowercase();
            match (l.next(), l.next()) {
                (Some(x), None) if (x as u32) <= 0xFFFF => x as u32 as u16,
                _ => u,
            }
        }
        None => u,
    }
}

fn is_word(u: u16) -> bool {
    char::from_u32(u as u32).is_some_and(|c| c.is_alphanumeric() || c == '_')
}

/// Every match of `query`, in document order.
pub fn find_all(doc: &Node, query: &str, opts: FindOptions) -> Vec<(usize, usize)> {
    let q: Vec<u16> = query.encode_utf16().map(|u| fold(u, opts.match_case)).collect();
    if q.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (path, start) in textblocks(doc) {
        let Some(n) = node_at(doc, &path) else { continue };
        let units: Vec<u16> = inline::text_units(n.children()).into_iter().map(|u| fold(u, opts.match_case)).collect();
        if units.len() < q.len() {
            continue;
        }
        let mut i = 0;
        while i + q.len() <= units.len() {
            if units[i..i + q.len()] == q[..] {
                let ok = !opts.whole_word
                    || ((i == 0 || !is_word(units[i - 1])) && (i + q.len() == units.len() || !is_word(units[i + q.len()])));
                if ok {
                    out.push((start + i, start + i + q.len()));
                    i += q.len();
                    continue;
                }
            }
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_are_found_per_block_case_insensitively() {
        let d = Node::from_slice(br#"{"content":[{"content":[{"text":"Le chat et le Chat","type":"text"}],"type":"paragraph"},{"content":[{"text":"chatte","type":"text"}],"type":"paragraph"}],"type":"doc"}"#).expect("parses");
        let all = find_all(&d, "chat", FindOptions::default());
        assert_eq!(all, vec![(4, 8), (15, 19), (21, 25)]);
        let whole = find_all(&d, "chat", FindOptions { match_case: false, whole_word: true });
        assert_eq!(whole.len(), 2);
        let case = find_all(&d, "Chat", FindOptions { match_case: true, whole_word: false });
        assert_eq!(case, vec![(15, 19)]);
    }
}
