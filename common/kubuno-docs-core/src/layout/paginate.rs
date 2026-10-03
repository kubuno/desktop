//! `paginateMulti` (`canvas-engine.ts:3552-3885`): the continuous layout sliced into pages.
//!
//! Lines never split; a page takes lines while they fit, then:
//! * a paragraph flagged `breakBefore` (a `pageBreak` node before it, or `pageBreakBefore`) starts a
//!   new page; a section change starts a new page (and the section's geometry);
//! * `keepLines` / `keepNext` push the whole cluster to the next page when it would not fit but
//!   would fit on an empty page (`shouldKeepBreak`);
//! * a page that starts inside a table whose `headerRows`/`headerRepeat` is set repeats the header
//!   rows on top (phantom lines: painted, never hit);
//! * the void of a very tall table row is paginated continuously (`tableVoidFragment`);
//! * a section with several columns fills them left to right before the next page.
//!
//! Each page's lines are rebased to the page (y = 0 at the top of the content box) and keep their
//! global ProseMirror positions (`rebuildPageParas`).

use super::{ColumnBand, DocPx, DocumentLayout, LayoutLine, LayoutParagraph, LayoutTable, PageLayout};

/// The text geometry of a section (`SectionPageGeom`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SectionGeom {
    pub content_h: DocPx,
    pub columns: usize,
    pub col_w: DocPx,
    pub col_gap: DocPx,
}

#[derive(Clone, Copy)]
struct Ref {
    para: usize,
    line: usize,
}

fn keep_run_height(layout: &DocumentLayout, refs: &[Ref], start: usize) -> DocPx {
    let p = refs[start].para;
    let mut h = 0.0;
    let mut j = start;
    while j < refs.len() && refs[j].para == p {
        h += line(layout, refs[j]).height;
        j += 1;
    }
    let pp = &layout.paragraphs[p];
    if pp.keep_next && j < refs.len() && layout.paragraphs[refs[j].para].sec_idx == pp.sec_idx {
        h += line(layout, refs[j]).height;
    }
    h
}

fn should_keep_break(layout: &DocumentLayout, refs: &[Ref], i: usize, remaining: DocPx, content_h: DocPx) -> bool {
    let p = &layout.paragraphs[refs[i].para];
    if !p.keep_lines && !p.keep_next {
        return false;
    }
    let need = keep_run_height(layout, refs, i);
    need > remaining && need <= content_h
}

fn line(layout: &DocumentLayout, r: Ref) -> &LayoutLine {
    &layout.paragraphs[r.para].lines[r.line]
}

fn shifted_table(t: &LayoutTable, start_y: DocPx, x_shift: DocPx, content_h: DocPx) -> LayoutTable {
    let mut out = t.clone();
    out.dy = t.dy - start_y;
    out.dx = t.dx + x_shift;
    // Cells wholly off the page are dropped, so a split table does not paint borders in the
    // margins of the other pages (`:3421-3425`).
    out.cells.retain(|c| {
        let y = c.y + out.dy;
        y + c.height > 0.5 && y < content_h - 0.5
    });
    out
}

/// `rebuildPageParas` (`:3391-3450`).
fn rebuild(layout: &DocumentLayout, taken: &[Ref], start_y: DocPx, x_shift: DocPx, content_h: DocPx) -> Vec<LayoutParagraph> {
    let mut paras: Vec<LayoutParagraph> = Vec::new();
    let mut cur_src: Option<usize> = None;
    for r in taken {
        let src = &layout.paragraphs[r.para];
        let l = &src.lines[r.line];
        let mut shifted = l.clone();
        shifted.y -= start_y;
        shifted.baseline -= start_y;
        if x_shift != 0.0 {
            for sp in &mut shifted.spans {
                sp.x += x_shift;
            }
            shifted.cell_x = shifted.cell_x.map(|x| x + x_shift);
            shifted.caret_x = shifted.caret_x.map(|x| x + x_shift);
            if let Some(img) = shifted.image.as_mut() {
                img.x += x_shift;
            }
        }
        if cur_src == Some(r.para) {
            if let Some(last) = paras.last_mut() {
                last.lines.push(shifted);
            }
        } else {
            paras.push(LayoutParagraph {
                lines: vec![shifted],
                y: src.y - start_y,
                height: src.height,
                pm_start: src.pm_start,
                pm_end: src.pm_end,
                doc_idx: src.doc_idx,
                sec_idx: src.sec_idx,
                break_before: src.break_before,
                keep_lines: src.keep_lines,
                keep_next: src.keep_next,
                table: src.table.as_ref().map(|t| shifted_table(t, start_y, x_shift, content_h)),
            });
            cur_src = Some(r.para);
        }
    }
    paras
}

