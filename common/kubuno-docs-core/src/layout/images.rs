//! Images.
//!
//! # The contract, and the two traps
//!
//! 1. **`image.alt` is not alt text.** It is a four-way discriminated union,
//!    and one of its arms (`kbtextrich:`) holds an entire ProseMirror
//!    sub-document — a rich text box, whose `src` is only a blank frame SVG.
//!    Trimming, re-encoding or length-capping `alt` deletes user prose. Nothing
//!    here may rewrite it; read it and leave the bytes alone.
//! 2. **Image bytes travel as base64 data URIs inside the document JSON**, and
//!    the server's `PATCH` is capped at 2 MiB. One phone photo makes a document
//!    permanently unsavable. An image inserted by this app is therefore
//!    downscaled and re-encoded at insert time — max 1 600 px on the long edge,
//!    400 KiB encoded — and the user is told then, not at save time.
//!
//! # What this module is, and what it is not
//!
//! Geometry and arithmetic only. No pixels are decoded here: natural sizes
//! arrive from the caller (WIC lands later) and [`display_size`] takes them as
//! an `Option` precisely so that the not-yet-decoded case is a value rather
//! than a guess. The web has the same two phases — it lays out at 320 × 200
//! until `naturalWidth` is known, then relayouts on `kubuno-image-loaded`
//! (`canvas-engine.ts:1864-1865`, `:456-457`) — and a port that skips the first
//! phase paginates differently from the browser for the first frame.
//!
//! # Why the numbers are what they are
//!
//! * [`img_aabb`] is a line-for-line port of `imgAABB`
//!   (`canvas-engine.ts:435-442`). It is not decoration: the line breaker uses
//!   its `w` as an inline image's token width (`:1913`), its `h` as the line's
//!   height contribution (`:2050`, `:1874-1875`), and the float registration
//!   builds the text-exclusion band from it centred on the drawing centre
//!   (`:1271-1276`). A rotated image that reserves its unrotated box overlaps
//!   the text around it.
//! * A **floating** image reserves ZERO height in the flow (`:1880`, `:1893`)
//!   and its paragraph's spacing is forced to 0 at parse (`:749-751`). This is
//!   deliberate and documented in the web source: a non-zero height injected
//!   ~30 px of emptiness around every anchored object, including the
//!   "in front of text" ones which by definition occupy nothing.
//! * The insert budget's ceilings come from `31-CORRECTIONS.md` §C7: the office
//!   module declares no `DefaultBodyLimit`, so its `Json<UpdateDocumentDto>`
//!   extractor runs on axum's 2 MiB default, and `content_json` is a nested
//!   JSON object inside that same body (`office/src/models/document.rs:77`).
//!   Base64 costs +33 % over the raw bytes, so one 3 MB phone photo is already
//!   terminal — and a 413 on that path is not retryable.

#![allow(dead_code)]

use serde_json::Value;

use super::DocPx;
use crate::model::Node;

// ───────────────────────────── placement and geometry ─────────────────────────

/// How an image participates in the layout.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Placement {
    /// A block of its own.
    #[default]
    Block,
    /// Inside a line, contributing its height to that line's box.
    Inline,
    /// Anchored, with text flowing around it.
    Floating,
}

impl Placement {
    /// Whether the object is anchored — the case that reserves no height at all
    /// in the text flow (`canvas-engine.ts:1880`).
    pub fn is_floating(self) -> bool {
        matches!(self, Placement::Floating)
    }
}

/// The six `wrap` values that make a block image an anchored object.
///
/// `FLOATING_WRAPS` (`canvas-engine.ts:422`). The seventh value, `inline`, is
/// the default and is the only one that is a real full-width block.
pub const FLOATING_WRAPS: [&str; 6] =
    ["square", "tight", "through", "topBottom", "behind", "front"];

/// Whether a `wrap` attribute names one of the anchored modes.
pub fn is_floating_wrap(wrap: &str) -> bool {
    FLOATING_WRAPS.contains(&wrap)
}

/// What the layout engine needs to know about one image.
#[derive(Clone, Debug, Default)]
pub struct Image {
    pub placement: Placement,
    pub width:     DocPx,
    pub height:    DocPx,
    /// Rotation in degrees, which changes the box the image occupies.
    pub rotation:  f32,
}

impl Image {
    /// The axis-aligned box this image occupies at the given display size.
    pub fn aabb(&self, display_w: DocPx, display_h: DocPx) -> Aabb {
        img_aabb(display_w, display_h, self.rotation)
    }

    /// The height this image reserves in the text flow.
    ///
    /// Zero for an anchored object; the rotated box's height otherwise.
    pub fn reserved_height(&self, display_w: DocPx, display_h: DocPx) -> DocPx {
        if self.placement.is_floating() {
            0.0
        } else {
            self.aabb(display_w, display_h).h
        }
    }

