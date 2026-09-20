# Native structured-rendering contract (C01)

Status: frozen admission contract for C02, recorded on 2026-09-20. This
document defines the minimum relationships that the native producer, FFI,
ManT IR, and consumers must preserve. It does not claim that the structured
renderer is implemented.

## Evidence checkpoint

| Item | Frozen identity |
| --- | --- |
| Opening `dev` | `aa211f7345dd80acd4b8b056e0deef0c368e18c6` |
| C00a freeze/preflight | `081bdff9` |
| C00b CVS refresh | `6a1469a4` |
| Snapshot | `cvs-20260920T122115Z` |
| Oracle identity | `cvs-20260920T122115Z-linux-x86_64-gcc-16.2.1` |
| Oracle binary SHA-256 | `d7c58752e2586695d17f4a87cc5540b35ec65ebec51bbcb5de8c9e6786e7311d` |
| Pristine archive SHA-256 | `6ccee6e73346b3e2c9701ba70dc5ac0525f2dd160cfe98ffc00ad56371d28c7d` |
| Source lock SHA-256 | `7c65308e4df4e574f2875dcda34f8dbd194fc9c6f4b421e2530a8a7908fe0e1a` |
| Shipping manifest SHA-256 | `fd6ff088c152e57194a945d6a4deb95876fb62304008d368da35d12904afe966` |
| Oracle attestation SHA-256 | `72aca4b181b0463d41ae24138d1f62fdf19cebb9e543af260fbbc70f75d4bde6` |
| Rust toolchain | rustc 1.98.0, Cargo 1.98.0, x86_64-unknown-linux-gnu |
| C toolchain | GCC 16.2.1 20260810 |
| Host | Linux x86_64, WSL2 kernel 6.18.33.2 |

The registered preflight accepted this exact archive, attestation, binary,
source lock, manifest, platform, and UTF-8 profile before the baseline oracle
run. The functional sample is
`tests/fixtures/roff/entry-name-boundaries-man.1`, SHA-256
`b3d172e9c0eb769985af02be8ae3aa915cd3c335a418c85f9983bdfed3d5f34b`.
Its exact 78-column oracle output and opening-dev/C00b product projections are tracked in
[`baselines/native-structured-rendering-c01`](baselines/native-structured-rendering-c01/README.md).
The same registered oracle was also run in ASCII and UTF-8 modes over the six
minimal logical-connection inputs described below; the combined raw output is
tracked beside the other baseline artifacts.

C00b passed `LIBMANDOC_RS_DENY_WARNINGS=1 cargo test -p libmandoc-rs
--all-features`, the affected codec/engine/application tests, and
`scripts/check.sh --build-profile release`. The full check passed outside the
filesystem sandbox after the first in-sandbox run reached four loopback-bind
tests that the sandbox denied. The check covered the workspace, CLI feature
matrix, independent consumers, packaged sources, rustdoc, strict Clippy, fuzz
compilation, symbol isolation, fixture/query gates, and release smoke test.
Sanitizer jobs and native Windows/macOS execution were not run locally and
remain unverified here.

The opening-dev and C00b release binaries and working probes are retained below
`target/structured-rendering-baseline/`; the exact differing JSON and shared
text/Markdown/outline outputs are also tracked with this document. Text,
Markdown, and outline output for the functional sample are byte-identical.
Document and explanation JSON differ only in the recorded libmandoc snapshot
version. Three alternating performance
rounds over the same GCC fixture contribute 21 samples per revision and
operation; the raw data and peak-RSS checkpoint are tracked with the baseline.
No fixed performance threshold is inferred from this host sample.

The C02b collector was sampled again on 2026-09-21 with the checked-in GCC
and Git fixtures, a warmed release build, UTF-8 profile, and native width 78.
Each test performs one warm-up traversal followed by ten measured traversals;
the metrics are deterministic across those ten traversals.  GCC reports
6,416,335 collector events, 1,232,413 tokens, 16,384 sidecar slots,
117,571,904 sidecar bytes, and 170,612,387 cumulative builder-allocation
bytes; the complete test took 2.41 seconds wall time with 137,792 KiB peak
RSS.  Git reports 287,477 events, 54,832 tokens, 1,024 sidecar slots,
3,678,528 sidecar bytes, and 5,512,427 cumulative builder-allocation bytes;
it took 0.17 seconds with 51,748 KiB peak RSS.  These are local workset
evidence, not release thresholds or claims that unsupported structures loaded
successfully.  The commands are the two ignored
`ffi::structured::tests::probe_{gcc,git}_sidecar_workset` tests under
`/usr/bin/time -v cargo test --release -p libmandoc-rs --all-features`.

## Authority and difference ledger

The pinned pristine CVS implementation is the default behavior contract.
Structured collection may observe execution but must not create a second roff
interpreter or change the order in which native handlers and terminal state
advance.

| Classification | Frozen treatment |
| --- | --- |
| Upstream-consistent | Macro execution, emitted characters, explicit breaks, field geometry, table/equation layout, font/escape state, and diagnostic timing follow the pinned CVS for the selected profile. |
| Approved enhancement | Page header/footer decorations and their dedicated whitespace are omitted without suppressing buffered body output or state advancement. |
| Approved enhancement | Evidenced manual links, including `.Xr`, `.MR`, traditional styled `name(section)`, and the bounded Sphinx `\%<>` compatibility case, retain typed targets. The compatibility delta must be listed against the new oracle. |
| Safety boundary | Authorized bundle lookup, include depth and source limits, parser depth/loop limits, output budgets, and temporary-path redaction remain enforced even if unrestricted upstream execution differs. |
| Historical presentation preference | Hidden ordinary `.Lk` addresses, alternative `.Bx` wording, bracket/punctuation rewrites, spacing substitutions, and old `.mc`/`.ti` omissions are not contracts merely because old snapshots expected them. They require independent CVS evidence or explicit approval. |
| Pending verification | Existing tbl source recovery, equation semantic retention, named-glyph handling, multi-tag preservation, long-tail manual-link recognition, and platform-specific rendering must be classified during their vertical slice. Until then they are uncovered, not approved differences. |

For source identity, the relevant pinned path is `read.c:mparse_buf_r() ->
mparse_readmem()/mparse_readfd()`. An `.so` enters a distinct source, resets its
line counter and current filename, then restores the parent values on return.
Consequently, collecting only the current filename during rendering is too
late. For output, `term.c:term_flushln()` consumes and may partially flush the
active field, while `term_ascii.c:ascii_endline()` and `ascii_advance()` also
advance terminal state. Page callbacks in `man_term.c` and `mdoc_term.c` flush
fields and reset state; they cannot simply be replaced by empty callbacks.

## 1. Content, owners, and entry evidence

All keys below are unsigned 32-bit, nonzero, one-based dense indexes into their
named result table, and local to one result. Key `n` selects table index `n-1`.
Zero is the only absent-key sentinel. Keys are never pointers, source line
numbers, mutable traversal indexes, or public `NodeId`s.

