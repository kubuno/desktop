# Edge-case fixtures for the `.kbdoc` round-trip invariant

Hand-built, one hazard per file. Every one of them is a shape the format permits, that a plausible
implementation silently corrupts, and that no current writer in the repo can be asked to produce on
demand.

Two things drive the whole set, and both come from the same root cause — **two different serialisers
write this format**: the browser's `JSON.stringify` (`ed.getJSON()`, `DocumentEditorPage.tsx:14330`)
and the server's `serde::Serialize` on `PmNode` (`office/src/converters/types.rs:4-16`,
`#[serde(default)]` with **no** `skip_serializing_if`).

1. **Most of what must be preserved is a sibling of the body**, not a node inside it
   (`46-preserved-subtrees.md §0`). A node-shaped `Opaque` escape hatch catches none of it.
2. **Node key *presence* is part of the document.** The browser omits absent keys; the server writes
   `"attrs":null,"content":null,"marks":null,"text":null`. Both shapes are in the wild, sometimes in
   the same file (`31-CORRECTIONS.md` C5c / R1).

---

## 1. What these files are

Each file is a **`content_json` envelope** — the value the API carries and the value stored inside a
`.kbdoc`, *without* the `{"version":1,"content":…}` file wrapper and *without* gzip
(`41-kbdoc-envelope.md §2-3`; `office/src/services/content_files.rs:138-140`, `:193-197`). A test
that wants the on-disk shape wraps and gzips these; a test of `model/` reads them directly.

All 42 files satisfy the three non-negotiables of `41 §7`, verified mechanically:

* `_type` is the literal `"multi-page"` — anything else makes the web re-wrap the whole envelope as
  a bare PM doc and lose page setup, headers and comments (`DocumentEditorPage.tsx:512-518`);
* `sections` and `pages` are both present, non-empty arrays (`:14594`, `:4715` — both `as` casts
  with no runtime guard);
* every `pages[i].content` is a `doc` **node**, never a bare array (`:4716`,
  `document_convert.rs:78`).

**Not covered here: the bare-doc shape.** One stored document in five is not an envelope at all
(`31-CORRECTIONS.md` C5b/R3: 70 of 337). Its write-back rule is *"a bare doc is written back bare"*
and it belongs in the corpus set, where five such fixtures already live.

## 2. Conventions that are load-bearing

**Key order.** Every file is written with every object's keys in **byte-sorted order**, because that
is the order the stored file already has: the office server re-serialises through
`serde_json::Value` with no `preserve_order` on every write (`office/Cargo.toml:82`,
`content_files.rs:108-111`), so `serde_json::Map` is a `BTreeMap` and every nested object comes back
sorted. Two files break that rule **on purpose** — `keyorder-envelope-root.json` (everywhere) and
`keyorder-raw-subtree.json` (only inside subtrees the model carries raw) — and their entries say so.

**Formatting follows the assertion, not taste.** A fixture claimed for **A1** is written compact
(one line, no whitespace between tokens, no trailing newline); a fixture claimed for **A2** is
pretty-printed. There is no third style. Before this pass all 21 files were pretty-printed with a
trailing `0x0A` while the README implied 19 of them were byte-exact: `serde_json::to_vec` emits
compact bytes with no trailing newline, so those 19 would have failed on formatting **before any
hazard was reached** (`31-CORRECTIONS.md` R5, measured). 38 files are now compact; 4 are pretty and
claimed for A2 only.

Side effect worth keeping: a compact file contains **no `0x0A` byte at all**, so a CI checkout with
Git-for-Windows' default `core.autocrlf=true` cannot change it. The four pretty files are still
exposed, which is harmless for A2 (their *strings* contain no newlines) but a
`tests/fixtures/** -text` line in `desktop/.gitattributes` is still the right fix — the repo has no
`.gitattributes` and `core.autocrlf=false` is a local setting (`31-CORRECTIONS.md` R5).

**Ids.** `sec-0`, `page-0`, … — short and readable on purpose. Real ids are uuids; nothing in the
reader may require the uuid shape, and the write-back rule keeps the file's ids rather than minting
new ones (`41 §7`).

**Names.** Every author, reviewer and commenter in this directory is **invented**. No fixture may
carry the name of a real person: these files are public the moment they are pushed
(`31-CORRECTIONS.md` R11 caught one and it has been replaced).

**Images.** Any `src` here is a 1×1 fully transparent PNG data URI, built locally, never downloaded.
Where the real product would write something else — a rich text box's `src` is a generated white
frame SVG (`richTextBoxFrameSvg`, `DEP:1467-1469`, wrapped by `svgToDataUrl`,
`documents/shapes/params.ts:37-38`) — the entry says so; the hazard under test is never the `src`.

## 3. The assertions