    /// The width an inline image advances the caret by — the rotated box's
    /// width, which is what the line breaker uses as the token width
    /// (`canvas-engine.ts:1913`).
    pub fn token_width(&self, display_w: DocPx, display_h: DocPx) -> DocPx {
        self.aabb(display_w, display_h).w
    }
}

/// An axis-aligned bounding box's size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub w: DocPx,
    pub h: DocPx,
}

/// The box a rotated image occupies, in document pixels.
///
/// A port of `imgAABB` (`canvas-engine.ts:435-442`), including its early exit:
/// the web writes `if (!rot) return { w, h }`, which is taken for `0`, `-0` and
/// `NaN` alike, so those three cases return the unrotated box rather than a box
/// full of `NaN`.
pub fn img_aabb(w: DocPx, h: DocPx, rot: f32) -> Aabb {
    if rot == 0.0 || rot.is_nan() {
        return Aabb { w, h };
    }
    let r = rot.to_radians();
    Aabb {
        w: (w * r.cos()).abs() + (h * r.sin()).abs(),
        h: (w * r.sin()).abs() + (h * r.cos()).abs(),
    }
}

/// The size the web lays an image out at before its pixels are known
/// (`canvas-engine.ts:1864-1865`).
pub const NATURAL_FALLBACK_W: DocPx = 320.0;
pub const NATURAL_FALLBACK_H: DocPx = 200.0;

/// The display size of an image, given its natural size (once decoded) and the
/// column it sits in.
///
/// A port of `layoutParagraph`'s image branch (`canvas-engine.ts:1866-1871`),
/// whose rules are each load-bearing:
///
/// * an explicit `width` is clamped to the content width, an absent one falls
///   back to the natural width **also clamped**;
/// * an explicit `height` is a **free stretch** — the aspect ratio is not
///   enforced, because Word does not enforce it either;
/// * a non-finite or non-positive result falls back a second time, which is how
///   a hand-edited `"width": -1` still renders.
///
/// An **inline** image is different and is handled here too: the web never
/// consults the natural size for `inlineImage`, it takes `max(1, w)` and
/// `max(1, h)` straight from the attributes (`canvas-engine.ts:858-859`). An
/// inline image with no explicit size is therefore 1 × 1 in the browser, and
/// matching that is parity even though it looks like a bug.
pub fn display_size(
    image: &Image,
    natural: Option<(DocPx, DocPx)>,
    content_width: DocPx,
) -> (DocPx, DocPx) {
    if image.placement == Placement::Inline {
        return (image.width.max(1.0), image.height.max(1.0));
    }

    let (nat_w, nat_h) = natural.unwrap_or((0.0, 0.0));
    // `(imgReady(img) ? img.naturalWidth : 0) || 320` — a zero or non-finite
    // natural size is the not-yet-decoded case, not a zero-sized image.
    let nat_w = if nat_w.is_finite() && nat_w > 0.0 { nat_w } else { NATURAL_FALLBACK_W };
    let nat_h = if nat_h.is_finite() && nat_h > 0.0 { nat_h } else { NATURAL_FALLBACK_H };

    let mut disp_w = if image.width != 0.0 {
        image.width.min(content_width)
    } else {
        nat_w.min(content_width)
    };
    let mut disp_h = if image.height != 0.0 { image.height } else { nat_h * (disp_w / nat_w) };
    if !disp_w.is_finite() || disp_w <= 0.0 {
        disp_w = nat_w.min(content_width);
    }
    if !disp_h.is_finite() || disp_h <= 0.0 {
        disp_h = nat_h * (disp_w / nat_w);
    }
    (disp_w, disp_h)
}

// ───────────────────────────────── reading a node ─────────────────────────────

/// Reads an `image` node's geometry. Never rewrites any attribute.
///
/// Returns `None` for anything that is not an `image` or an `inlineImage`, so
/// the caller can hand it every block in the body.
pub fn describe(node: &Node) -> Option<Image> {
    let inline = match node.node_type()? {
        "image" => false,
        "inlineImage" => true,
        _ => return None,
    };
    let attrs = attrs_of(node);
    let attrs = attrs.as_ref();

    let placement = if inline {
        Placement::Inline
    } else {
        // `(a.wrap as string) || 'inline'` — absent, null or empty all mean
        // inline, and an unknown value is not floating either.
        let wrap = attrs.and_then(|a| a.get("wrap")).and_then(Value::as_str).unwrap_or("inline");
        if is_floating_wrap(wrap) { Placement::Floating } else { Placement::Block }
    };

    Some(Image {
        placement,
        width: number(attrs, "width"),
        height: number(attrs, "height"),
        rotation: number(attrs, "rotation"),
    })
}

/// The node's `attrs` object, if it has one.
fn attrs_of(node: &Node) -> Option<Value> {
    serde_json::from_str(node.raw("attrs")?.get()).ok()
}