```rust
struct StructuredDocument {
    profile: RenderProfile,
    metadata: Metadata,
    sources: Vec<SourceRecord>,
    root_source: SourceKey,
    roots: Vec<ContentRoot>,
    owners: Vec<ContentOwner>,
    atoms: Vec<ContentAtom>,
    points: Vec<ContentPoint>,
    links: Vec<LinkOccurrence>,
    declarations: Vec<DeclarationEvidence>,
    blocks: Vec<NativeBlock>,
    tables: Vec<NativeTable>,
    fixed_views: Vec<FixedView>,
    diagnostics: Vec<NativeDiagnostic>,
}

struct ContentOwner {
    key: OwnerKey,
    kind: ContentOwnerKind,
    roots: Vec<ContentRootKey>,
    blocks: Vec<NativeBlockKey>,
    provenance: Provenance,
}

struct NativeBlock {
    key: NativeBlockKey,
    owner: OwnerKey,
    kind: NativeBlockKind,
    parent: Option<NativeBlockKey>,
    ordinal: u32,
    root: Option<ContentRootKey>,
    children: Vec<NativeBlockKey>,
    provenance: Provenance,
}

struct ContentRoot {
    key: ContentRootKey,
    owner: OwnerKey,
    kind: ContentRootKind,        // heading, term, body, cell, or fixed body
    atoms: Vec<ContentAtomKey>,   // authoritative logical order
    points: Vec<ContentPointKey>, // zero-width positions in this root
    provenance: Provenance,
}

struct ContentAtom {
    key: ContentAtomKey,
    root: ContentRootKey,
    owner: OwnerKey,
    kind: ContentAtomKind,
    provenance: Provenance,
}

enum ContentAtomKind {
    Text {
        text: String,
        display_override: Option<String>,
        style: NativeStyle,
        role: Option<NativeRole>,
        link: Option<LinkOccurrenceKey>,
    },
    Whitespace {
        text: String,
        display_override: Option<String>,
        breakable: bool,
        style: NativeStyle,
        role: Option<NativeRole>,
        link: Option<LinkOccurrenceKey>,
    },
    BreakOpportunity,             // zero text and zero logical scalars
    HardBreak,                    // zero text, one logical `\n` scalar
}

struct ContentRef {
    atom: ContentAtomKey,
    bytes: Range<u32>,            // half-open UTF-8 range in Text/Whitespace
}

struct ContentPoint {
    key: ContentPointKey,
    root: ContentRootKey,
    boundary: PointBoundary,
    scalar_boundary: u32,         // root-relative Unicode-scalar boundary
    owner: OwnerKey,              // must equal the root owner
    provenance: Provenance,
}

enum PointBoundary {
    BetweenAtoms { atom_boundary: u32 },
    InAtom { atom: ContentAtomKey, byte_offset: u32 },
}

struct LinkOccurrence {
    key: LinkOccurrenceKey,
    owner: OwnerKey,
    target: NativeLinkTarget,
    title: Option<String>,        // non-visible advisory metadata
    label: Vec<ContentRef>,
    provenance: Provenance,
}

struct DeclarationEvidence {
    owner: OwnerKey,
    forms: Vec<Vec<ContentRef>>,  // forms and parts remain in source order
    name_hints: Vec<NameHint>,
    relations: Vec<NativeRelation>,
}
```

Every visible semantic character belongs to exactly one text or whitespace
atom. A wrapper such as strong, emphasis, or link supplies attributes and
topology; it does not copy the text. Adjacent atoms with identical attributes
and owner may be coalesced. A `ContentRef` can slice only a UTF-8 boundary of a
text or whitespace atom; byte ranges are canonical and scalar ranges are
checked derivations. Cross-wrapper forms are ordered vectors of refs rather
than source ranges or reconstructed strings.

Atoms split at style, role, link, owner, root, provenance, and logical
connection boundaries. Consequently a link label contains whole linked atoms,
and every atom with `link=l1` occurs exactly once in `l1.label`; form/name refs
may select UTF-8 subranges without changing wrapper ownership.

The public v0.12 IR keeps these roots and atoms as its only inline text store.
Existing inline topology remains, but its leaves become references:

```rust
enum Inline {
    Text { content: ContentRef },
    Code { content: ContentRef },
    Strong { children: Vec<Inline> },
    Emphasis { children: Vec<Inline> },
    Link { occurrence: LinkOccurrenceKey, children: Vec<Inline> },
    Anchor { point: ContentPointKey, id: NodeId, aliases: Vec<FragmentAlias> },
    LineBreak { atom: ContentAtomKey },
}
```

`Text` and `Code` no longer own `String`; `Link` does not copy its target or
title; `Anchor` does not copy an owner span; and `LineBreak` names a
`HardBreak`. Each inline-bearing block, term, heading, or cell names one
`ContentRoot`; its inline leaves cover that root's Text/Whitespace/HardBreak
atoms exactly once and in order. `BreakOpportunity` sits in root order without
an inline leaf, and points are referenced by anchors or empty-object locations.
Wrapper paths remain structural `ContentLocation`s, while their leaves resolve
to the same atom keys used by entry facts, fixed placements, links, search, and
copy. IR validation requires Strong/Emphasis/Code/Link topology to agree with
the referenced atom style/role/link facts; wrappers own structure only. Native
result keys are validated and copied directly into this document store; codec
does not create a second text tree or later remap refs by string.
Logical matching and copy always use `text`. The profile-specific
`display_override`, when present, is a bounded glyph projection mapped back to
the same logical atom; it is never searched or treated as a second body.

Logical connection facts survive the C builder in source-neutral atoms. A
separator consumed by native line wrapping becomes exactly one whitespace atom;
padding and continuation indentation do not. A zero-width legal break becomes
`BreakOpportunity`; the absence of that atom forbids a break between adjacent
text atoms during ordinary word wrapping. A renderer may use a grapheme-safe
emergency visual wrap for a word wider than a flow viewport, but records that
boundary only in its runtime display map; it never changes atoms, copy text,
names, links, or fixed geometry. A hard break becomes `HardBreak`, contributes
one `\n` scalar to
logical text coordinates, and renders a line boundary. `BreakOpportunity` and
points contribute zero scalars and are omitted by logical copy and matching.
The atoms and points serialize in the IR/wire; they are not a permanent native
event history or an instruction for Rust to execute roff.

`OwnerKey` identifies a native heading, list item, definition item, table cell,
fixed display, or other addressable content owner. Owners remain distinct even
when their visible names, source location, or description text are equal. An
empty owner exists without borrowing the following item's body. A zero-width
anchor uses a checked `ContentPoint` either between atoms or at a UTF-8 boundary
inside a text/whitespace atom in one of the owner's roots; no dummy character
is inserted. Its `scalar_boundary` must equal the scalar count derived from all
preceding atom content plus the selected in-atom prefix. A link occurrence owns
its typed target and
ordered label refs. All its labelled atoms carry the same link key. Placements
derive occurrence identity through their target ref; they never store a second
occurrence key. Two separately authored links always get different keys even
when source position, target, and label are equal; repeated placement of one
link reuses its one key.

Native C exports structure and evidence: real head/term/body boundaries,
native roles, explicit relationships, and content refs. It does not construct
`EntryFacts`, infer a complete command grammar, allocate public node IDs, or
derive alias equivalence from layout. `libmandoc-rs` validates and owns those
facts without depending on `mant-ir`. `mant-codec` maps owners to IR, runs the
single source-neutral recognizer, validates refs, and constructs `EntryFacts`,
names, occurrence bindings, aliases, and reading context.

### Worked entry walk-through

The primary synthetic owner `o1` has term root `r1`. Its visible term is
`--output, -o=FILE`, represented without copied form strings:

| Atom | UTF-8 bytes | Attributes / final inline path |
| --- | --- | --- |
| `a1` | `--` (0..2) | strong; `Term { index: 0 } / [0, 0]` |
| `a2` | `output` (0..6) | emphasis inside link `l1`; `Term { index: 0 } / [1, 0, 0]` |
| `a3` | `,` (0..1) | plain punctuation; `Term { index: 0 } / [2]` |
| `a4` | ` ` (0..1) | breakable whitespace; `Term { index: 0 } / [3]` |
| `a5` | `-` (0..1) | strong; `Term { index: 0 } / [4, 0]` |
| `a6` | `o` (0..1) | link `l2`; `Term { index: 0 } / [5, 0]` |
| `a7` | `=FILE` (0..5) | emphasis argument; `Term { index: 0 } / [6, 0]` |

Native declaration evidence is exact and ordered:

```text
f1 = [{atom:a1, bytes:0..2}, {atom:a2, bytes:0..6}]
f2 = [{atom:a5, bytes:0..1}, {atom:a6, bytes:0..1},
      {atom:a7, bytes:0..5}]
name_hint(form:f1) = [{atom:a1, bytes:0..2}, {atom:a2, bytes:0..6}]
name_hint(form:f2) = [{atom:a5, bytes:0..1}, {atom:a6, bytes:0..1}]
l1 = {owner:o1, target:Manual("output", "1"), label:[a2:0..6]}
l2 = {owner:o1, target:Section("argument-file"), label:[a6:0..1]}
```

Codec maps `o1` to one `DefinitionItem`, preserves its description block as the
body provider, and projects those refs to two `EntryForm`s. Each final
`EntryContentSlice` is `{ location: <root/path above>, content: ContentRef }`;
its bytes remain atom-local and it owns no copied string.
The second full form has seven Unicode scalars (`-o=FILE`), while its name range
is `0..2`; the first form and name range are both `0..8`.
The resulting `EntryFacts.names` are `--output` and `-o`, and each
`EntryNameBinding` points to its exact form occurrence. Neither range is a roff
source range or a terminal-column range.

The same walk-through includes these independent owners:

| Owner | Required object relation and result |
| --- | --- |
| `o2` | mdoc `.Fl q` term root `r2` has `a8="-q"` and native flag role; Rust constructs an option entry. A prose `.Fl` outside an owner remains prose. |
| `o3` | term root `r3` has `a9="--empty"`; body root list is empty. It remains addressable and never borrows `o4`'s body. |
| `o4` | linked term `a10="--same"`, body root `r5="body A"`, occurrence `l3`. |
| `o5` | linked term `a11="--same"`, body root `r7="body B"`, occurrence `l4`. Equal spelling/target/source coordinates do not merge `o4`, `o5`, `l3`, or `l4`. |

Invalid or no-longer-visible evidence is rejected as a unit and emits a
bounded `evidence-dropped` diagnostic carrying the owner and the best available
provenance; it does not manufacture a fallback name. The C producer supplies
roles and ordered refs only. Rust is the sole owner of form validation,
source-neutral name grammar, `EntryFacts`, public IDs, aliases, and reading
context.

### Logical connection across native lines

`soft-wrap` is not a sufficient boolean. Before native field processing
consumes a space, rewrites a breakable hyphen, converts a non-breaking space,
or drops a zero-width break marker, the annotation sidecar records its pending
atom. The C builder commits the surviving atom while constructing the single
logical body; Rust never joins physical lines or removes trailing hyphens.

For the oracle input below, `.ll 18n` forces the relevant field decisions while
the selected native output profile remains 78 columns:

```roff
.TH WRAP 1 "2026-09-20"
.SH TEST
.ll 18n
abcdefgh-ijklmnopqrst uvwxyz
```

The body line was replaced in turn with the following six cases after oracle
preflight. The logical mapping is frozen as follows:

| Body fact | Native placement observation | Authoritative logical relation |
| --- | --- | --- |
| `abcdefgh ijklmnopqrst uvwxyz` | ASCII/UTF-8 wrap at ordinary spaces | `Whitespace(" ", breakable=true)` exactly once; no output indentation atom |
| `abcdefgh-ijklmnopqrst uvwxyz` | ASCII/UTF-8 wrap after the existing `-` | text atom retains `-`, followed by `BreakOpportunity`, then adjacent text |
| `abcdefgh\:ijklmnopqrst uvwxyz` | ASCII wraps there; UTF-8 keeps one over-wide word | ASCII profile inserts `BreakOpportunity`; default UTF-8 profile inserts no atom and no character |
| `abcdefgh\%ijklmnopqrst uvwxyz` | both devices keep the first two runs together | no atom and no invented hyphen or space |
| `abcdefgh\&-ijklmnopqrst uvwxyz` | ASCII/UTF-8 keep the protected hyphenated word together | text atom retains `-`; no following `BreakOpportunity` |
| `abcdefgh\~ijklmnopqrst uvwxyz` | ASCII displays a plain space, UTF-8 displays NBSP; neither breaks there | `Whitespace("\u{a0}", breakable=false)` logically in both profiles; ASCII must carry `display_override=" "`, UTF-8 has none |

For the first case, if `a20="abcdefgh"` is placed on native line one and
`a21="ijklmnopqrst"` on line two, the consumed source space becomes one logical
whitespace atom `a22=" "` between them. `a22` has no visible placement at that
native wrap; it is nevertheless part of names, search, and logical copy ranges.
For default UTF-8 `\:` there is no `a22`; `a20` and `a21` are
adjacent and remain one logical occurrence. ASCII instead inserts a zero-scalar
`BreakOpportunity`, which may give that occurrence two placements without
changing copied text. A hard `\p` materializes an executed `HardBreak` at the
position where it takes effect and contributes one logical newline scalar.
Field flush or physical endline alone never manufactures that hard break.

The pinned implementation basis is `term.c:term_fill()`, which consumes
automatic-wrap spaces and rewrites `ASCII_HYPH`, `term_field()`, which skips
zero-width break markers, `term_word()`, which maps escapes into buffer control
bytes, `roff.c:roff_parsetext()`, which marks eligible authored hyphens, and the
device mapping in `chars.c`. The ASCII/UTF-8 differences for `\:` and `\~` are
retained as profile behavior; this migration does not normalize the devices or
add dictionary hyphenation.

## 2. Fixed displays and tables use one body

A fixed view is geometry over logical content, not another text document.

```rust
struct FixedView {
    owner: OwnerKey,
    lines: Vec<FixedLine>,
}

struct FixedLine {
    key: FixedLineKey,
    placements: Vec<Placement>,
    decorations: Vec<Decoration>,
    terminal_columns: u32,
}

struct Placement {
    key: PlacementKey,
    target: PlacementTarget,
    start_column: u32,
    end_column: u32,
    root_scalar_range: Range<u32>,
    map: CellMapKind,
}

enum PlacementTarget {
    Content(ContentRef),
    Point(ContentPointKey),
}

enum CellMapKind {
    Affine { columns_per_scalar: u8 },
    GraphemeCluster,
    Overlay,
}

struct Decoration {
    key: DecorationKey,
    text: String,
    start_column: u32,
    width_columns: u32,
    kind: DecorationKind,
}
```

Placements on a line do not overlap unless an explicitly supported overstrike
relationship says so. A content placement is split at extended-grapheme and
non-linear movement boundaries. Contiguous ASCII-like scalars may use an affine
segment; a wide or combining cluster uses one `GraphemeCluster` segment; an
overstrike uses explicit `Overlay` segments at the same columns. Consequently,
each terminal-column intersection maps to a whole grapheme and a checked
content byte/scalar subrange. UTF-8 ranges, Unicode scalar ranges, and terminal
column ranges are validated separately. `root_scalar_range` is relative to the
target's root and must equal the prefix sum implied by the content ref; a point
placement uses its checked `scalar_boundary` for both endpoints. Point
placements have an empty column range at their exact column. `Decoration` is
limited to generated
border, rule, or padding glyphs that are not semantic content. Authored spaces,
non-breaking spaces, generated text with semantic meaning, and link labels stay
in the logical body. A physical line is materialized from placements and
decorations only at a display/export boundary.

Consider a two-column table whose first logical cell spans two rows, whose
second-row right cell is empty, and whose left cell contains a link split over
two physical lines:

```text
logical table
  row r1: cell a(rowSpan=2) = [c10:"alpha ", c11:"manual"]
          cell b             = [c12:"value"]
  row r2: cell c             = [] at point p3

fixed view
  line f1: place c10@col0, place c12@col18, decoration "│"@col16
  line f2: place c11@col2 (content carries link l7), place point p3@col18
```

Cells `a`, `b`, and `c` are the authoritative logical table objects. The span
does not clone cell `a`. The two placements of `c10`/`c11` render one logical
link occurrence `l7`, so reference and explanation scans count it once. The
empty cell is revealable through `p3` without fabricated text. A repeated
header may place the same occurrence again without increasing its logical
count; two separately authored identical links have distinct occurrence keys.

Consumer paths are frozen as follows:

| Consumer | Required path |
| --- | --- |
| Entry, explain, references | Traverse logical blocks/cells once; never rescan placements for candidates. |
| Interactive search | Match logical text once, then map the one hit to every visible placement required for highlighting. |
| Protocol search | Match the same logical roots once. Return the logical root/range plus zero or more canonical-Markdown projection ranges; repeated fixed placements or Markdown degradation never increase the logical hit count. |
| Click/reveal | Map the visible placement back to its content ref, link occurrence, owner, and logical address. |
| Logical copy | Emit text/whitespace atoms and `HardBreak` newlines; omit break opportunities, points, padding, borders, and rules. |
| Fixed visual copy | Project profile glyphs (including display overrides) in the selected full-result physical line/column interval, including visible border/rule decorations and internal padding; do not add viewport-clipped columns or right padding beyond the selection. A point copies no glyph. |
| Copy target / read entry | Use the typed target / logical owner, never screen glyphs or decorations. |
| JSON | Preserve logical content and checked view relations; do not serialize a second complete fixed string. |
| Markdown | Simple tables use ordinary Markdown. Complex fixed views degrade to documented fenced literal text; style and click ranges are lost, but the IR variant is not reclassified as code. |

## 3. Sources, addresses, and the v0.12 wire

### Source table and provenance