| | Assertion | Scope |
|---|---|---|
| **A1** | **Byte-exact.** `read(bytes)` → `write()` **==** `bytes`. No canonicalisation, no `canon()`, no normalisation step (`31-CORRECTIONS.md` C6). | The 38 compact, byte-sorted, newline-free files. |
| **A2** | **Semantic + preservation.** Parse, re-serialise, then assert **(a)** `from_str::<Value>(input) == from_str::<Value>(output)` after the documented open-time transform, **(b)** every unknown subtree, every unknown attribute and the **presence or absence** of each optional node key came through unchanged, and **(c)** the per-file verbatim payload check named in the entry. | The 4 pretty files. |
| **B** | **Stability**, required everywhere: `write(read(write(read(b))))` == `write(read(b))`. | All 42. |
| **C** | After a write-back the envelope still satisfies §1's three non-negotiables. | All 42. |

A1 is only meaningful over `Box<RawValue>`: a model that routes anything through `serde_json::Value`
re-prints numbers even with `float_roundtrip` on (`47-serde-roundtrip.md`, experiment 3), so
`serde_json = { version = "1", features = ["raw_value", "float_roundtrip"] }` is a precondition of
this directory, not an optimisation.

A2's clause (a) says *"after the documented transform"* for exactly two reasons, both modelled and
both deliberate: `envelope-three-pages.json` is flattened to one page on open (`41 §7`), and
`attrs.collapsed` is rewritten from `collapsedDefault` on open (`DEP:4722-4725`,
`31-CORRECTIONS.md` C9). Everywhere else the transform is the identity and clause (a) is plain
equality.

**Do not "fix" an A1 failure by normalising.** That is the `canon()` C6 deleted, and it re-opens the
hole C6 closed. If a fixture cannot pass A1, move it to A2 and say why here.

---

## 4. The fixtures

### 4.1 Tracked changes — the case the whole design exists for

Until this pass there was **zero** coverage of any of it (`31-CORRECTIONS.md` R4), while `40 §7`
names that exact gap: *"the whole `Box<RawValue>` machinery ships untested on the one case that
motivates it."*

| File | Hazard | Assert |
|---|---|---|
| `track-insertion-deletion-marks.json` | `insertion` and `deletion` marks with their complete, real attribute set — `author`, `authorId`, `date` (ISO-8601 UTC, second precision), `id` (`documents/track-changes.ts:129-153`, marks at `:156-173`). Three runs: insertion alone; insertion **and** deletion on the same run (legal — both declare `excludes:''`); and a deletion whose `author`/`authorId`/`date` are the empty string, which is a legal unattributed change. | **A1.** All four attributes survive on every run, empty strings included: `authorColor` keys on `authorId \|\| author` (`track-changes.ts:113-123`), so emptying the pair collapses every author into one colour. Deleted text **stays in the document** carrying its mark until someone accepts (`track-changes.ts:10-14`) — a model that drops `deletion` silently accepts deletions nobody approved. There are no format-change revisions in this model; do not invent `pPrChange`. |
| `track-table-revisions.json` | The three importer-only table attributes, none of them declared by the editor, all destroyed by the browser's first open: `table.attrs.rowCantSplit` (`bool[]`, one per row, `docx/read/table.rs:415`), `tableRow.attrs.rowRevision` and `tableCell.attrs.cellRevision`, both `{type,author,authorId,date,id}` where `type` is `"insertion"` or `"deletion"` (`table.rs:383,456` / `:293`, shape at `read/revisions.rs:108-116`). | **A1.** The three attributes come back untouched on nodes the desktop **renders** — so they can never be swallowed by node-level opacity; they need `attrs: Option<Box<RawValue>>`. `converters/docx/write/revisions.rs:261-264` reads this family back, so dropping them changes what Word receives, not just what we store (`31-CORRECTIONS.md` R7). |

### 4.2 Envelope siblings — the wall a node-shaped `Opaque` misses entirely