/// One numeric attribute, with the web's own coercion.
///
/// The web writes `Number(a.width) || 0`, which accepts a numeric string (the
/// DOCX importer has written both shapes) and folds `NaN`, `null`, absent and
/// `0` into the single "unset" value 0.
fn number(attrs: Option<&Value>, key: &str) -> f32 {
    let Some(v) = attrs.and_then(|a| a.get(key)) else { return 0.0 };
    let n = match v {
        Value::Number(n) => n.as_f64().map_or(0.0, |x| x as f32),
        Value::String(s) => s.trim().parse::<f32>().unwrap_or(0.0),
        Value::Bool(b) => f32::from(u8::from(*b)),
        _ => 0.0,
    };
    if n.is_finite() { n } else { 0.0 }
}

// ─────────────────────────────────── the `alt` union ──────────────────────────

/// What an `image.alt` value actually is.
///
/// The prefixes are declared at `DocumentEditorPage.tsx:1385-1388` and read at
/// `canvas-engine.ts:303` (`kbshape:`), `:567` (`kbtextrich:`),
/// `DocumentEditorPage.tsx:1453` (`kbtext:`) and `:6849` (`kbenvelope:`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AltKind {
    /// A parametric vector shape. The SVG `src` is regenerated from the alt
    /// when it is missing, so an empty `src` here is the NORMAL state and must
    /// not be treated as a broken image.
    Shape,
    /// A legacy single-string text box.
    Text,
    /// A complete ProseMirror document, URI-encoded. The `src` is only a blank
    /// frame; the prose lives here and nowhere else.
    RichText,
    /// A pasted cross-module JSON envelope.
    Envelope,
    /// Ordinary alt text, or no `alt` at all.
    Plain,
}

impl AltKind {
    /// The prefix that selects this arm, or `None` for plain alt text.
    pub fn prefix(self) -> Option<&'static str> {
        match self {
            AltKind::Shape => Some("kbshape:"),
            AltKind::Text => Some("kbtext:"),
            AltKind::RichText => Some("kbtextrich:"),
            AltKind::Envelope => Some("kbenvelope:"),
            AltKind::Plain => None,
        }
    }

    /// Whether this arm carries machine payload rather than prose for a screen
    /// reader — i.e. whether showing it in an "alt text" field would be wrong.
    pub fn is_payload(self) -> bool {
        self != AltKind::Plain
    }
}

/// Classifies an `alt` value. Borrows; rewrites nothing.
///
/// `kbtextrich:` is tested before `kbtext:` even though the two cannot actually
/// collide (`kbtextrich:` has no colon at index 6). The order is written down
/// because the cost of getting it wrong — a rich text box read as a legacy
/// one-line label, and re-saved as one — is a silent deletion of everything the
/// user typed into the box.
pub fn classify_alt(alt: &str) -> AltKind {
    for kind in [AltKind::RichText, AltKind::Text, AltKind::Shape, AltKind::Envelope] {
        if let Some(prefix) = kind.prefix() {
            if alt.starts_with(prefix) {
                return kind;
            }
        }
    }
    AltKind::Plain
}

/// The payload after the prefix, as a **slice of the original string**.
///
/// Borrowed on purpose: a payload that is never copied is a payload that can
/// never be trimmed, normalised or percent-decoded on the way past.
pub fn alt_payload(alt: &str) -> Option<&str> {
    classify_alt(alt).prefix().and_then(|prefix| alt.strip_prefix(prefix))
}

/// An image node's `alt`, decoded from JSON and otherwise untouched.
///
/// Returns the string the document carries, escapes resolved — the same value
/// the web sees as `String(a.alt)`. `null` and absent both give `None`, which
/// the web also collapses (`a.alt != null ? String(a.alt) : undefined`).
pub fn alt_of(node: &Node) -> Option<String> {
    match attrs_of(node)?.get("alt")? {
        Value::String(s) => Some(s.clone()),
        Value::Null => None,
        // A non-string `alt` is stringified by `String()` in the web. Anything
        // we produced here would be a guess at its spelling, so report it as
        // absent rather than invent one; the bytes are untouched either way.
        _ => None,
    }
}

/// Classifies an image node's `alt` directly. `Plain` when there is no `alt`.
pub fn classify(node: &Node) -> AltKind {
    alt_of(node).as_deref().map_or(AltKind::Plain, classify_alt)
}

// ──────────────────────────────── the insert budget ───────────────────────────

/// The maximum long edge, in pixels, of an image this app inserts.
pub const MAX_LONG_EDGE: u32 = 1600;
/// The maximum encoded size, in bytes, of an image this app inserts.
pub const MAX_ENCODED_BYTES: usize = 400 * 1024;
/// The JPEG quality used when re-encoding photographic content (§C7).
pub const JPEG_QUALITY: u8 = 82;

/// The server's `PATCH` ceiling: axum's `DefaultBodyLimit`, which the office
/// module never overrides (§C7). Measured, not assumed, on day one of slice 4 —
/// until then this is axum's documented default.
pub const PATCH_LIMIT_BYTES: usize = 2 * 1024 * 1024;
/// Where the status bar turns amber (§C7): 1.5 MiB.
pub const WARN_BYTES: usize = 3 * 1024 * 1024 / 2;