```rust
struct SourceRecord {
    key: SourceKey,
    identity: SourceIdentity,
    format: SourceFormat,
    decoded_byte_len: u64,
    content_sha256: Option<[u8; 32]>,
    coordinates: SourceCoordinates,
}

enum SourceIdentity {
    Path(String),                // caller-visible logical path
    BundleMember(String),        // normalized authorized bundle member
    Anonymous(String),           // bounded caller label, not a fake path
}

enum SourceCoordinates {
    DecodedUtf8Bytes,            // exact ranges and decoded-byte columns
    NativeNormalizedBytes,       // line/column only after native conversion
}

struct SourceSpan {
    source: SourceKey,
    line: u32,                   // one-based
    column: u32,                 // one-based unit selected by SourceCoordinates
    end_line: Option<u32>,
    end_column: Option<u32>,     // one-based exclusive
    byte_range: Option<Range<u64>>,
}

enum Provenance {
    Authored(SourceSpan),
    Generated { trigger: Option<SourceSpan> },
    Unknown,
}
```

The source table contains the root and only members that actually participate
in parsing or execution. Keys are assigned on first actual entry; the root
enters first and therefore has key one, and `root_source` must reference it.
Input authorization slots are a separate call-local domain. Re-including an
identical logical member with identical bytes may reuse its `SourceKey`, but
each include occurrence, owner, and content occurrence remains distinct. A key
is valid only in its result and is not a public identity or access capability.

`SourceRecord.format` preserves the required man/mdoc/Markdown fact. Every
participating roff include uses the active root macroset; Markdown documents use
the same source-table model without entering this native producer.

Exact byte offsets are into the caller-visible, decompressed UTF-8 input bytes,
starting at byte zero and including any leading UTF-8 BOM. Lines recognize LF,
CRLF, and a final unterminated line; terminator bytes belong to byte offsets but
not to a line's column domain. In `DecodedUtf8Bytes`, column is a one-based UTF-8
byte position in the decoded line content; tab is one byte and is never expanded
to terminal columns. If the first line starts with a BOM, column one maps to
absolute byte offset three while exact byte ranges still include the BOM when
they cover it. The bounded source map records this adjustment and any CRLF
translation explicitly. For gzip/zstd the compressed container offset is never
a source offset.

The pinned parser's native `pos` is a byte index in its possibly converted line
buffer, not a Unicode character index. If a checked original mapping is not
available (for example Latin-1 converted to native escape spelling), the record
uses `NativeNormalizedBytes`, omits exact byte ranges, and labels line columns
as one-based native-normalized byte positions. It never calls those positions
original UTF-8 or scalar columns. Mapping tables and their bytes are bounded.
Generated output has no source byte range; its optional trigger describes why
it exists, not where its glyphs were authored. Unknown provenance never defaults
to the root.

Every authored span is checked against the record selected by its own key:
known key, nonzero line/column, ordered endpoints, and range within that
source's decoded length. While input bytes are available, native transfer also
checks UTF-8 boundaries and line/byte consistency; self-contained wire decoding
cannot re-prove those facts because the source bytes are intentionally absent.
A range cannot cross sources. Diagnostics and semantic evidence use the same
source-qualified span.

Structural decoding rejects an unknown key or a range outside the selected
record. It cannot determine that another valid, sufficiently long source key is
factually wrong. Wrong-valid-source tests therefore use a known multi-source
fixture/oracle and compare the producer's independently known include/run
binding; they are semantic integration negatives, not claims about Serde alone.
Temporary decompression paths, `/proc/self/fd` paths, native handles, and
unauthorized filesystem fallback never enter `SourceIdentity`.

Source identity is bound while `mparse_readmem()`/`mparse_readfd()` enters a
root or include and is copied to nodes, diagnostics, evidence, and annotated
buffer runs at creation time. The first collector must preserve it. Codec
renumbering must update every content span, diagnostic, and evidence reference
before the native result is released.

### Logical and visible coordinates

Three units remain disjoint:

- source coordinates: UTF-8 bytes plus proven line/column within one source;
- logical coordinates: canonical UTF-8 byte ranges in atoms, with checked
  Unicode-scalar half-open ranges derived for explain/name projections; and
- fixed geometry: terminal-cell columns within one native physical line.

A root's logical sequence concatenates Text/Whitespace bytes and one `\n` byte
for each `HardBreak`; break opportunities and points contribute zero bytes and
scalars. Root-relative search ranges use that sequence. Atom-local
`ContentRef.bytes` and root-relative byte/scalar ranges are converted by checked
prefix sums, never by terminal columns.

`ContentLocation` remains a typed path through document heading, section
heading, blocks, list/definition owners, table cells, and inline wrappers.
`ContentPoint` adds a boundary position for empty owners and zero-width targets.
`FixedLocation` contains a fixed block path, line key, and terminal column
range. `FlowLocation` is not stored in IR or wire because it depends on the
current viewport. Each renderer builds a bounded runtime display map:

```rust
struct DisplaySegment {
    surface: SurfaceKey,
    display_line: u32,
    columns: Range<u32>,
    target: DisplayTarget,
    content_bytes: Option<Range<u32>>,
    content_scalars: Range<u32>,
    map: CellMapKind,
}

enum DisplayTarget {
    Content(ContentRef),
    Point(ContentPointKey),
    Decoration(DecorationKey),
}
```

Flow layout emits content/point segments while wrapping logical atoms and uses
their profile display override only for glyph production. Fixed
layout derives all three target kinds from persisted placements/decorations and
applies horizontal viewport clipping only after the full map exists. Content
and decoration segments split at grapheme/non-linear movement boundaries, so
hit testing and visual copy never select half a wide or combining glyph. The
resolver provides both directions:

Decoration segments have `content_bytes=None` and an empty scalar range. Point
segments are likewise empty; only content segments carry logical byte/scalar
coverage.

```text
logical ContentRef/ContentPoint -> zero or more fixed/runtime display segments
visible display segment -> exactly one logical ref/point or Decoration
logical occurrence -> its owner, typed target/evidence, and all placements
```

Viewport offsets are not serialized. A hidden right-hand placement is still in
the result and resolves identically after horizontal movement or resize.

Public explanation and name-binding ranges remain owner/form-relative Unicode
scalar ranges derived from checked `ContentRef` bytes. Public
`mant.search/v0.12` now matches logical roots and identifies each hit by root
plus UTF-8 byte/scalar range. Its `markdownProjections` array carries any
canonical Markdown v1 byte/line ranges recorded by the encoder's source map;
it is a display projection, not match authority. A repeated table header can
therefore yield several projections/highlights for one hit. TUI search uses the
same logical hit plus the runtime display map and must not synthesize spaces
from physical `join_before` flags. Logical, Markdown-projection, and terminal
coordinate domains stay distinct.

The v0.12 `SearchQuery` removes `scope`; the old `scope: visible|markdown`
field is rejected by real Serde decoding. Literal/regex, case, and word options
apply to logical root sequences, pagination counts logical hits, and context is
derived from logical blocks/HardBreak boundaries before optional Markdown or
display projection. Searching generated CommonMark markup is no longer part of
the document-search protocol; a future representation-inspection API would be
a separate contract.

### Unpublished v0.12 rewrite policy

The v0.12 IR/wire is not released and is rewritten in place for this migration;
crate and schema versions are not bumped merely for the refactor. No compatibility
reader or legacy AST fallback is retained. Persisted document caches must carry
a structured-rendering format fingerprint and invalidate old records.

The final field/discriminator changes to implement in C09a are frozen here:

| Shape | Final v0.12 contract |
| --- | --- |
| `Document` | replace singular `source` authority with nonempty `sources` and `rootSource`; add the single `contentStore` containing owners/roots/atoms/points/links/fixed views; keep metadata/content/diagnostics |
| `SourceRecord` | closed object: `key`, `identity`, `format`, `decodedByteLength`, optional `contentSha256`, `coordinates` |
| `SourceSpan` | closed object: required `source`, line/column, optional end and exact byte range |
| provenance | closed tagged union: `authored`, `generated`, `unknown`; generated may carry `trigger` only |
| content owner | closed object: `key`, owner kind, ordered roots/blocks, and `provenance` |
| content root | closed object: `key`, `owner`, root kind, ordered `atoms`/`points`, and `provenance` |
| content atom | closed tagged union: `text`, `whitespace`, `break-opportunity`, `hard-break`; text/whitespace own logical text and optional bounded `displayOverride`; hard break counts one scalar and break opportunity counts none |
| content point | closed object: `key`, `root`, tagged `boundary`, `scalarBoundary`, `owner`, `provenance`; the boundary is either between atoms or a UTF-8 position in a root atom, and its scalar value is checked against that root |
| link occurrence | closed object: `key`, `owner`, typed `target`, ordered `label` refs, and `provenance` |
| inline leaves | `text`/`code` carry only `content`; `line-break` carries a HardBreak atom; `anchor` carries a point; `link` carries an occurrence key; wrappers retain children but no leaf owns visible text |
| block discriminator | retain `type`; add `fixed-display`; every inline-bearing block/heading/term/cell carries a root; equation/unsupported visible output also uses a root rather than `value`/`text`; `preformatted` continues to mean code/literal source semantics, not native fixed geometry |
| table | logical `rows` remain authoritative; optional `fixedView` references their content rather than copying it |
| fixed view | closed lines containing checked `placements` and `decorations`; placement target is tagged `content` or `point` and carries scalar/terminal mapping |
| content reference | closed object with result-local `atom` key and half-open UTF-8 atom byte range |
| logical location | retain typed heading/section/content paths; add checked point and fixed-line roots without embedding viewport state |
| search query | remove `scope`; preserve pattern/syntax/case/word/context/limit/offset with logical-hit semantics and reject the old field |
| search hit | replace Markdown-only authority with required logical root and UTF-8 byte/scalar range; add `markdownProjections` as a possibly empty array of canonical Markdown v1 coordinates |

All new tagged unions use explicit kebab-case discriminators and all structural
objects use `deny_unknown_fields`. Old singular-source documents, copied inline
strings, old fixed text copies, unknown keys, a range outside its selected
source, unrecognized variants, extra fields, and a known discriminator with the
old meaning must fail real Serde decoding plus structural validation. A valid
but factually wrong source key remains the producer/oracle semantic negative
defined above. Schema snapshots are regenerated only with real decoding tests;
a JSON Schema snapshot alone is not evidence of rejection.

Every document-bearing single-document protocol response replaces its copied
root `source` with a closed `sourceContext { sources, rootSource }`. This covers
document, outline, excerpt, explanation, and search responses, so every emitted
`SourceSpan.source` resolves in the same envelope. Scope responses carry one
source context per loaded document record. Catalog rows remain root summaries
and derive format/path from the root record; they do not copy include tables.
UI format dispatch likewise resolves `document.root_source` through the dense
source table and reads that record's format.
Decoders reject a missing/empty table, duplicate/non-dense keys, an invalid
root, and the old singular `source` field. Persistent caches include a format
fingerprint and invalidate the old shape.

## 4. FFI and lifetime contract

The structured ABI is private to `libmandoc-rs`, but its minimum interface is
frozen. All enum and status fields are `uint32_t`; keys and collection counts
are `uint32_t`; source sizes/ranges, aggregate counters, and byte lengths are
`uint64_t`; terminal columns and logical scalar offsets are `uint32_t`.
Booleans are `uint8_t` and must be 0 or 1. Every view has explicit reserved
bytes initialized to zero.

```c
struct mant_bytes_view { const uint8_t *ptr; uint64_t len; };
struct mant_slice_view { const void *ptr; uint32_t count; uint32_t stride; };

enum mant_structured_status {
    MANT_STRUCTURED_OK = 0,
    MANT_STRUCTURED_INVALID_INPUT = 1,
    MANT_STRUCTURED_REENTRANT = 2,
    MANT_STRUCTURED_BUDGET = 3,
    MANT_STRUCTURED_BUILDER_ALLOC = 4,
    MANT_STRUCTURED_NATIVE = 5,
    MANT_STRUCTURED_RELATION = 6,
    MANT_STRUCTURED_UNSUPPORTED = 7,
};

struct mant_input_source_view {
    uint32_t identity_kind;
    uint32_t format;
    struct mant_bytes_view logical_name;
    struct mant_bytes_view resolver_name;
    struct mant_bytes_view source_bytes;
    uint32_t reserved;
};

enum mant_structured_resolve_status {
    MANT_RESOLVE_FOUND = 0,
    MANT_RESOLVE_NOT_FOUND = 1,
    MANT_RESOLVE_DENIED = 2,
    MANT_RESOLVE_IO = 3,
    MANT_RESOLVE_PANIC = 4,
    MANT_RESOLVE_INVALID = 5,
};

typedef uint32_t (*mant_structured_resolve_fn)(
    void *context,
    uint32_t current_input,
    struct mant_bytes_view requested_name,
    uint32_t *out_input);

struct mant_structured_input_view {
    struct mant_slice_view sources;
    uint32_t root_input;
    uint32_t profile;
    uint32_t width;
    mant_structured_resolve_fn resolve;
    void *resolve_context;
    uint32_t reserved;
};

struct mant_structured_failure_view {
    uint32_t status;
    uint32_t stage;
    uint32_t limit_kind;
    uint64_t observed;
    uint64_t allowed;
    uint32_t reserved;
};

uint32_t mant_structured_abi_version(void); /* exactly 1 for this contract */
uint64_t mant_structured_discriminant_fingerprint(void);
uint32_t mant_structured_render(
    const struct mant_structured_input_view *,
    const struct mant_structured_limits *,
    struct mant_structured_result **out_result,
    struct mant_structured_failure_view *out_failure);
uint32_t mant_structured_result_check(
    const struct mant_structured_result *,
    struct mant_structured_failure_view *out_failure);
uint32_t mant_structured_result_view(
    const struct mant_structured_result *,
    struct mant_structured_result_view *out);
void mant_structured_result_free(struct mant_structured_result *); /* NULL-safe */

size_t mant_structured_view_size(uint32_t view_kind);
size_t mant_structured_view_align(uint32_t view_kind);
size_t mant_structured_view_offset(uint32_t view_kind, uint32_t field);
```

View-kind IDs are frozen in this order: input source `1`, input `2`, failure
`3`, limits `4`, result `5`, then source/span/provenance/owner/content root/
content atom/content ref/content point/link/block/table/table row/table cell/
fixed view/fixed line/placement/decoration/form/name hint/relation/diagnostic as
`6..26`, and document metadata as `27`. Field IDs are one-based in declaration
order; zero is invalid. Metadata was added when the first vertical session
proved that the parser/tree must be released before result return, leaving no
sound later source for title, section, date, OS, architecture, name, alias,
macroset, or `hasBody`.
`view_size`/`view_align` return zero for an invalid kind and `view_offset`
returns `SIZE_MAX` for an invalid kind or field. Every ABI record except the
generic byte/slice views ends with an explicit `uint32_t reserved` field (plus
compiler-required padding) that producers zero and consumers reject when
nonzero. The checked-in C header and Rust mirror created in C02a are the
executable field declarations. A deterministic FNV-1a fingerprint over all
closed discriminant domains is exported by the C side and independently
recomputed by the Rust ABI test; changing a field, ID, or discriminant requires
updating this contract, the ABI version/fingerprint, and F review.

Core discriminants use zero only for invalid/absent and the following nonzero
values:

| Domain | Values |
| --- | --- |
| identity | `path=1`, `bundle-member=2`, `anonymous=3` |
| format | `man=1`, `mdoc=2`, `markdown=3` |
| profile | `utf8=1`, `ascii=2` |
| source coordinates | `decoded-utf8-bytes=1`, `native-normalized-bytes=2` |
| provenance | `authored=1`, `generated=2`, `unknown=3` |
| atom | `text=1`, `whitespace=2`, `break-opportunity=3`, `hard-break=4` |
| style bits | `bold=1<<0`, `italic=1<<1`, `literal=1<<2`, `underline=1<<3` |
| native role | `flag=1`, `environment-variable=2`, `argument=3`, `command-or-directive=4`, `path=5` |
| point boundary | `between-atoms=1`, `in-atom=2` |
| placement target | `content=1`, `point=2` |
| cell map | `affine=1`, `grapheme-cluster=2`, `overlay=3` |
| link target | `external=1`, `email=2`, `document=3`, `manual=4`, `section=5` |
| table cell | `text=1`, `horizontal-rule=2`, `double-horizontal-rule=3`, `isolated-horizontal-rule=4`, `isolated-double-horizontal-rule=5` |
| table alignment | `left=1`, `center=2`, `right=3`; zero is absent |
| decoration | `border=1`, `rule=2`, `padding=3` |
| relation | `alias=1`, `reading-context=2` |
| diagnostic level | `style=1`, `warning=2`, `error=3`, `unsupported=4` |
| diagnostic code | pinned native `mandocerr` ordinal plus one, currently `1..210` |
| native stage | `marshal=1`, `resolve=2`, `parse=3`, `render=4`, `finalize=5`, `check=6` |