| File | Hazard | Assert |
|---|---|---|
| `envelope-comments-thread.json` | The one item split across both worlds: a `comment` mark with `commentId` on the anchored run (`DEP:1242-1254`) **and** the matching thread in `envelope.comments` — `{id, author, authorId, text, createdAt, resolved, replies[], quote?}` (`DEP:12420-12424`), here with one reply. (`46 §6` fixture 3.) | **A1.** Both halves survive a body-only edit. Drop the **mark** and the gutter filters the thread out (`DEP:12662-12666`) while it stays in the Yjs map and in the envelope — invisible, undeletable, still exported to DOCX; it is explicitly **not** garbage-collected (`void anchoredIds`, `:12661`). Drop the **key** and DOCX export loses every comment (`document_convert.rs:149-156`). The desktop has no Yjs client, so `comments` is opaque carried state: writing `comments: []` because we have no thread model wipes the export copy of every thread. |
| `envelope-sibling-wall.json` | `styles` + `watermark` + `lineNumbers` + `pageNumStart` + `spell` + `refSettings` + `sources` in one envelope (`46 §6` fixture 5). Real shapes, not sketches: `NamedStyleMeta` (`DEP:387`), `WatermarkDef` (`DEP:391`), `LineNumbersDef` (`DEP:397`), `SpellSettings` (`DEP:385`, whose `rules` key is a real grammar-rule id from `GRAMMAR_RULES`, `frontend/src/spellcheck.ts:92-112`, read as *"active unless explicitly false"* at `:114`), `ReferencesSettings` — four `CommonTableSettings` blocks at their real defaults (`documents/references/types.ts:87-145`) — and `Source` with all eight keys (`references/sources.ts:9-19`). | **A1**, and separately: after a simulated body-only edit **every one of the seven keys is still present and byte-identical**. This is the regression test for **two live web data-loss bugs**: (1) `refSettings` / `sources` / `citationStyle` are written by `serializeDoc` (`DEP:537-538`) and **never read back** (`parseDocContent`, `:490-511`), so the load falls back to `DEFAULT_REFERENCES` / `[]` / `'APA'` (`:14584-14590`) and the next 700 ms autosave overwrites the stored values — **a bibliography survives exactly until the second time someone opens the document in a browser** (`46 §2.8`); (2) `headerEven` / `footerEven` are parsed and serialisable but never destructured on load and never passed to `serializeDoc`, so **every web save drops them** (`46 §2.1`) — that half is exercised by the next fixture. |
| `envelope-hf-even-odd.json` | `evenOdd`, `hfFirstPage`, and all four header/footer documents at the envelope root: `header`, `footer`, `headerEven`, `footerEven` (`DEP:522-523,539`; read at `:505-508`). The footer carries the literal token `{page}` because the importer rewrites `PAGE`/`NUMPAGES`/`DATE`/`TITLE` into `{page}` `{pages}` `{date}` `{titre}` text in header/footer documents (`docx/read/fields.rs:116-124`), expanded at paint time by `expandHFDoc` (`DEP:544-556`). | **A1.** All six keys return unchanged, `headerEven`/`footerEven` included — bug (2) above. `hfFirstPage` **suppresses** the header on page 1; it is not a separate first-page part, and there is no per-section variant of either flag (`46 §2.1`). Carrying `headerEven`/`footerEven` verbatim is strictly better than the incumbent, which deletes them. |
| `envelope-doc-flags.json` | The four remaining root keys with no fixture anywhere: `trackChanges` (bool, omitted when false, `DEP:536`), `headingNumbers` (bool, omitted when false, `:528`), `citationStyle` (string, omitted when it equals `'APA'`, `:538`) and `pageGrad` — a `Gradient`, i.e. `{type:'linear'\|'radial', angle, stops:[{color,position,opacity}]}` (`core/frontend/src/ui/gradient.ts:12-16`, imported by `DEP:120`). | **A1.** `trackChanges` is the worst single-key loss in the format: drop it and tracking is **off** for the next editor, whose edits enter the document unmarked and indistinguishable — a reviewing document that silently stops reviewing (`46 §2.6`). `pageGrad` also proves the model does not flatten a nested object into a colour string. |
| `envelope-unknown-sibling.json` | `kbFutureRootKey` (invented) next to `sources`, `refSettings`, `citationStyle`. | **A1**, and after a simulated body-only edit all four keys are still present. A node-shaped `Opaque` catches **none** of this — these are siblings of the body, not nodes in it (`46 §0`). |
| `envelope-three-pages.json` | Three pages and **two** sections. The corpus says this path is live — **6 of 267** real envelopes have more than one page (`31-CORRECTIONS.md` R2) — while no current writer emits it, so it can only be hand-made. Also carries the always-present `watermark` / `pageBorder` / `lineNumbers` explicit nulls. | **A2 (transform).** Assert exactly: `pages.len() == 1` and `sections.len() == 1`; `pages[0].id == "page-0"` and `sections[0].id == "sec-0"` (kept from the file, **no new uuid**); `pages[0].content.content` is the three bodies concatenated in order, mirroring `flattenToDoc` (`DEP:4712-4718`); `sec-1`, `page-1`, `page-2` are **gone**; every other root key byte-identical, explicit `null`s included. Extra shells are **dropped, not re-emitted headless**: a page object without `content` throws `TypeError` in the browser at `:4716`, so C5's original rule shipped a file the web cannot open (`41 §8`, `31-CORRECTIONS.md` R2). |

### 4.3 Node key presence, and node types that had no fixture at all