/// How an inserted image should be re-encoded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// Kept lossless because the source has an alpha channel; re-encoding it as
    /// JPEG would fill the transparency with black.
    Png,
    Jpeg { quality: u8 },
}

/// What to do with an image the user is inserting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InsertPlan {
    /// Target pixel width and height.
    pub width:  u32,
    pub height: u32,
    /// Whether the source had to be shrunk to meet [`MAX_LONG_EDGE`].
    pub resized: bool,
    pub encoding: Encoding,
    /// The ceiling the encoded result must meet.
    pub max_encoded_bytes: usize,
}

/// Plans an insert: the target pixel size and the encoding to use.
///
/// Decoding and encoding happen elsewhere (WIC). This is the whole of the
/// policy, so that the policy has one home and can be tested without pixels.
pub fn plan_insert(width: u32, height: u32, has_alpha: bool) -> InsertPlan {
    let (w, h) = fit_long_edge(width, height);
    InsertPlan {
        width: w,
        height: h,
        resized: (w, h) != (width, height),
        encoding: if has_alpha { Encoding::Png } else { Encoding::Jpeg { quality: JPEG_QUALITY } },
        max_encoded_bytes: MAX_ENCODED_BYTES,
    }
}

/// Shrinks a size so its long edge is at most [`MAX_LONG_EDGE`], keeping the
/// aspect ratio. Never upscales, and never produces a zero edge — a 1 px edge
/// rounded down to 0 is an image that decodes to nothing.
pub fn fit_long_edge(width: u32, height: u32) -> (u32, u32) {
    let long = width.max(height);
    if long <= MAX_LONG_EDGE || width == 0 || height == 0 {
        return (width, height);
    }
    let scale = f64::from(MAX_LONG_EDGE) / f64::from(long);
    let w = ((f64::from(width) * scale).round() as u32).max(1);
    let h = ((f64::from(height) * scale).round() as u32).max(1);
    (w, h)
}

/// The length of `n` bytes once base64-encoded, padding included.
pub fn base64_len(raw_bytes: usize) -> usize {
    // 4 characters per 3-byte group, the last group padded out.
    raw_bytes.div_ceil(3) * 4
}

/// The length of the `src` string a data URI of `raw_bytes` bytes would occupy
/// inside the document JSON.
///
/// Base64's alphabet needs no JSON escaping, so the string's length in the
/// serialised document is its character count plus the two quotes; those two
/// are the caller's business, not this function's.
pub fn data_uri_len(mime: &str, raw_bytes: usize) -> usize {
    "data:".len() + mime.len() + ";base64,".len() + base64_len(raw_bytes)
}

/// Whether an encoded image meets the per-image ceiling.
pub fn fits_insert_budget(encoded_bytes: usize) -> bool {
    encoded_bytes <= MAX_ENCODED_BYTES
}

// ───────────────────────────── the document's size ────────────────────────────

/// How close a document is to being unsavable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SizeStatus {
    /// Comfortably under the limit.
    Ok,
    /// Past [`WARN_BYTES`] — the status bar turns amber.
    Warn,
    /// Past [`PATCH_LIMIT_BYTES`]: the next save returns 413, which is terminal.
    Over,
}

/// What a document currently costs on the wire.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    /// The serialised `content_json`, in bytes. This is the number the limit
    /// applies to (plus the few dozen bytes of the PATCH envelope around it).
    pub total_bytes: usize,
    /// How much of that is base64 image data, summed over the `src` attributes
    /// that are data URIs.
    ///
    /// Body only: header and footer subtrees live in the envelope root, which
    /// the model deliberately keeps as raw bytes and never walks. They are
    /// counted in `total_bytes` — the number that decides the save — but not
    /// attributed here.
    pub image_bytes: usize,
    /// How many image nodes carry their bytes inline.
    pub image_count: usize,
}

impl Usage {
    pub fn status(self) -> SizeStatus {
        if self.total_bytes > PATCH_LIMIT_BYTES {
            SizeStatus::Over
        } else if self.total_bytes >= WARN_BYTES {
            SizeStatus::Warn
        } else {
            SizeStatus::Ok
        }
    }

    /// Bytes still available before the save starts failing. Saturates at 0.
    pub fn headroom(self) -> usize {
        PATCH_LIMIT_BYTES.saturating_sub(self.total_bytes)
    }

    /// Whether adding `encoded_bytes` more would put the document over.
    ///
    /// Asked BEFORE the insert, which is the whole point: a 413 at save time
    /// arrives after the user has typed another page.
    pub fn would_exceed(self, encoded_bytes: usize) -> bool {
        encoded_bytes > self.headroom()
    }
}