Failure `limit_kind` is zero when unrelated to a limit and otherwise is the
one-based field ID in `mant_structured_limits`. Owner/root/block/role/relation,
table, and diagnostic enums are closed semantic enums listed by the generated
C02a header and v0.12 schema; zero remains invalid/absent, additions require the
same contract/fingerprint review rather than silent passthrough.

Input byte/path pointers are borrowed for the synchronous call only and are
never retained by a result. A zero-length byte/slice view is exactly
`{ptr=NULL,len/count=0}`; a nonempty view requires a non-null aligned pointer.
`stride` must equal the queried native size of that element view. Strings are
length-delimited UTF-8 and never NUL-terminated. `root_input` and callback
input values are one-based `InputSourceSlot`s into `sources`; they are
authorization-table positions, not result `SourceKey`s. Profile is `utf8` or
explicit test/library `ascii`, and width is positive.
Each input source contains identity kind, format, caller-visible logical-name
bytes, a normalized relative `resolver_name`, and source bytes. `Path` logical
names may be absolute, `Anonymous` logical names are non-path labels, and only
`BundleMember` logical names must themselves be normalized relative names; for
bundle members `logical_name == resolver_name` is required. Resolver names are
the authorization keys and are never copied into the result identity. The
native entry accepts man or mdoc roots; Markdown uses the shared
public source model but is rejected by this native call. All supplied sources
remain alive through the
last native resolver callback. If the platform bridge uses a resolver callback,
`current_input` identifies the including authorized source and
`requested_name` is the exact authored logical include name. The callback
applies the authorized exact-then-current-directory lookup policy and returns
`FOUND` with a nonzero slot already present in `sources`; all other statuses set
`out_input` to zero. C revalidates the slot, identity, normalized path, and
root/beside authorization before reading it. The callback catches Rust panic as
`PANIC`; no callback pointer is retained. Bundle lookup never falls back to the
host filesystem. The preferred in-memory path performs the identical lookup
directly over the input-source slice without a callback.

When a root or include first actually enters `mparse_readmem()`, the builder
assigns the next dense result `SourceKey` and records
`InputSourceSlot -> SourceKey`. Unused authorized inputs never enter the result.
Repeated entry of the same slot reuses its result key while owners/content
occurrences remain distinct. Result `rootSource`, every span, diagnostic,
evidence item, and annotated run uses only the result key domain; no finalize
renumbering is required.

`render` returns `OK` only with a non-null result and a zeroed failure view. On
every non-OK status, `out_result` is null and the caller-owned failure view
contains only `status`, `stage`, `limitKind`, `observed`, and `allowed`; it owns
no pointer. Statuses distinguish invalid input, same-thread re-entry, budget,
builder allocation, native parse/render, internal relation failure, and an
explicit unsupported-coverage status with the numeric values above. C fully
cleans partial state before returning an error. In C02a, a document with body
content returns `UNSUPPORTED` rather than a successful empty body; C02b removes
that restriction only for the body shapes its collector completely covers.
`result_check` is idempotent, returns `OK` only for a complete internally valid
handle, and uses the same zero/failure rule. `result_view` rejects a null or
unchecked handle and zeroes `out`; it never transfers ownership. Invalid
view/field queries return zero size/alignment or `SIZE_MAX` offset. Rust
marshalling and owned-copy errors use Rust error variants and still free any
successful native handle.

### Result views and topology

One `mant_structured_result_view` contains, in order, `rootSource`, profile,
width, the document metadata view, then typed `mant_slice_view` fields for `sources`,
`spans`, `provenances`, `owners`, `contentRoots`, `contentAtoms`,
`contentRefs`, `contentPoints`, `links`, `blocks`, `tables`, `tableRows`,
`tableCells`, `fixedViews`, `fixedLines`, `placements`, `decorations`, `forms`,
`nameHints`, `relations`, and `diagnostics`, followed by its reserved word. These
are result-owned immutable
tables. Keys are always `index + 1`; every `first/count` pair is a zero-based
sub-slice into the named unkeyed descriptor table, currently `contentRefs`.
Other parent/child collections use parent keys plus an explicit ordinal so
they do not require incompatible contiguity orderings.

`span` and `provenance` are immutable descriptor tables addressed internally by
one-based `uint32_t` indexes. Zero means absent only for an optional authored or
trigger span. Every object-level `provenance` field is a nonzero provenance
index; the selected record has exactly one active representation. These
descriptor indexes do not escape as public document identity.

| View | Frozen fields |
| --- | --- |
| `metadata` | macroset, presence flags, title, section, volume, operating system, architecture, name, date, alias target, `hasBody` |
| `source` | `key`, identity kind, `format`, coordinate kind, logical-name bytes, decoded length, hash-present, 32-byte hash |
| `span` | present flags, `source`, line/column/end values, byte-range-present, `byteStart`, `byteEnd` |
| `provenance` | kind `authored/generated/unknown`, authored span, optional generated trigger span |
| `owner` | `key`, owner kind, provenance |
| `content_root` | `key`, `owner`, owner-local ordinal, root kind, provenance |
| `content_atom` | `key`, `root`, root-local ordinal, `owner`, atom kind, `styleFlags`, `role`, `link`, logical-text bytes, display-override-present/bytes, whitespace-breakable, provenance |
| `content_ref` | `atom`, `byteStart`, `byteEnd` |
| `content_point` | `key`, `root`, root-local ordinal, `owner`, boundary kind, `atomBoundary` or `atom/byteOffset`, `scalarBoundary`, provenance |
| `link` | `key`, `owner`, target kind, `targetA`, target-B-present/`targetB`, title-present/title, `firstLabelRef/labelRefCount`, provenance |
| `block` | `key`, `owner`, block kind, parent key, ordinal, provenance, optional root/table/fixed key |
| `table` | `key`, owning block, optional fixed-view key, provenance |
| `table_row` | `key`, table key, table-local ordinal, provenance |
| `table_cell` | `key`, row key, row-local column ordinal, owner, kind/alignment, row/column span, provenance |
| `fixed_view` | `key`, owner, nonzero `block` xor nonzero `table`, provenance |
| `fixed_line` | `key`, view key, view-local ordinal, total columns |
| `placement` | key, line, line-local ordinal, target kind, inline content-ref fields or point key, root-scalar start/end, column start/end, cell-map kind/value |
| `decoration` | `key`, line, line-local ordinal, decoration kind, text bytes, column start/end, provenance |
| `form` | `key`, owner, role, `firstRef/refCount`, provenance |
| `name_hint` | `key`, form key, `firstRef/refCount`, provenance |
| `relation` | `key`, owner, relation kind, target owner, provenance |
| `diagnostic` | level/code, message bytes, optional source-qualified span, owner key when bound |

In this table every key, enum, flag set, ordinal, count, scalar endpoint, and
column is `uint32_t`; booleans/presence bits are `uint8_t`; source sizes and
exact source byte endpoints are `uint64_t`; strings are `mant_bytes_view`.
`targetA/targetB` mean URI/absent, address/absent, document/fragment,
manual-name/manual-section, or section-id/absent for the five link kinds in
order. Optional B/title views are null/zero unless their preceding presence bit
is one. Style bits are `strong=1`, `emphasis=2`, and `code=4`; unknown bits are
rejected. Role and the remaining semantic enum fields follow the closed header
set described above.

Tagged-union inactive fields are zero. A placement target is exactly one of
content or point. A content placement refers only to Text/Whitespace; a point
placement has equal root-scalar and terminal endpoints. Ordinals are dense from
zero within each parent and define public vector order. For a block with
`parent=0`, `ordinal` is dense among that owner's top-level blocks; otherwise
the parent is a block with the same owner and the ordinal is dense among its
children. `ContentOwner.blocks` contains those top-level keys. A block's
optional root/table/fixed fields obey its kind and all inactive fields are zero.
A fixed view belongs directly to exactly one block or table; a table-owned view
reaches its block through the table. Every in-atom point
names a Text/Whitespace atom in the same root and a UTF-8 byte boundary.
`content_atom.link` must
refer to a link whose owner matches and whose label contains that atom range.
Placements contain no occurrence key: link/reference identity is derived from
the logical target. Blocks form an acyclic forest, table spans resolve within
their grid, all sub-slices are in range, and each child has one logical parent.