/// `buildRepeatedHeader` (`:3499-3518`).
fn repeated_header(src: &LayoutParagraph, header_h: DocPx, rows: usize) -> Option<LayoutParagraph> {
    let t = src.table.as_ref()?;
    let r_y0 = t.grid.row_y.first().copied()? + t.dy;
    let mut lines = Vec::new();
    for ln in &src.lines {
        if ln.image.is_some() {
            continue;
        }
        if ln.y < r_y0 - 0.5 || ln.y + ln.height > r_y0 + header_h + 6.0 {
            continue;
        }
        let mut c = ln.clone();
        c.y -= r_y0;
        c.baseline -= r_y0;
        c.phantom = true;
        lines.push(c);
    }
    let mut table = t.clone();
    table.cells.retain(|c| c.row < rows.max(1));
    table.dy = -(t.grid.row_y.first().copied().unwrap_or(0.0));
    Some(LayoutParagraph {
        lines,
        y: 0.0,
        height: header_h,
        pm_start: src.pm_start,
        pm_end: src.pm_start,
        doc_idx: src.doc_idx,
        sec_idx: src.sec_idx,
        table: Some(table),
        ..Default::default()
    })
}

/// `paginateMultiOnce`.
pub fn paginate(layout: &DocumentLayout, geoms: &[SectionGeom]) -> Vec<PageLayout> {
    let default = SectionGeom { content_h: 931.0, columns: 1, col_w: layout.content_w, col_gap: 0.0 };
    let geom_for = |s: usize| -> SectionGeom { geoms.get(s).or(geoms.last()).copied().unwrap_or(default) };
    let mut refs: Vec<Ref> = Vec::new();
    for (pi, p) in layout.paragraphs.iter().enumerate() {
        for li in 0..p.lines.len() {
            refs.push(Ref { para: pi, line: li });
        }
    }
    if refs.is_empty() {
        return vec![PageLayout { height: geom_for(0).content_h, ..Default::default() }];
    }

    let mut pages: Vec<PageLayout> = Vec::new();
    let mut i = 0usize;
    while i < refs.len() {
        let page_sec = layout.paragraphs[refs[i].para].sec_idx;
        let g = geom_for(page_sec);
        let cols = g.columns.max(1);
        let content_h = g.content_h;
        let mut page_start_y = line(layout, refs[i]).y;

        // The void of a table row taller than what is left (`:3780-3800`).
        if let Some(prev) = pages.last() {
            if cols == 1 && content_h > 50.0 {
                let prev_end = prev.start_y + prev.height;
                let para = &layout.paragraphs[refs[i].para];
                if para.table.is_some() && refs[i].line != 0 && para.sec_idx == prev.sec_idx && line(layout, refs[i]).y > prev_end + 1.0 {
                    page_start_y = prev_end;
                    while line(layout, refs[i]).y + line(layout, refs[i]).height - page_start_y > content_h {
                        let mut frag = para.clone();
                        frag.lines.clear();
                        frag.y -= page_start_y;
                        frag.table = para.table.as_ref().map(|t| shifted_table(t, page_start_y, 0.0, content_h));
                        pages.push(PageLayout { paragraphs: vec![frag], start_y: page_start_y, height: content_h, sec_idx: page_sec, columns: Vec::new() });
                        page_start_y += content_h;
                    }
                }
            }
        }

        let mut page_paras: Vec<LayoutParagraph> = Vec::new();
        let mut bands: Vec<ColumnBand> = Vec::new();
        let mut stop = false;
        for col in 0..cols {
            if stop || i >= refs.len() || layout.paragraphs[refs[i].para].sec_idx != page_sec {
                break;
            }
            let col_start_y = if col == 0 { page_start_y } else { line(layout, refs[i]).y };
            // Repeated header rows (`:3810-3822`).
            let mut hdr_h = 0.0;
            let first = refs[i];
            let fp = &layout.paragraphs[first.para];
            let mut hdr_rows = 0;
            if let Some(t) = &fp.table {
                let n = if t.grid.header_rows > 0 { t.grid.header_rows } else if t.grid.header_repeat { 1 } else { 0 };
                if cols == 1 && n > 0 && t.grid.row_y.len() > n && first.line != 0 {
                    let hh = t.grid.row_y[n] - t.grid.row_y[0];
                    if hh > 0.0 && hh < content_h * 0.5 {
                        hdr_h = hh;
                        hdr_rows = n;
                    }
                }
            }
            let capacity = content_h - hdr_h;
            let mut taken: Vec<Ref> = Vec::new();
            let mut last_para: Option<usize> = None;
            while i < refs.len() {
                let p = &layout.paragraphs[refs[i].para];
                if p.sec_idx != page_sec {
                    stop = true;
                    break;
                }
                let starting = last_para != Some(refs[i].para);
                if !taken.is_empty() && starting && p.break_before {
                    stop = true;
                    break;
                }
                let l = line(layout, refs[i]);
                if !taken.is_empty() && starting && should_keep_break(layout, &refs, i, capacity - (l.y - col_start_y), capacity) {
                    break;
                }
                if !taken.is_empty() && (l.y + l.height - col_start_y) > capacity {
                    break;
                }
                last_para = Some(refs[i].para);
                taken.push(refs[i]);
                i += 1;
            }
            let x_shift = col as f32 * (g.col_w + g.col_gap);
            if !taken.is_empty() {
                bands.push(ColumnBand { start_y: col_start_y - hdr_h, x_shift });
            }
            let mut rebuilt = rebuild(layout, &taken, col_start_y - hdr_h, x_shift, content_h);
            if hdr_h > 0.0 {
                if let Some(h) = repeated_header(fp, hdr_h, hdr_rows) {
                    rebuilt.insert(0, h);
                }
            }
            page_paras.extend(rebuilt);
        }
        pages.push(PageLayout { paragraphs: page_paras, start_y: page_start_y, height: content_h, sec_idx: page_sec, columns: bands });
    }
    pages
}