/// Measures a document: what it serialises to, and how much of that is images.
///
/// Serialises the whole document, so it belongs on the structural debounce
/// rather than on every keystroke.
pub fn measure(document: &crate::model::Document) -> Result<Usage, crate::model::Error> {
    let total_bytes = document.to_vec()?.len();
    let mut usage = Usage { total_bytes, ..Usage::default() };
    for block in document.blocks() {
        walk(block, &mut usage);
    }
    Ok(usage)
}

/// Adds one node's inline image bytes to `usage`, then its children's.
fn walk(node: &Node, usage: &mut Usage) {
    if matches!(node.node_type(), Some("image") | Some("inlineImage")) {
        if let Some(len) = inline_src_len(node) {
            usage.image_bytes += len;
            usage.image_count += 1;
        }
    }
    for child in node.children() {
        walk(child, usage);
    }
}

/// The length of a node's `src` when — and only when — it is a data URI.
///
/// A plain URL costs its own length and nothing more; it is the data URIs that
/// make a document unsavable, so they are what the status bar reports.
fn inline_src_len(node: &Node) -> Option<usize> {
    let src = attrs_of(node)?.get("src")?.as_str()?.to_owned();
    is_data_uri(&src).then_some(src.len())
}

/// Whether a `src` carries its bytes inline.
pub fn is_data_uri(src: &str) -> bool {
    src.starts_with("data:")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(json: &str) -> Node {
        Node::from_slice(json.as_bytes()).expect("fixture must parse")
    }

    // ── imgAABB ───────────────────────────────────────────────────────────────

    #[test]
    fn an_unrotated_image_occupies_its_own_box() {
        assert_eq!(img_aabb(200.0, 100.0, 0.0), Aabb { w: 200.0, h: 100.0 });
    }

    #[test]
    fn a_quarter_turn_swaps_the_box() {
        // The whole reason imgAABB exists: at 90° a wide image is a tall one,
        // and a line that reserved 100 px would clip 200 px of picture.
        let ab = img_aabb(200.0, 100.0, 90.0);
        assert!((ab.w - 100.0).abs() < 1e-3, "w was {}", ab.w);
        assert!((ab.h - 200.0).abs() < 1e-3, "h was {}", ab.h);
    }

    #[test]
    fn a_forty_five_degree_turn_grows_the_box_by_root_two() {
        let ab = img_aabb(100.0, 100.0, 45.0);
        let expected = 100.0 * std::f32::consts::SQRT_2;
        assert!((ab.w - expected).abs() < 1e-3, "w was {}", ab.w);
        assert!((ab.h - expected).abs() < 1e-3, "h was {}", ab.h);
    }

    #[test]
    fn a_half_turn_is_the_same_box_again() {
        let ab = img_aabb(200.0, 100.0, 180.0);
        assert!((ab.w - 200.0).abs() < 1e-3, "w was {}", ab.w);
        assert!((ab.h - 100.0).abs() < 1e-3, "h was {}", ab.h);
    }

    #[test]
    fn a_negative_rotation_gives_the_same_box_as_its_mirror() {
        let a = img_aabb(180.0, 60.0, -30.0);
        let b = img_aabb(180.0, 60.0, 30.0);
        assert!((a.w - b.w).abs() < 1e-3 && (a.h - b.h).abs() < 1e-3);
    }

    #[test]
    fn a_nan_rotation_does_not_produce_a_nan_box() {
        // The web's `if (!rot)` is taken for NaN, so the unrotated box comes
        // back. A NaN width here would poison the whole page's pagination.
        let ab = img_aabb(200.0, 100.0, f32::NAN);
        assert_eq!(ab, Aabb { w: 200.0, h: 100.0 });
    }

    // ── the alt union ─────────────────────────────────────────────────────────

    #[test]
    fn each_prefix_selects_its_own_arm() {
        assert_eq!(classify_alt("kbshape:rect|1|2"), AltKind::Shape);
        assert_eq!(classify_alt("kbtext:Hello"), AltKind::Text);
        assert_eq!(classify_alt("kbtextrich:%7B%7D"), AltKind::RichText);
        assert_eq!(classify_alt("kbenvelope:%7B%7D"), AltKind::Envelope);
        assert_eq!(classify_alt("A photo of a cat"), AltKind::Plain);
        assert_eq!(classify_alt(""), AltKind::Plain);
    }

    #[test]
    fn a_rich_text_box_is_never_read_as_a_legacy_text_box() {
        // `kbtextrich:` starts with the letters of `kbtext`. Reading it as the
        // legacy single-string arm would replace a sub-document with a label.
        assert_eq!(classify_alt("kbtextrich:%7B%22type%22%3A%22doc%22%7D"), AltKind::RichText);
        assert_ne!(classify_alt("kbtextrich:x"), AltKind::Text);
    }

    #[test]
    fn a_rich_text_payload_survives_classification_byte_identically() {
        // The trap this module exists for: `alt` carries the entire prose of a
        // text box. Classifying must be a pure read — same bytes, and in fact
        // the very same memory, not a copy that something could normalise.
        let alt = "kbtextrich:%7B%22type%22%3A%22doc%22%2C%22content%22%3A%5B%7B%22type%22%3A%22\
                   paragraph%22%2C%22content%22%3A%5B%7B%22type%22%3A%22text%22%2C%22text%22%3A%22\
                   Bonjour%20%20%20%22%7D%5D%7D%5D%7D%20";
        let kind = classify_alt(alt);
        assert_eq!(kind, AltKind::RichText);

        let payload = alt_payload(alt).expect("a rich text box has a payload");
        let prefix = kind.prefix().expect("the arm has a prefix");
        assert_eq!(format!("{prefix}{payload}"), alt);
        // Trailing spaces and percent-encoding are part of the payload: nothing
        // trimmed it, nothing decoded it.
        assert!(payload.ends_with("%20"));
        assert!(payload.contains("%20%20%20"));
        // Borrowed, not rebuilt.
        assert!(std::ptr::eq(payload.as_ptr(), alt.as_bytes()[prefix.len()..].as_ptr()));
    }

    #[test]
    fn plain_alt_text_has_no_payload_to_strip() {
        assert_eq!(alt_payload("A photo of a cat"), None);
        assert!(!classify_alt("A photo of a cat").is_payload());
        assert!(classify_alt("kbshape:x").is_payload());
    }

    #[test]
    fn a_shape_with_no_src_is_not_a_broken_image() {
        // A `kbshape:` node regenerates its SVG from the alt, so an empty src
        // is its normal state and must not be reported as missing.
        let n = node(r#"{"attrs":{"alt":"kbshape:rect|0.25","src":""},"type":"image"}"#);
        assert_eq!(classify(&n), AltKind::Shape);
        assert!(describe(&n).is_some());
    }

    #[test]
    fn reading_an_image_node_leaves_its_bytes_untouched() {
        let src = r#"{"attrs":{"alt":"kbtextrich:%7B%22type%22%3A%22doc%22%7D","height":0,"rotation":30,"src":"data:image/svg+xml,frame","width":240},"type":"image"}"#;
        let n = node(src);
        let image = describe(&n).expect("an image node describes");
        assert_eq!(image.width, 240.0);
        assert_eq!(image.rotation, 30.0);
        assert_eq!(classify(&n), AltKind::RichText);
        assert_eq!(alt_of(&n).as_deref(), Some("kbtextrich:%7B%22type%22%3A%22doc%22%7D"));
        // The node round-trips exactly as it arrived.
        let out = String::from_utf8(n.to_vec().expect("serialises")).expect("utf-8");
        assert_eq!(out, src);
    }

    #[test]
    fn an_absent_or_null_alt_reads_as_no_alt() {
        assert_eq!(alt_of(&node(r#"{"attrs":{"src":"x"},"type":"image"}"#)), None);
        assert_eq!(alt_of(&node(r#"{"attrs":{"alt":null,"src":"x"},"type":"image"}"#)), None);
        assert_eq!(classify(&node(r#"{"attrs":null,"type":"image"}"#)), AltKind::Plain);
    }

    // ── describe ──────────────────────────────────────────────────────────────

    #[test]
    fn only_image_nodes_describe() {
        assert!(describe(&node(r#"{"type":"paragraph"}"#)).is_none());
        assert!(describe(&node(r#"{"type":"table"}"#)).is_none());
        assert!(describe(&node(r#"{"text":"x","type":"text"}"#)).is_none());
        assert!(describe(&node(r#"{"type":"image"}"#)).is_some());
        assert!(describe(&node(r#"{"type":"inlineImage"}"#)).is_some());
    }

    #[test]
    fn the_six_anchored_wraps_are_floating_and_inline_is_not() {
        for wrap in FLOATING_WRAPS {
            let n = node(&format!(r#"{{"attrs":{{"wrap":"{wrap}"}},"type":"image"}}"#));
            assert_eq!(
                describe(&n).expect("describes").placement,
                Placement::Floating,
                "{wrap} must be anchored"
            );
        }
        let n = node(r#"{"attrs":{"wrap":"inline"},"type":"image"}"#);
        assert_eq!(describe(&n).expect("describes").placement, Placement::Block);
    }

    #[test]
    fn an_absent_or_unknown_wrap_is_a_block_not_a_float() {
        for json in [
            r#"{"type":"image"}"#,
            r#"{"attrs":{},"type":"image"}"#,
            r#"{"attrs":{"wrap":null},"type":"image"}"#,
            r#"{"attrs":{"wrap":"somethingFromNextYear"},"type":"image"}"#,
        ] {
            assert_eq!(describe(&node(json)).expect("describes").placement, Placement::Block);
        }
    }

    #[test]
    fn a_numeric_attribute_written_as_a_string_still_reads_as_a_number() {
        // Both serialisers are in the wild and the web coerces with `Number()`.
        let n = node(r#"{"attrs":{"height":"90","rotation":"45","width":"120"},"type":"image"}"#);
        let image = describe(&n).expect("describes");
        assert_eq!((image.width, image.height, image.rotation), (120.0, 90.0, 45.0));
    }

    #[test]
    fn garbage_geometry_reads_as_unset_rather_than_as_nan() {
        let n = node(r#"{"attrs":{"height":{},"rotation":"soon","width":"abc"},"type":"image"}"#);
        let image = describe(&n).expect("describes");
        assert_eq!((image.width, image.height, image.rotation), (0.0, 0.0, 0.0));
    }

    // ── display size and reserved height ──────────────────────────────────────

    #[test]
    fn an_undecoded_image_lays_out_at_the_webs_fallback_size() {
        // The browser uses 320x200 until the pixels arrive, then relayouts. A
        // port that guesses another number paginates differently for one frame.
        let image = Image::default();
        assert_eq!(display_size(&image, None, 600.0), (320.0, 200.0));
    }

    #[test]
    fn an_explicit_width_is_clamped_to_the_column_and_the_height_follows() {
        let image = Image { width: 2000.0, ..Image::default() };
        let (w, h) = display_size(&image, Some((800.0, 400.0)), 602.0);
        assert_eq!(w, 602.0);
        assert!((h - 301.0).abs() < 1e-3, "h was {h}");
    }

    #[test]
    fn an_explicit_height_stretches_freely() {
        // Word does not enforce the aspect ratio and neither does the web.
        let image = Image { width: 200.0, height: 40.0, ..Image::default() };
        assert_eq!(display_size(&image, Some((800.0, 400.0)), 600.0), (200.0, 40.0));
    }

    #[test]
    fn a_negative_size_falls_back_instead_of_vanishing() {
        let image = Image { width: -10.0, height: -10.0, ..Image::default() };
        let (w, h) = display_size(&image, Some((400.0, 200.0)), 600.0);
        assert_eq!(w, 400.0);
        assert!((h - 200.0).abs() < 1e-3, "h was {h}");
    }

    #[test]
    fn an_inline_image_never_falls_back_to_the_natural_size() {
        // `Math.max(1, Number(a.width) || 0)` — the inline branch does not look
        // at the decoded pixels at all.
        let image = Image { placement: Placement::Inline, ..Image::default() };
        assert_eq!(display_size(&image, Some((800.0, 600.0)), 600.0), (1.0, 1.0));
        let sized = Image { placement: Placement::Inline, width: 24.0, height: 24.0, rotation: 0.0 };
        assert_eq!(display_size(&sized, None, 600.0), (24.0, 24.0));
    }

    #[test]
    fn an_anchored_image_reserves_no_height_in_the_flow() {
        // A non-zero height here injects ~30 px of emptiness around every
        // anchored object, including "in front of text" ones that occupy none.
        let float = Image { placement: Placement::Floating, ..Image::default() };
        assert_eq!(float.reserved_height(300.0, 200.0), 0.0);
        let block = Image::default();
        assert_eq!(block.reserved_height(300.0, 200.0), 200.0);
    }

    #[test]
    fn a_rotated_block_reserves_its_rotated_height() {
        let image = Image { rotation: 90.0, ..Image::default() };
        let reserved = image.reserved_height(300.0, 100.0);
        assert!((reserved - 300.0).abs() < 1e-3, "reserved {reserved}");
        let width = image.token_width(300.0, 100.0);
        assert!((width - 100.0).abs() < 1e-3, "width {width}");
    }

    // ── the insert budget ─────────────────────────────────────────────────────

    #[test]
    fn a_phone_photo_is_shrunk_to_the_long_edge_keeping_its_ratio() {
        assert_eq!(fit_long_edge(4032, 3024), (1600, 1200));
        assert_eq!(fit_long_edge(3024, 4032), (1200, 1600));
    }

    #[test]
    fn an_image_already_within_the_budget_is_not_upscaled() {
        assert_eq!(fit_long_edge(1600, 1200), (1600, 1200));
        assert_eq!(fit_long_edge(100, 50), (100, 50));
        assert_eq!(fit_long_edge(0, 0), (0, 0));
    }

    #[test]
    fn a_very_wide_banner_keeps_at_least_one_pixel_of_height() {
        // 8000x1 scaled by 0.2 rounds to zero, and a zero-height bitmap decodes
        // to nothing at all.
        let (w, h) = fit_long_edge(8000, 1);
        assert_eq!(w, MAX_LONG_EDGE);
        assert!(h >= 1, "height was {h}");
    }

    #[test]
    fn transparency_is_kept_as_png_and_photographs_become_jpeg() {
        let alpha = plan_insert(4032, 3024, true);
        assert_eq!(alpha.encoding, Encoding::Png);
        assert!(alpha.resized);
        assert_eq!((alpha.width, alpha.height), (1600, 1200));

        let photo = plan_insert(1200, 900, false);
        assert_eq!(photo.encoding, Encoding::Jpeg { quality: JPEG_QUALITY });
        assert!(!photo.resized);
        assert_eq!(photo.max_encoded_bytes, MAX_ENCODED_BYTES);
    }

    #[test]
    fn base64_costs_a_third_more_than_the_bytes_it_carries() {
        assert_eq!(base64_len(0), 0);
        assert_eq!(base64_len(1), 4);
        assert_eq!(base64_len(3), 4);
        assert_eq!(base64_len(4), 8);
        assert_eq!(base64_len(3 * 1024 * 1024), 4 * 1024 * 1024);
    }

    #[test]
    fn a_three_megabyte_photo_does_not_fit_and_the_budgeted_one_does() {
        // The defect C7 names: one untouched phone photo is over the PATCH
        // limit on its own, before any text.
        let raw = data_uri_len("image/jpeg", 3 * 1024 * 1024);
        assert!(raw > PATCH_LIMIT_BYTES);
        assert!(!fits_insert_budget(raw));
        assert!(fits_insert_budget(data_uri_len("image/jpeg", 280 * 1024)));
    }

    #[test]
    fn the_data_uri_header_is_counted_not_ignored() {
        assert_eq!(data_uri_len("image/png", 0), "data:image/png;base64,".len());
        assert_eq!(data_uri_len("image/png", 300), "data:image/png;base64,".len() + 400);
    }

    // ── the document's size ───────────────────────────────────────────────────

    fn document(json: &str) -> crate::model::Document {
        crate::model::Document::from_slice(json.as_bytes()).expect("fixture must parse")
    }

    #[test]
    fn inline_image_bytes_are_attributed_to_the_images() {
        let payload = "A".repeat(1000);
        let doc = document(&format!(
            r#"{{"content":[{{"attrs":{{"src":"data:image/png;base64,{payload}"}},"type":"image"}},{{"content":[{{"text":"hi","type":"text"}}],"type":"paragraph"}}],"type":"doc"}}"#
        ));
        let usage = measure(&doc).expect("measures");
        assert_eq!(usage.image_count, 1);
        assert_eq!(usage.image_bytes, "data:image/png;base64,".len() + 1000);
        assert!(usage.total_bytes > usage.image_bytes);
    }

    #[test]
    fn an_image_nested_in_a_table_is_still_counted() {
        // Images do not only live at the top level, and a status bar that misses
        // the ones inside tables under-reports exactly the documents most at risk.
        let doc = document(
            r#"{"content":[{"content":[{"content":[{"content":[{"attrs":{"src":"data:image/png;base64,AAAA"},"type":"image"}],"type":"tableCell"}],"type":"tableRow"}],"type":"table"}],"type":"doc"}"#,
        );
        let usage = measure(&doc).expect("measures");
        assert_eq!(usage.image_count, 1);
    }

    #[test]
    fn a_referenced_image_costs_nothing_to_the_budget() {
        let doc = document(
            r#"{"content":[{"attrs":{"src":"https://example.invalid/cat.png"},"type":"image"}],"type":"doc"}"#,
        );
        let usage = measure(&doc).expect("measures");
        assert_eq!(usage.image_count, 0);
        assert_eq!(usage.image_bytes, 0);
    }

    #[test]
    fn the_envelope_shape_is_measured_too() {
        let doc = document(
            r#"{"_type":"multi-page","pages":[{"content":{"content":[{"attrs":{"src":"data:image/png;base64,AAAA"},"type":"image"}],"type":"doc"},"id":"page-0"}]}"#,
        );
        let usage = measure(&doc).expect("measures");
        assert_eq!(usage.image_count, 1);
        assert!(usage.total_bytes > 0);
    }

    #[test]
    fn the_warning_fires_before_the_save_fails() {
        // The whole point of C7: amber while the document is still savable.
        const { assert!(WARN_BYTES < PATCH_LIMIT_BYTES) };
        assert_eq!(Usage { total_bytes: 0, ..Usage::default() }.status(), SizeStatus::Ok);
        assert_eq!(
            Usage { total_bytes: WARN_BYTES, ..Usage::default() }.status(),
            SizeStatus::Warn
        );
        assert_eq!(
            Usage { total_bytes: PATCH_LIMIT_BYTES, ..Usage::default() }.status(),
            SizeStatus::Warn
        );
        assert_eq!(
            Usage { total_bytes: PATCH_LIMIT_BYTES + 1, ..Usage::default() }.status(),
            SizeStatus::Over
        );
    }

    #[test]
    fn an_insert_that_would_break_the_document_is_refused_before_it_happens() {
        let nearly_full = Usage { total_bytes: PATCH_LIMIT_BYTES - 1024, ..Usage::default() };
        assert!(nearly_full.would_exceed(MAX_ENCODED_BYTES));
        assert!(!nearly_full.would_exceed(512));
        assert_eq!(nearly_full.headroom(), 1024);

        let over = Usage { total_bytes: PATCH_LIMIT_BYTES + 5, ..Usage::default() };
        assert_eq!(over.headroom(), 0);
        assert!(over.would_exceed(1));
    }
}