The C header and Rust `repr(C)` declarations contain these fields in the same
order. Tests query size, alignment, and every consumed field offset for every
view kind, not only the largest structs. Rust calls `result_view` once, validates
all slice ptr/count/stride triples before dereference, then copies in this order:
sources/spans, owners/roots/atoms/points, links/evidence, blocks/tables/fixed
relations, diagnostics. All arithmetic and `try_reserve` calls precede slicing
or allocation.

### Limits

`mant_structured_limits` contains these `uint64_t` fields; zero is invalid, not
“unlimited”. The Rust constructor supplies the recorded defaults:

| Field | Default |
| --- | ---: |
| `maxInputSources` / `maxSources` | 4,096 / 4,096 |
| `maxSourcePathBytes` | 4 MiB |
| `maxDecodedSourceBytesPerSource` / `maxDecodedSourceBytesTotal` | 64 MiB / 256 MiB |
| `maxSourceMapEntries` / `maxSourceMapBytes` | 4,194,304 / 64 MiB |
| `maxBuilderOperations` / `maxBuilderAllocatedBytes` | 67,108,864 / 256 MiB |
| `maxContentBytes` | 128 MiB |
| `maxOwners` / `maxBlocks` / `maxContentAtoms` | 1,048,576 / 1,048,576 / 4,194,304 |
| `maxContentRefs` / `maxContentPoints` / `maxLinks` | 8,388,608 / 1,048,576 / 1,048,576 |
| `maxTables` / `maxTableRows` / `maxTableCells` | 1,048,576 / 1,048,576 / 4,194,304 |
| `maxFixedViews` / `maxFixedLines` | 1,048,576 / 1,048,576 |
| `maxPlacements` / `maxDecorations` | 8,388,608 / 8,388,608 |
| `maxForms` / `maxNameHints` / `maxRelations` | 1,048,576 / 1,048,576 / 4,194,304 |
| `maxConnectionAtoms` / `maxAnnotationRuns` | 4,194,304 / 4,194,304 |
| `maxAnnotationMutations` | 16,777,216 |
| `maxRelationEdges` / `maxDiagnostics` | 8,388,608 / 65,536 |
| `maxTransferObjects` / `maxTransferEdges` | 16,777,216 / 16,777,216 |
| `maxTransferBytes` | 512 MiB |
| `maxNestingDepth` / `maxIncludeDepth` | 256 / 64 |

Counters use checked `uint64_t` addition/multiplication and reject before growth.
`maxInputSources` bounds the authorization slice, `maxSources` bounds the dense
participating result, and `maxSourcePathBytes` is aggregate across each input's
identity name plus its distinct resolver name (a bundle member's equal shared
name is charged once). Both decoded-byte limits apply simultaneously.
Independently, every table count/key, atom-local byte endpoint, scalar endpoint,
and terminal column must fit its fixed `uint32_t` field; reaching the sentinel
space is a budget error. Separator bytes, zero-width connection atoms,
source-line mapping entries/bytes, and graph edges are charged explicitly even
when they add no final visible bytes. `maxContentBytes` includes logical atom
text, display overrides, decoration glyphs, link components, and diagnostic
messages. Builder operations charge every accepted/mutating sidecar action and
every object/edge append. Transfer limits independently charge each validated
record, traversed relation, and copied byte before Rust allocation. A later
evidence-based default change
requires updating this table and F review; callers may select lower positive
limits.

### Ownership and failure state

One synchronous call owns this sequence:

```text
Rust input/bundle/config
  -> thread-local re-entry guard
  -> native session + parser/tree + structured builder
  -> collector finalizes independent result arrays
  -> parser/tree/renderer/sidecars free before native success return
  -> opaque result handle with immutable checked views
  -> Rust validates and copies an owned StructuredDocument
  -> result arrays/strings free
  -> re-entry guard releases
```

| Memory | Owner and release point |
| --- | --- |
| caller input, bundle, config | Rust caller; alive through the last native read/callback, never retained by handle |
| path CStrings, input descriptors, resolver context | Rust call frame; dropped after native can no longer callback |
| re-entry token | outermost Rust RAII guard; acquired before installing other TLS, released last on every path |
| parser/tree/meta and renderer/tcols/buffers | C session; exactly-once cleanup after collector finalization and before `mant_structured_render` returns success; on failure, at the safe native return boundary |
| diagnostic FILE/temp strings | C session; close/free on success and failure after copying accepted diagnostics |
| include decoded bytes/path | resolver call owner; source registry/provenance copies required facts before `mparse_readmem()` returns |
| source registry/maps and per-column sidecars | C builder; copy/move only finalized immutable source/map facts into result arrays; free sidecars with the renderer before success return or at the safe failure boundary; no renderer pointer escapes |
| builder arenas/strings/relations | C builder, transferred atomically to one result handle on success |
| result arrays/strings | opaque C handle; borrowed by stack view descriptors and freed once by `result_free` |
| Rust partial transfer | Rust RAII containers using `try_reserve`; drop before freeing handle on error |
| owned `StructuredDocument` | Rust; independent after native handle free |

C allocator pairs with C free; Rust never frees C storage. Rust callbacks,
including platform resolver callbacks, catch panics and return an error code;
neither language unwinds across the boundary. The public Rust layer exposes
owned values only.

The failure state machine is fixed: guard rejection installs no other TLS and
creates no handle; marshalling failure drops Rust temporaries then the guard;
builder budget/allocation failure marks the collector failed/no-op, completes
native unwinding to a safe return, frees C state, and returns status with no
handle. Finalization/relation failure likewise frees the session and candidate
arrays before returning no handle. A later ABI/view/UTF-8/owned-copy failure
drops Rust partial values, frees the already independent handle, then releases
the guard. Borrowed stack views require no free and
are simply no longer used before handle free. Every controlled failure test is
followed by a successful ordinary call on the same thread.

## 5. Native execution and display

The structured default profile is deterministic UTF-8 at 78 native columns.
Locale and actual viewport width do not silently change it. An explicit caller
width creates a different result under the same profile family; tests compare
that run to an oracle invoked with the same width.

The first collector owns one pending sidecar for every `termp_col`, not one for
only the currently selected field. Sidecar slots align with native buffer slots
and record owner, source/provenance, style, link, role, whitespace/control fact,
and logical token identity. `term_setcol()` grows the sidecar array with
`tcols[]`, switching `p->tcol` selects the matching sidecar, and `adjbuf()`
growth preserves slot alignment. Table columns can therefore retain independent
suffixes across physical lines.

Every accepted buffer write reports one disposition to the sidecar:

- `ignored`: native rejected the write (including a suppressible ordinary
  space), so no annotation is created;
- `append`: create annotations for the new physical slots;
- `replace`: remove the overwritten slot facts before installing new ones;
- cursor-back/move: move the pending write cursor without inventing text;
- truncate: delete truncated annotations and revoke any uncommitted connection;
- clear: commit or discard the selected prefix as directed, then reset exactly
  the native range being cleared.

`term_fill()` snapshots `ASCII_HYPH`, `ASCII_NBRSP`, and zero-width break facts
before their in-place rewrite. When `term_flushln()` advances `tcol->col` over
an automatic-wrap space, the builder commits its one logical whitespace atom
before the cursor passes it. A multi-column return retains the untouched suffix
and its annotations in place; the non-multicolumn completion that zeros
`col/lastcol` clears the matching sidecar. There is no fictional buffer
compaction or tab expansion: `term_field()` reports tab/advance geometry to the
placement builder while the pending slots stay indexed as native stores them.
Overstrike/backspace may map several physical slots to one logical grapheme and
explicit overlay placement; an overwritten or tail-truncated glyph cannot
survive in logical content.

`TERMP_NOBUF`/`directc()`, table-border drawing, and direct `advance()` paths
use a direct-output record carrying current owner, output class, provenance,
and geometry. A border/padding class becomes `Decoration`; authored semantic
output becomes an atom/placement. These hooks observe but do not replace the
native callbacks or their state advancement.