| File | Hazard | Assert |
|---|---|---|
| `node-explicit-null-keys.json` | **The R1 case.** One paragraph and its text node in the server's shape — `"attrs":null,"content":null,"marks":null,"text":null` written explicitly (`converters/types.rs:4-16`) — next to a compact paragraph that omits the same keys, **in the same file**. Corpus-wide: 17 of 337 documents carry the explicit-null shape and `legacy-bare-doc-table.json` mixes both in one body (`31-CORRECTIONS.md` C5c). | **A1**, and it is the fixture that decides the node type. `skip_serializing_if = "Option::is_none"` destroys the null nodes; omitting it destroys the other 320 documents. There is no tuning that saves both: the node must model **presence as distinct from null** — `BTreeMap<String, Box<RawValue>>` per node with `type`/`content`/`marks`/`text` pulled out by name, or `Option<Option<Box<RawValue>>>` per key. |
| `node-unknown-type.json` | A `kbFutureBlock` node — not in the 24-type schema — between two paragraphs, with its own `attrs` and children. | **A1**, **and the node keeps index 1 of 3**. An unknown node is `Box<RawValue>` in the children vector (`46 §7`); its ProseMirror size is unknown, so caret arithmetic must refuse to cross it rather than guess. |
| `node-codeblock.json` | `codeBlock`, content `text*` with marks stripped, and its one attribute `language` (string \| null, `extension-code-block/dist/index.js:25-40`) — spelled **both** ways in one file: `"rust"` and an explicit `null`. The text carries a real `\n`. | **A1.** `null` and absent stay distinct; the `\n` escape is re-emitted in serde's canonical form, so this file is unaffected by the open `text-escape-forms.json` decision. The canvas engine renders `codeBlock` (`canvas-engine.ts:729-1013`) but never its marks — carry, do not validate. |
| `node-horizontal-rule.json` | `horizontalRule` — a content-less leaf, ProseMirror size **1** — between two paragraphs. | **A1**, plus the size assertion: `canvas-engine.ts:684-690` falls through to `sz = 2` and the rule branch (`:997-1002`) adds the trailing `pos++` at `:1016`, so **the web consumes 2 positions where the model has 1** and every caret position after a rule drifts (`31-CORRECTIONS.md` R10, confirmed on both sides). The desktop returning 1 is a **deliberate divergence**, not parity — the same +1 hits *any* inline node the engine does not know, which is exactly what a `Box<RawValue>` desktop holds. |
| `node-inline-image.json` | `inlineImage`, the *inline* atom, with all five declared attributes `src` / `width` / `height` / `alt` / `rotation` (`DEP:1426-1447`) — distinct node type from block `image`, easy to fold into one Rust variant and lose. | **A1.** `src` is a locally built 1×1 transparent PNG data URI. The width/height here are doc px, not the image's pixel size. |
| `node-footnote-endnote.json` | `footnote` (`DEP:1597-1620`) and `endnote` (`documents/endnotes.ts:70-88`) in one paragraph. Both are **inline atoms with a single `text` string attribute** — no rich content, no separate note store. | **A1.** The note text lives nowhere else: drop the atom and the note is deleted outright. Numbering is positional, derived at layout time (arabic for footnotes, **lowercase roman** for endnotes, `endnotes.ts:56-67`), so nothing numbers them in storage and re-ordering the atoms renumbers everything after the change. |
| `node-field.json` | `field`, an inline atom with `kind` / `instr` / `cached` (`documents/fields.ts:326-362`). One `kind:"pages"` carrying `NUMPAGES \* ROMAN`, one `kind:"other"` carrying a `DOCPROPERTY` instruction we cannot recompute. | **A1.** `instr` is kept **verbatim for the round trip** and is the only copy of the Word instruction (`fields.ts:342-346`); unknown field types deliberately become `kind:'other'` and are frozen on their cached result (`fields.ts:10-12`), which makes `instr` load-bearing rather than decorative. Flatten a field to its cached text and the page number freezes at "3" forever. Note the escapes: `\\` and `\"` are already serde's canonical forms, so A1 holds independently of the `text-escape-forms.json` decision. |
| `node-sectionbreak-header.json` | `sectionBreak` carrying `hfLinked:false` plus its **own** `header` and `footer` — two complete ProseMirror documents nested inside a node attribute — with `orientation:"landscape"`, per-side margins and `pageColor` (`DEP:598-627`). (`46 §6` fixture 4.) | **A1.** Sections 1..N do not exist in `sections[]` at all; their geometry and their running heads live **inside the body**, here (`buildSectionGeoms`, `DEP:294-318`). A walker that recurses only on `content` treats these two documents as opaque strings-in-attrs and preserves them, which is correct; anything that *rewrites* a `sectionBreak` must treat them as structured data. |

### 4.4 Marks

