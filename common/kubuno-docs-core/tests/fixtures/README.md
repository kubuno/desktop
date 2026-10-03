# Kubuno Documents — round-trip fixtures

These fixtures exist for one test: **a document read from storage and written back must produce
the same bytes.** Not the same meaning — the same bytes. A desktop client that re-serialises a
document it only partly understands will silently drop whatever it did not model: tracked changes,
anchored comments, header and footer definitions, a rich text box's entire sub-document. The user
never sees it go.

## What is here, and what is not

| Set | Where | In this repo? |
|---|---|---|
| `edge/` — hand-written, one hazard each | `edge/*.json` | **yes** |
| `template-*.json` — the four built-in templates | here | **yes** |
| The real-document corpus — 18 documents | see below | **no** |

### The corpus lives outside the repository, on purpose

The 18 corpus fixtures are real `.kbdoc` documents taken from a maintainer's own local office
content cache. They are the single most valuable material in this test — they are the bytes the
system actually distributes, and they are what revealed that **two different serialisers write this
format** (the browser's `JSON.stringify` omits absent node keys; the server's `serde::Serialize` on
`PmNode` writes them as explicit `null`), a divergence no amount of reading the schema would have
surfaced.

They are also somebody's documents, and this repository is public. So they stay on disk and out of
the tree.

Point the test suite at them with:

```
KUBUNO_DOC_CORPUS=C:\kubuno-build\documents-fixtures\corpus
```

The corpus tests **skip** when the variable is unset or the directory is missing — they do not
fail. That is deliberate: CI and any contributor's clone run the `edge/` and template suites, which
are entirely synthetic and cover the round-trip design; the corpus suite is an additional, stronger
check that runs where the corpus exists. `PROVENANCE.md` in that directory records where each file
came from, what it exercises, and the review that excluded three documents (two embedding a real
personal photograph, one embedding third-party artwork).

If you are reconstituting the corpus on a new machine, it is the `.kbdoc` files under the desktop
sync client's instance directory: `%APPDATA%\kubuno-desktop\instances\<instance>\office\content\`.
Each is gzip; the stored `content_json` is the literal slice between `{"content":` and
`,"version":1}`. Take it verbatim — reformatting it destroys the very property under test.

## The two assertions

They are different tests and conflating them is how the byte-exact one gets quietly weakened into
uselessness.

- **A1 — byte-exact.** `read(bytes) → write() == bytes`, with no canonicalisation on either side.
  It runs over the corpus only: those 18 files are compact, every nested object is byte-sorted,
  there is no trailing newline and — verified across all 337 documents in the cache — not one JSON
  escape sequence anywhere. They are the real serialiser's own output, so they are the only files
  a byte comparison is meaningful against.
- **A2 — semantic, plus preservation.** For the `edge/` and template fixtures, which are
  pretty-printed by hand or come from a `JSONB` column and therefore cannot be byte-compared:
  parse, re-serialise, and assert the two parse to equal `Value`s **and** that every unknown
  subtree, every unknown attribute, and the *presence or absence* of each optional node key came
  through unchanged.

## What no fixture covers yet

Listed so the gap is a decision rather than an oversight. Nothing in the corpus or in `edge/`
exercises: the `insertion` / `deletion` marks and `cellRevision` / `rowRevision` / `rowCantSplit`
attributes (tracked changes — the exact case the preservation machinery exists for), the envelope
keys `comments`, `spell`, `trackChanges`, `evenOdd`, `headerEven`, `footerEven`, `headingNumbers`,
`sources`, `refSettings`, `citationStyle`, `pageGrad`, or the node types `codeBlock`,
`horizontalRule`, `inlineImage`, `footnote`, `endnote`, `field`.

These must be hand-written into `edge/` **before** the model is written, not after. They are a few
hundred bytes each, and they are the only thing that tests the design rather than the happy path.

## Keep fixtures impersonal

Every author, reviewer and comment name in a hand-written fixture is invented. One slipped through
as a real person's name and was corrected; if you add a fixture, do not use a real one.