/// The page holding the line of `pos` (the first line whose range contains it, with the
/// wrap-boundary affinity of [`super::caret`]), and that page's column band for it.
pub fn page_of_pos(pages: &[PageLayout], pos: usize) -> Option<usize> {
    for (pi, page) in pages.iter().enumerate() {
        for p in &page.paragraphs {
            for l in &p.lines {
                if !l.phantom && !l.no_caret && pos >= l.pm_start && pos <= l.pm_end {
                    return Some(pi);
                }
            }
        }
    }
    None
}

/// The page whose band contains the continuous ordinate `y` (single-column pages: the band
/// `[start_y, next page's start_y)`).
pub fn page_of_y(pages: &[PageLayout], y: DocPx) -> usize {
    let mut best = 0;
    for (i, p) in pages.iter().enumerate() {
        if p.start_y <= y + 0.01 {
            best = i;
        } else {
            break;
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::flow::Flow;
    use crate::measure::FixedMeasure;
    use crate::model::Node;

    fn many(n: usize, extra: &str) -> Node {
        let mut parts = Vec::new();
        for i in 0..n {
            parts.push(format!(r#"{{"content":[{{"text":"paragraph {i}","type":"text"}}],"type":"paragraph"}}"#));
            if !extra.is_empty() && i == n / 2 {
                parts.push(extra.to_string());
            }
        }
        Node::from_slice(format!(r#"{{"content":[{}],"type":"doc"}}"#, parts.join(",")).as_bytes()).expect("parses")
    }

    fn geom() -> SectionGeom {
        SectionGeom { content_h: 931.0, columns: 1, col_w: 602.0, col_gap: 0.0 }
    }

    #[test]
    fn lines_fill_pages_and_never_overflow() {
        let d = many(120, "");
        let l = Flow::new().layout(&d, &[], &|_| 602.0, &FixedMeasure);
        let pages = paginate(&l, &[geom()]);
        assert!(pages.len() >= 3, "{} pages", pages.len());
        for p in &pages {
            for para in &p.paragraphs {
                for ln in &para.lines {
                    assert!(ln.y >= -0.01 && ln.y + ln.height <= 931.0 + 0.01);
                }
            }
        }
        // Every line is on exactly one page.
        let n: usize = pages.iter().map(|p| p.paragraphs.iter().map(|q| q.lines.len()).sum::<usize>()).sum();
        assert_eq!(n, l.paragraphs.iter().map(|p| p.lines.len()).sum::<usize>());
    }

    #[test]
    fn a_page_break_starts_a_new_page() {
        let d = many(4, r#"{"type":"pageBreak"}"#);
        let l = Flow::new().layout(&d, &[], &|_| 602.0, &FixedMeasure);
        let pages = paginate(&l, &[geom()]);
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[1].paragraphs[0].lines[0].y, 0.0);
    }

    #[test]
    fn two_columns_fill_side_by_side() {
        let d = many(120, "");
        let l = Flow::new().layout(&d, &[], &|_| 283.0, &FixedMeasure);
        let g = SectionGeom { content_h: 931.0, columns: 2, col_w: 283.0, col_gap: 36.0 };
        let pages = paginate(&l, &[g]);
        assert_eq!(pages[0].columns.len(), 2);
        assert!(pages[0].columns[1].x_shift > 300.0);
    }

    #[test]
    fn the_page_of_a_position_is_found() {
        let d = many(120, "");
        let l = Flow::new().layout(&d, &[], &|_| 602.0, &FixedMeasure);
        let pages = paginate(&l, &[geom()]);
        let last = l.paragraphs.last().and_then(|p| p.lines.last()).map(|l| l.pm_end).unwrap_or(0);
        assert_eq!(page_of_pos(&pages, last), Some(pages.len() - 1));
        assert_eq!(page_of_pos(&pages, 1), Some(0));
    }
}