| File | Hazard | Assert |
|---|---|---|
| `mark-code-bookmark-spelllang.json` | Three marks with no previous fixture, on three runs of one paragraph: `code` (no attributes at all — the mark object is `{"type":"code"}`), `bookmark` with `name` (`DEP:1255-1268`), `spellLang` with `lang` (`DEP:1269-1282`). | **A1.** A mark with no `attrs` key must not grow one. Bookmarks are the target of `PAGEREF` cross-references and are imported with Word's internal names filtered but `_Toc…` kept (`docx/read/fields.rs:357-365`): lose one and every cross-reference pointing at it falls back to its cached string forever. `spellLang` is per-range proofing language; losing it red-underlines a whole passage in the wrong dictionary. |
| `mark-reference-entries.json` | `indexEntry` (`text`, `sub`) ⇄ Word `XE` and `citationEntry` (`short`, `long`, `category`) ⇄ Word `TA` — marks, not nodes, deliberately, because Word hides them as field codes inside flowing text (`documents/references/entries.ts:21-44`). | **A1.** The generated index and table of authorities are rebuilt from these marks (`collectIndexHits`, `entries.ts:52-62`); regenerating the index after a lossy desktop save produces an empty table, with no error anywhere. |
| `mark-text-effect.json` | `textEffect` with all nine attributes non-null in one mark: `fill`, `outline{color,width,dash}`, `shadow{color,blur,dx,dy,inner}`, `glow{color,radius}`, `reflection{opacity,size,blur,distance}`, `ligatures`, `numForm`, `numSpacing`, `stylisticSet` (`documents/text-effects/model.ts:154-170`, value shapes at `:15-58`). `shadow.dy` is spelled `2.0`. | **A1.** Nested objects inside a **mark**'s attrs survive verbatim, `2.0` included — the same `Box<RawValue>` rule as node attrs, one level deeper. This is also the only mark registered in `RICH_ZONE_EXTENSIONS` (`DEP:1683`), so it can appear inside a header or a text box too. |
| `mark-textstyle-fontsize-polymorphic.json` | **`textStyle.fontSize` is a string *or* a number, and both are common.** Three runs in one file: `"13pt"`, `11.0`, `18`. The corpus has 16 documents with the string form and 20 with the number form (`31-CORRECTIONS.md` R6); `legacy-bare-doc-table.json` stores `11.0`, `comment-mark-orphan.json` stores `"28pt"`. | **A1.** A typed `Option<String>` fails to deserialize a fifth of the corpus; a typed `Option<f64>` fails on the other fifth. `attrs` stays raw and any typed *view* accepts both, exactly as the server already does (`length_pt`, `docx/read/run.rs:491-498`). `11.0` must also come back `11.0`, never `11` — that file is byte-stable under Rust and **not** under the browser, so the oscillation is already in the corpus. |
| `mark-importer-only-textstyle.json` | A `textStyle` mark carrying the **ten** importer-only attributes — `underlineStyle` (`run.rs:353`), `underlineWords` (`:356`), `underlineColor` (`:359`), `doubleStrike` (`:363`), `allCaps` (`:366`), `hidden` (`:372`), `charScale` (`:309`), `charPosition` (`:417`), `fontSizeCs` (`:398`) and **`shading`** (`run.rs:378`, a run-level background kept as a `textStyle` attribute when a `w:highlight` already took the `highlight` mark) — plus the declared `fontSize` / `letterSpacing`. | **A1.** All twelve survive; `fontSize` stays the string `"14pt"` and `letterSpacing` stays the number `0.85`. `shading` is the tenth attribute `40 §5` missed (`31-CORRECTIONS.md` R7) and is declared by the frontend only as a **paragraph** global attribute (`DEP:1207-1211`), never on `textStyle` — so ProseMirror drops it on first open. **There is no importer-only mark *type*:** every mark the converters emit is declared in the frontend schema; the editor-never-produces surface is these attributes. |
| `attr-unknown-paramark.json` | `paragraph.attrs.paraMark` — `{type,author,authorId,date,id}`, the revision of the paragraph **mark** itself (`docx/read/revisions.rs:46-49,338`), dropped by ProseMirror on the browser's first open — next to `zzUnknownVendorAttr: null` and the known `textAlign`. | **A1.** Requires `attrs: Option<Box<RawValue>>` with known keys read *out of* the raw value, never a typed struct. `null` and *absent* are two different documents: `zzUnknownVendorAttr` must come back as an explicit `null`. `write/revisions.rs:261-264` reads `paraMark` back, so dropping it changes the exported DOCX. |

### 4.5 Numbers