The structural owner stack is pushed before a node's pre-handler and remains
active through pre-handler recursion, normal child traversal, font restoration,
post-handler recursion, and post-triggered flush; it pops only after all those
effects finish. In particular this covers man `IP/TP` heads that manually call
`print_man_node()` in pre and URL post-handlers that generate punctuation and
print children in post. Hooks follow the real pre → child → post → buffered
output path and do not pre-walk the AST or print those children a second time.
They never force a flush to close an IR block and never skip
`ascii_endline()`/`ascii_advance()` state changes. Header/footer output is
classified separately from body content at production time; omitting page
decorations cannot discard a body run flushed by a page callback.

Connection and break facts are explicit:

- a consumed breakable separator remains one logical separator, without output
  indentation or a duplicate replacement space;
- an existing breakable hyphen remains a real hyphen followed by an adjacent
  run; a zero-width breakpoint joins adjacent runs without text;
- non-breaking whitespace/connection remains protected even if a native buffer
  temporarily represents it with a plain byte;
- a hard break is an authored/executed boundary that consumers preserve;
- a fixed line boundary belongs to physical fixed geometry and is not removed;
- vertical space is an independently owned line-count request.

No consumer infers these by scanning final newline characters, applying
`join(" ")`, stripping line-end `-`, or re-executing escape names. No-fill
lines, complex tables, standalone geometry-sensitive equations, and other
geometry-sensitive regions retain fixed physical lines even beyond 78 columns.
Inline equations remain atoms in the surrounding annotated field and do not
force a fixed block merely because `term_eqn()` produced them. Ordinary prose retains logical content,
separator properties, and hard breaks and can reflow in the shared document
renderer. A name, form, style run, or link crossing a native line remains one
logical occurrence with multiple display mappings.

The TUI migration therefore requires a distinct `Fixed/NoWrap` surface. A
fixed physical line never wraps and resize never invokes native rendering.
The document pane owns one horizontal offset applied to its fixed surfaces;
ordinary flow remains at offset zero and reflows normally. Search reveal moves
the offset to show a hidden match. Hit regions are clipped intersections of
placements with the viewport; wide glyph boundaries and combining marks cannot
create half-glyph targets. Copy/selection uses complete fixed coordinates, not
only the visible terminal buffer. CLI machine/text output does not truncate
fixed lines. The consumer migration set is IR visit/validation, references,
entry/explain, search, JSON/schema, Markdown, text/ANSI render, TUI buffer,
hit-testing, selection/copy, and resize/reveal.

## 6. Errors, budgets, and recovery

Budgets are checked before growth or materialization. The structured call must
count at least:

| Budget | Unit |
| --- | --- |
| Input | source count, logical-path bytes, decoded bytes per source and total, include depth |
| Native work | executed/reparsed line limits already supplied by native, tree depth, builder operations, annotation mutations, and diagnostic count |
| Builder memory | allocated bytes, content atoms, owners, blocks/items/cells, connection atoms, annotation runs, source maps, refs, links, points, fixed lines, placements, decorations, and relation edges |
| Owned output | total UTF-8 bytes and each collection count before the C result or Rust copy grows |
| Transfer work | validated objects, edges, path depth, and copied bytes, including source-table mappings |

Source-table count/path bytes and annotation fragmentation are work budgets,
not merely final serialized-size checks. Adjacent equal annotations are merged,
but a pathological alternating input still reaches a deterministic limit.
Budget exhaustion stops further collection, marks the builder failed, returns
no partial document, and cleans up at a safe native return boundary. It does not
blindly `longjmp` from arbitrary upstream frames.

Recoverable and tested failures are: pre-growth budget rejection, injected
allocation failure in the new builder's own fallible allocator, invalid FFI
views/relations, Rust transfer allocation/validation failure where fallible,
and same-thread re-entry. Each must release the handle and guard and allow the
next normal call to succeed.

Upstream `mandoc_malloc/calloc/realloc*` still terminate the process on fatal
system OOM, and existing Rust allocations are not universally fallible. This
work does not claim to recover those failures and does not add product
subprocess isolation. Native execution also lacks a complete instruction-step
budget beyond its existing loop/depth/source limits; collection limits do not
make invisible upstream execution free. Cancellation tokens are not part of
this migration. A caller may discard a result after the synchronous safe
boundary, but cannot claim that the running C call was interrupted.

## C02 admission decision

The six required areas are frozen:

| Gate | Decision |
| --- | --- |
| Content and entry | One atom body referenced by string-free inline leaves, exact dual cross-wrapper forms/name ranges, typed link occurrences, native evidence only in C, canonical recognition and `EntryFacts` in `mant-codec`; worked owners include mdoc roles, empty body, duplicates, and linked terms. |
| Fixed/table | Logical cells/content are authoritative; content/point placements plus decoration carry grapheme/scalar/column maps; span, empty cell, repeated placement, occurrence counting, two copy modes, and consumer routes are explicit. |
| Address/wire | One public atom address model, byte/scalar/column units, runtime flow map versus persisted fixed geometry, SourceFormat, input-slot/result-key separation, source contexts, zero-width points, multi-source provenance, generated/unknown rules, structural versus semantic source validation, and strict in-place v0.12 rewrite are explicit. |
| FFI/lifetime | Versioned v1 minimum render/status/check/view/free/layout-query ABI, current view/field IDs and core discriminants, dense keys, fixed-width fields, checked slices, transfer order, ownership/free table, controlled failure state machine, and re-entry guard are explicit; C02a's checked-in closed semantic-enum header becomes the executable fingerprint before producer code. |
| Execution/display | Per-`termp_col` sidecars, accepted-write/consume/truncate rules, direct output, owner-stack pre/child/post/flush order, exact connection atoms, inline equation behavior, 78-column profile, NoWrap/horizontal behavior, and consumer migration are explicit. |
| Errors/resources | Concrete limit fields/defaults and counter widths, pre-growth rejection, no-partial-result rule, controlled recovery, fatal OOM boundary, incomplete native execution budgeting, and no cancellation promise are explicit. |

These decisions unblock C02a; they do not mark any capability implemented.
Vertical samples in C02-C05 may disprove a relation. Such a change requires a
concrete failing case, an update to this table, and targeted N/F/S review before
another compatibility layer or copied body is introduced.

## Required verification matrix for the first slices

C02a must prove input-slot/result-key separation (including an unused slot before
an include), source table/root validity, callback status/current-source rules,
source-qualified diagnostics, unknown keys, structural per-source out-of-range
rejection, known-fixture wrong-valid-source detection at the producer/oracle
boundary, owned mapping after native free, budget/allocation/transfer cleanup,
re-entry recovery, all ABI sizes/alignments/field offsets/discriminants, and
independent `structured`/`render` feature builds.

The C02a input requires an explicit man or mdoc format. `Auto` remains an old
parser convenience and is rejected at the structured boundary; callers must
carry the already-established source format instead of rescanning roff bytes.
Until a bounded pre-conversion byte map is implemented, source records use
`native-normalized-bytes` coordinates and diagnostics expose only their proven
source-qualified line/column positions, never invented exact byte ranges.

C02b must prove root/include equal line and byte offsets remain distinct,
nested and repeated bundle includes retain source identity without host
fallback, generated/unknown content has no fake range, and buffer append,
overwrite, movement, partial consume, and clear keep annotations aligned. It
must run man/mdoc prose, title, style, link, page-boundary, and
60/78/100/120-column oracle cases. Logical-connection coverage includes the six
cases above, ordinary long words, macro-argument hyphens, consecutive and
non-breaking whitespace, `.br`, `\p`, no-fill, cross-style/link/name boundaries,
inline versus standalone equations, and partial/multicolumn flush. ASCII/UTF-8
device differences, logical search/query/copy ranges, profile display
overrides, and visible continuation style/hit regions are checked independently;
comparison cannot normalize whitespace to obtain a pass.

C03-C05 must carry the worked entry and table examples through the real IR,
semantic index, outline, explain, reference scan, search, Markdown, JSON, and
TUI Buffer. Tests reject copied `Inline::Text/Code` strings and prove that
entry, fixed, search, and copy resolve the same atom store. Fixed tests include
a long no-fill line, narrow/wide resize,
horizontal navigation, right-side reveal, clipped links, wide/combining glyphs,
copy, empty points, spans/rules, and repeated placements. Logical traversal
must count each authored occurrence once.

The current baseline does not cover a native structured result because that
producer does not yet exist. It also does not establish sanitizer or native
Windows/macOS results, fatal-OOM recovery, cancellation, or exact performance
cost for the future annotation sidecar. These remain explicit verification
work, not implicit passes.