| File | Hazard | Assert |
|---|---|---|
| `float-sub-ulp.json` | `sections[0].margins.left` is the literal `47.999999999999996`. Without `float_roundtrip` this parses to the **wrong f64** (`0x4048000000000000`, i.e. `48.0`); with it the bits are right but a `Value` or typed-`f64` hop still prints `47.99999999999999`. | **A1.** The 18 digits come back exactly. Then, separately: parsing this margin into `DocPx` — `pub type DocPx = f32` (`desktop/documents/src/doc/mod.rs:16`) — yields **48**. `sections` is the one object the desktop unavoidably interprets (`PageGeometry`, `doc/mod.rs:43-51`), so the raw token and the layout value must be **two different fields** (`31-CORRECTIONS.md` R8). Every real document stores margins as bare integers (`96`), which a typed `f64` re-prints as `96.0` — that alone fails A1 on all 267 corpus envelopes. |
| `float-twips-px.json` | `indentLeft: 18.866666666666667` (283 twips) and `tabStops[0].pos: 83.13333333333334` (1247 twips). Both are already shortest-form, so a `Value` hop preserves them — an **f32** hop does not (`18.866666793823242`, `83.13333129882812`). | **A1.** This is the fixture that fails when someone "simplifies" a `Box<RawValue>` attr into a `DocPx` field. Also asserts `tabStops[].type` survives while having zero layout effect in v1 (`canvas-engine.ts:954` keeps only `pos`). |
| `float-integral.json` | The same f64 spelled three ways in one `attrs`: `spaceBefore: 2`, `spaceAfter: 2.0`, `lineHeight: 1.0`. A typed `f64` prints `2.0` for both; a typed `u32` prints `2` for both; `Value` prints `2` and `1`. Every choice changes bytes for at least one of them. | **A1.** `2` stays `2` and `2.0` stays `2.0` **in the same object** — the cheapest proof that `attrs` is carried as `Box<RawValue>` and not as a typed `ParagraphAttrs`. |
| `float-exponent.json` | Four notations on one `image`: `1.0e2`, `0E0`, `-1.5e-3`, `1e21`. A `Value` hop yields `100` / `0` / `-0.0015` / `1e+21` in JS and `100.0` / `0.0` / `-0.0015` / `1e21` in Rust. | **A1**, all four verbatim. Plus: `1e21` overflows every integer type, so a model that infers *"`zOrder` is an int"* fails to **parse**, not merely to round-trip. |

### 4.6 Text

| File | Hazard | Assert |
|---|---|---|
| `text-combining-marks.json` | One string holding `e` + U+0301 (NFD) followed by U+00E9 (NFC) — two spellings of "école"; a second paragraph of Devanagari with U+0941 / U+094D / U+0947. | **A1**, and **no NFC/NFD normalisation anywhere**. Slice 1c: `badCut` refuses a cut before `\p{M}` (`canvas-engine.ts:2124-2146`), so the two spellings must break identically. |
| `text-astral-emoji.json` | U+1D11E and U+1F600 as raw UTF-8 (serde_json emits non-ASCII raw, `41 §2`). | **A1**, still raw UTF-8, never re-escaped to `\uD834\uDD1E`. Second assertion: this text node's ProseMirror length is **30 UTF-16 code units, not 28 chars** — all `pmPos`/`pmLen` arithmetic is UTF-16 (`canvas-engine.ts:685`, `:1944`, `:2144`), so a `char`-based length desynchronises the caret past the first emoji. |
| `text-rtl.json` | Arabic, U+200F RIGHT-TO-LEFT MARK, Hebrew, then an ASCII tail in one string. | **A1**, U+200F never stripped — it is not in JS `\s`, so the breaker sees it as part of a word token (`canvas-engine.ts:1909-1948`). Open for slice 1c: the web measures **per token and sums**, so cross-token bidi reordering does not exist in the browser and the desktop must not introduce it. |
| `text-zwj-sequence.json` | 👩‍💻 (U+1F469 U+200D U+1F4BB) and a three-person ZWJ family. | **A1.** Slice 1c: `badCut` rejects only low surrogates and `\p{M}`, and U+200D is `Cf` — so the **web can split a ZWJ sequence** (`canvas-engine.ts:2124-2146`). The desktop reproduces that; it does not fix it. |
| `text-escape-forms.json` | the four escape sequences `\u00e9`, `\/`, `\u2028` and `\u0041` — four escapes whose values are ordinary characters. A `Value` hop yields `é`, `/`, a raw U+2028 byte sequence and `A`: the same string, different bytes. | **A2 — because the decision it forces is not made yet.** `text` is a field the renderer must read, so either the model keeps the raw token alongside the decoded string (and this file becomes A1), or A1 is defined as "byte-identical modulo JSON string escaping". Pick one, write it in `model/mod.rs`, then move this row. Real data does not force the choice: **not one JSON escape exists in the whole 337-document corpus** (`31-CORRECTIONS.md` R5). |
| `text-lone-surrogate.json` | `"orphan \ud83d here"` — an unpaired high surrogate in escape form. | **A2, and a decision, not a test.** `serde_json::from_slice::<Value>` **fails outright** ("lone leading surrogate in hex escape"); `Box<RawValue>` parses and re-emits it. Assert whichever policy `model/mod.rs` states — refuse to open and report the byte offset, or substitute U+FFFD and mark the document dirty-on-open. Note a lone surrogate cannot reach a stored file *through the server* (its own `Value` parse rejects it first), so "refuse to open" is defensible and cheap. |
| `para-whitespace-only.json` | Four paragraphs: three spaces; a lone U+00A0; U+202F + U+2007; U+200B + U+00AD. | **A1**, with **no trimming of any kind**. Slice 1c: the first three are whitespace-run tokens (JS `\s` includes NBSP, U+202F, U+2007 — an NBSP *is* a break opportunity and stretches under justification), while the fourth is **not** whitespace to the web and is one ordinary word token of near-zero width. |

### 4.7 Paragraph degenerates

| File | Hazard | Assert |
|---|---|---|
| `para-empty.json` | Three paragraphs: one with **no `content` key at all**, one with `"content": []`, one with `attrs` and no content. | **A1** — absent stays absent and `[]` stays `[]`. Requires `Option<Vec<Node>>`; `#[serde(default)] Vec<Node>` collapses the first two into one document. The compact companion of `node-explicit-null-keys.json`: that file proves `null` ≠ absent, this one proves `[]` ≠ absent. |
| `para-empty-text-node.json` | `{"text":"","type":"text"}`, once bare and once carrying a `bold` mark, followed by a real text node. | **A1.** ProseMirror forbids empty text nodes, so a strict validator rejects this file: the model must **carry, not validate**. Second assertion: an empty text node has PM size 0 — a size function returning 1 per text node drifts every caret position after it. |

### 4.8 Structure, tables and images

| File | Hazard | Assert |
|---|---|---|
| `nesting-table-in-list-in-blockquote.json` | `blockquote > bulletList > listItem > {paragraph, table}` with a **second table nested inside a cell** — depth 8. | **A1** at depth 8. The model must stay recursive (`BlockBody::Blocks`; a flat `Vec<Row>` cannot hold a nested table). Note what it also says about the product: `blockquote` is in the schema and on the ribbon (`DEP:12144`) but appears **nowhere** in `canvas-engine.ts` — it falls through the bare `else` at `:1012-1014` and its text is invisible in the browser today. The desktop must round-trip it regardless; whether it *draws* it is a product decision and a deliberate divergence. |
| `table-cellborders-object.json` | `tableCell.attrs.cellBorders` **as an object**, with one side present (`{w,s,c}`, `docx/read/table.rs:55-75`), one side explicitly `null`, and two sides absent. | **A1.** The three states are three different documents. A present side is a border; an explicit `null` is *"no border"*, which can **veto the neighbour's inherited default**; an absent side **inherits** the table default (`table.rs:77-80`, which says in as many words that the distinction is load-bearing). |
| `table-cellborders-null.json` | The same cell with `"cellBorders": null` — the attribute at its declared default (`DEP:1482-1504`). | **A1.** Must not be confused with the previous file's *per-side* null, nor with the next file's absence. |
| `table-cellborders-absent.json` | The same cell with **no `cellBorders` key**. | **A1.** Collapsing null with absent corrupts every table that has a "no border" side. `Option<T>` is not enough here — the key's presence is itself data (same rule as `node-explicit-null-keys.json`). |
| `image-alt-kbtextrich.json` | **`image.alt` as a discriminated union.** A rich text box: `alt` is `kbtextrich:` + `encodeURIComponent(JSON.stringify(doc))` — a complete ProseMirror document inside a string attribute (`textBoxRichAlt`, `DEP:1457`; decoded at `canvas-engine.ts:566-569`) — while `src` carries nothing but a frame. | **A1**, and the `alt` string is carried **raw**: never trimmed, re-encoded, length-capped, or run through any "sanitise the string" helper, because `decodeURIComponent` round-trips are not byte-stable in general (`40 §9`). This is the single most dangerous attribute in the format: the prose inside every text box exists **nowhere else**, and losing it leaves a document that still opens, still paginates, and shows an empty rectangle. The same attribute also carries `kbshape:` (a shape's whole vector definition, including OOXML `avLst` adjustments) and `kbenvelope:`. **Deviation from production, stated on purpose:** the real `src` here would be a generated white-frame SVG data URI (`richTextBoxFrameSvg`, `DEP:1467-1469`); this fixture substitutes a 1×1 transparent PNG, because the hazard under test is `alt` and no fixture in this directory embeds a real image. |

### 4.9 Key order — the boundary of the design

| File | Hazard | Assert |
|---|---|---|
| `keyorder-envelope-root.json` | Every object written in reverse byte order, root included. | **A2 — and that is correct.** The envelope is `BTreeMap<String, Box<RawValue>>` (`46 §7`), so the root is re-sorted to `_type, lineNumbers, pages, sections, watermark` and typed structs re-emit in field-declaration order. Assert the output equals the byte-sorted form **and** assertion B. Normalising key order here is not a defect: it is exactly what the office server already does to the stored file on every write. |
| `keyorder-raw-subtree.json` | The same disorder, but *inside* an unknown root key and inside node `attrs` — including the adversarial key set `{"zeta","alpha","Mango","","10","2"}` — while every **modelled** object in the file is byte-sorted. | **A1 for the whole file**, which is the stronger claim: carried subtrees are re-emitted verbatim and unsorted, so the bytes come back identical. Read with the previous row this is the entire boundary of the design — *modelled* keys get normalised, *carried* keys do not — and the suite must demonstrate both or neither behaviour is pinned. If A1 fails here, the model is sorting something it was supposed to carry. |

---

## 5. What this directory still does not cover

* **Gzip framing.** No `.kbdoc` here. `41 §1` proves the 10-byte header is constant
  (`1F 8B 08 00 00 00 00 00 00 FF`) but **not** the DEFLATE stream; a round-trip test compares
  decompressed plaintext, never compressed bytes.
* **A real DOCX import.** `46 §6` fixture 1 wants header + footer + `evenOdd` + `trackChanges` from
  one import (`document_convert.rs:422-428`). The four keys are covered here across
  `envelope-hf-even-odd.json` and `envelope-doc-flags.json`, but **hand-written** — a file that
  actually came out of `import_docx`, with a `w:cantSplit` row and real tracked changes, still has
  to be produced by the importer and dropped in the corpus set (`40 §7`).
* **The bare-doc shape** (`{"content":…,"type":"doc"}`, 20 % of real documents) and its
  write-back rule — corpus set, `31-CORRECTIONS.md` C5b.
* **`image.alt` = `kbshape:` and `kbenvelope:`.** Only `kbtextrich:` is exercised here; the other
  two members of the union are the same carrying rule, and `kbshape:` is the main case for an
  imported document full of drawings, so it belongs in the corpus.
* **Anything about the API.** Byte identity is assertable desktop-local only; the server is a
  `Value` hop on every write (`models/document.rs:73` → `content_files.rs:108-111`). `Rev::digest`
  must hash what `GET` returns, never the outbound PATCH payload (`46 §5`), or C1's guard
  manufactures phantom conflicts.

## 6. Unverified, and decisions still owed

* **Nothing here has been run against the desktop model.** `desktop/documents/src` still has no
  `model/`, and the crate's `Cargo.toml` has no `serde`/`serde_json`. These files were validated
  with a scanner under node — JSON validity, byte-sorted key order, compact-and-newline-free
  formatting, and the three non-negotiables of §1 — **not** with `serde_json`. The float and f32
  figures quoted in §4.5 were measured with node's f64 / `Float32Array` and with the scratch Rust
  project of `47-serde-roundtrip.md`; slice 1b should re-measure them in Rust and correct this file
  if they differ.
* **`text-escape-forms.json` has no settled expected output**, and **`text-lone-surrogate.json` has
  no settled policy.** Both are A2 until `model/mod.rs` decides; the decision is owed before the
  first line of the module, because it is the type of the `text` field.
* **Provenance of the long decimals.** No writer in the repo emits an unrounded twips→px value: the
  DOCX importer rounds (`paragraph.rs:131-134`, `section.rs:15`, `table.rs:23`,
  `drawing.rs:278-283`) and the page-setup dialog rounds margins to integers (`DEP:3622`, used at
  `:3641`). The ruler-drag path (`setSectionMargins`, `:4748`) was **not** traced. §4.5 therefore
  tests the **reader**, which must be safe whatever the provenance turns out to be.
* **`envelope-sibling-wall.json` does not carry `headerEven`.** `46 §6` asks fixture 5 to double as
  the regression test for both web bugs; the `headerEven`/`footerEven` half lives in
  `envelope-hf-even-odd.json` instead, so the two files must be run together to close `46 §2.1`.
* **`refSettings` here is one plausible complete value**, copied key for key from `DEFAULT_TOC` /
  `DEFAULT_FIGURES` / `DEFAULT_INDEX` / `DEFAULT_AUTHORITIES` (`references/types.ts:94-145`). No
  real document in the corpus contains the key at all (`31-CORRECTIONS.md` R4), so the *shape* is
  verified against the type, not against stored data.
* **Whether any envelope key exists in production data that no current code writes** cannot be
  answered without a DB read. That is the whole reason for carrying unknown keys rather than an
  allow-list, and why `envelope-unknown-sibling.json` and `keyorder-raw-subtree.json` exist.
