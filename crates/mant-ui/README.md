# mant-ui

`mant-ui` is the Ratatui frontend component used by the `mant` executable. It
renders `ManT`'s in-memory `mant_ir::ResolvedContent` directly and owns
interactive navigation, search, scrolling, links, menus, mouse input, and
terminal presentation. Its host callbacks use `mant-protocol` DTOs for stable
catalog, search, and cross-document interactions without serializing the IR.

## What this crate provides

- A hierarchy-aware, collapsible Outline tree whose complete group labels
  summarize direct entries, descendants, and forms, then expand into commands, parameters,
  configuration keys, variables, values, and generic terms. Compact entry
  names preserve browsing density while an optional full-label mode wraps
  every authored form. Row-topology changes retain the selected node's viewport
  position whenever terminal bounds permit.
- Settled-scroll navigation following and selectable Markdown/mdoc page-local
  references.
- A live, bounded catalog finder that delegates complete-snapshot discovery
  and document loading back to the host.
- Typed Markdown/man reference activation, safe external-URI delegation, and
  bounded back/forward history.
- A visible `↗` reference badge beside linked headings and validated entry forms,
  without a redundant reference subtree. Body links retain collapsed reference
  groups independently of semantic entries. Selecting an occurrence reveals its
  source row; Enter on a reference explicitly opens it while Enter/Space on an
  owner retain folding. `O` opens the reference chooser and Shift+Y copies a
  reference target (choosing first when invoked on an associated owner).
- Confirmed full-document search with active and inactive match highlighting.
- tldr quick-reference and source-document rendering through one layout model.
- Span-aware table columns and cell-local anchors that retain their exact
  rendered rows through independent wrapping, stacking and nested tables.
- Keyboard, mouse, scrollbar, and resizable-pane interaction.
- Width-aware visual text selection plus typed requests for plain-text and
  complete-node Text/Markdown clipboard content.
- Public `App` and `DocumentView` layers for callers embedding the frontend in
  an existing Ratatui host.

Command-line parsing, document loading, terminal acquisition/restoration,
signal handling and static paging remain in the executable host, outside this
crate. The reader performs no process or terminal IO itself.

Navigation plans do not consume history or activate a tab until the candidate
document and destination have been validated. One private navigation ledger
owns bounded back/forward history and first-opened tabs; the application commits
the page and its ledger together. Default positions, authored fragments and
already-scanned reference occurrences are distinct local targets. Occurrence
handles are not public selectors or persistent revision identities: a reloaded
document still requires candidate-view and rendered-location validation.

`ReaderOptions` supplies an `Arc<ResolvedContent>` for the initial page and an
ordered vector of shared scope handles. `App::from_shared` retains those exact
allocations, as do node-copy requests and direct-file history fallbacks. Search
rows belong to their scope snapshot, not merely its address: two revisions of
the same document, or two unregistered direct files, remain distinct. Qualified
history keeps weak provenance instead of retaining every old document body;
local replay requires the same allocation, otherwise the host reloads the
address and the reader validates the destination before committing. Tab grouping
by address is only a display policy, never a revision cache.

The reference overlay owns its chooser and its explicit Open or Copy purpose;
Reveal remains an action on the selected source occurrence. Closing or replacing
the overlay drops that chooser, including during a successful page change.
An outside left click dismisses the chooser without also activating the menu,
tab or document behind it. Keyboard confirmation and footer clicks share the
same action path; no pending host operation runs merely by selecting a row.

Entry title colors reflect source-neutral roles, not importance, confidence or
alias equivalence. Generic terms remain primary text. The body applies type
color only at validated name bindings; identical prose and list markers do not
inherit it. Source bold/italic/code styling and link underlines compose with
that color. Code-token accents do not overwrite semantic name roles.

`DocumentView` prepares a borrowed binding map during construction and stores
the resulting immutable styled lines. Resizing only reflows those lines;
search and selection overlay their own state without rewriting the base styles,
link targets or source coordinates.
Block indentation is a signed displacement from its parent's content origin.
The UI shares origin composition and marker collision rules with the plain-text
frontend through `mant-ir`, and grapheme-safe cell primitives through
`mant-render`; only visible leaves are bounded for padding. Source term roots keep their hard lines and zero-width
targets do not manufacture blank rows.

Width reduction is a view policy, not source layout. When indentation would
leave fewer than 16 content cells (half the width on narrow views), both first
and continuation origins are reduced by one common displacement, bounded at
the left edge. One/two-column views reserve their whole width for content;
a glyph that cannot fit uses a bounded display replacement while preserving
its search identity. This avoids one-character columns at ordinary widths.
Soft wrapping, link hit regions and search highlights share the same cell map.
See the shared [entry presentation contract](https://github.com/BryanHeBY/ManT/blob/main/docs/architecture/entry-presentation.md)
for label modes, source binding coordinates and style precedence.

## Host boundary

```text
mant host
├─ supplies ResolvedContent ───────────────> App / DocumentView
├─ answers CatalogQuery ──────────────────> live finder
├─ resolves a DocumentOpenTarget <──────── cross-document activation
├─ decides whether to open HTTP(S)/mailto < external-link request
└─ renders/stores a typed CopyRequest <──── selection, node or reference target
```

The UI never scans the filesystem, interprets a source path, downloads data,
or opens a URI by itself. It emits typed requests to its host and keeps
page-local jumps in memory. This makes the same component usable by the
`mant` binary and by another Ratatui application with stricter host policy.

## Basic use

The host supplies resolved IR and owns the terminal event loop:

```rust,no_run
use std::sync::Arc;
use mant_ui::{App, ReaderOptions, ReaderServices};

# fn reader(content: Arc<mant_ir::ResolvedContent>) {
let mut app = App::from_shared(ReaderOptions::new(content));
let mut services = ReaderServices::default();
// Route host input through app.handle_event(), draw with app.draw(frame),
// and service queued capabilities after drawing:
app.service_pending(&mut services);
# }
```

`App::new`, `with_catalog` and `with_catalog_and_scope` are borrowing convenience
constructors: they copy the supplied IR into reader-owned handles. Prefer
`from_shared` when the host already owns immutable snapshots. The reader never
merges different allocations because their addresses happen to match.

The host routes events through `handle_event`, invokes `draw` from its Ratatui
frame callback, and supplies the clock to `tick` and `next_wakeup`. These methods
neither sleep nor acquire the terminal. `ReaderServices` independently supplies
discovery, document opening, external-URI opening and clipboard callbacks. Missing
capabilities produce notices without hidden filesystem or process fallbacks;
queued effects are serviced in that fixed order without skipping later effects
when an earlier capability fails. Its copy callback receives
a `CopyRequest`: visual selections already contain plain text, while semantic
node requests carry the complete resolved content, explicit content selector, and
requested format. The callbacks otherwise receive versioned catalog queries,
typed `DocumentOpenTarget` values, and an `ExternalUri` that has already passed
the shared HTTP(S)/mailto activation policy; rejected URI schemes remain
visible document text but never reach the callback. Callback failures return
to the UI as notices rather than giving the frontend hidden authority.
Typed email links use the IR-owned mailto serializer, so URI-sensitive mailbox
characters are encoded before the resulting `ExternalUri` crosses that same
activation boundary.

`DocumentOpenTarget::Address` identifies an exact logical source. Its `Manual`
variant preserves an optional manual section: an unqualified `printf` manual
must be resolved with manual-only policy, without inventing section 1 or falling
back to a Markdown document. A destination fragment is validated against the
actual returned IR before changing the displayed page, tab or history. Missing,
ambiguous or budget-unverifiable targets leave the source view intact.

Reference groups retain distinct source occurrences and full typed targets,
including fragments; repeated destinations are grouped without merging their
labels or source positions. They never promote prose into entries. The shared
IR scan uses a bounded work budget, and the UI additionally retains at most
1,000 records and 1 MiB of reference payload. A visible limited-inventory notice
discloses truncation. Empty labels still have a source reveal location and a
bounded sidebar fallback, without adding text to the body. Resizing reflows
original source-position markers through the same cell/table layout as links.
References lacking a registered Markdown namespace remain inspectable, but do
not imply permission to open arbitrary local files. Reference target copies
emit `CopyRequest::Reference`; complete-node copy is disabled on reference rows.
The copied text is a reusable URI destination from the shared IR codec, not
the display label. Document suffixes and encoded components preserve the typed
target; relative references retain their original document context. Invalid or
unrepresentable targets are refused rather than sanitized into another address.

Associated badges use typed heading locations or completely validated, nonempty
form associations, checked within that same IR work budget. Text resemblance is
not association evidence. Invalid, missing or budget-limited form associations
remain ordinary references; hidden or duplicate owners fall back to a unique
visible section or an unresolved-owner group. A bounded census of requested
owner identities reuses the existing semantic index and exact section metadata, so a
hidden owner cannot lend its badge to an unrelated same-ID node. Repeated native
anchors at one owner count once without rescanning the document's prose. This census spends the remaining shared scan
budget; incomplete verification cannot establish owner uniqueness.
A truncated inventory labels its
badge as known targets, never as proof that the only returned target is unique.
Entry labels keep their semantic color; badges use the reference color and
retain a visible arrow without color. Resizing wraps them with the owner.

`O` opens an explicit reference chooser even for one associated target, allowing
each original occurrence to be inspected and revealed without adding permanent
tree layers. Up/Down selects a source occurrence, Left/Right inspects long target
and source labels, Enter opens (or copies when invoked by Shift+Y), `r` reveals
that occurrence in the current document, and Escape returns. Mouse selection
does not open anything: the chooser's Open/Copy and Reveal footer actions are
explicit. Repeated targets retain separate source choices, including distinct
fragments. Selection, badges and folding do not load destinations; opening
continues through the existing typed host boundary and transactional validation.

The upper-right document tab stack records successful loads in stable first-open
order and deduplicates logical addresses. Selecting an addressed tab emits the
ordinary `open_document` request and commits the active tab only after the host
returns content; the initial direct bundle remains locally restorable. Tabs
retain the last selected semantic node, use terminal-column-aware truncation,
and expose bounded overflow controls. The stack and navigation history are each
capped at 64 entries.

Completing an effective document-text drag emits its plain-text copy request
immediately. A successful callback produces a short-lived, non-modal
confirmation; selection extraction excludes presentation-only tldr panel
borders. Holding the pointer at either vertical viewport edge scrolls and
extends the active selection until release or the document limit. Like a text
editor, Shift-modified clicks and drags retain the original mouse-down anchor
and move the active endpoint, while keyboard and Edit-menu actions can copy the
retained selection again.

Selection copies visual cells, including visual line breaks and continuation
padding; it is not a source-format export. Use document export to retain source
logical lines independently of viewport width. Literal text is never rewritten
in IR by a resize, and its significant spaces remain selectable.

## Platform behavior

The static pager consumes already projected text rather than querying the
engine during resize or search. Search uses visible physical rows (excluding
ANSI control bytes), so regex anchors refer to the current wrapped rows.
No-result searches preserve a valid viewport and permit forward/backward search
without an index entry. Match overlays retain source foreground/font state at
the original glyphs; clearing search restores that base presentation. Native
pager regressions and the CLI's Unix PTY test verify search, resize and colors.
Declaration-group context belongs to the protocol projection, not a second
adjacency inference in the UI; full-document TUI layout ignores that metadata.

The frontend is portable across Linux, macOS, and Windows and does not inspect
the original document source. Callers on every supported platform can provide
normalized man, mdoc, or Markdown queries.

Install [`mant`](https://crates.io/crates/mant) for the complete executable.
This component crate is a library and does not install a second command.
Compatibility and migration notes are recorded in the
[crate changelog](https://github.com/BryanHeBY/ManT/blob/main/CHANGELOG.md).

## License

Apache-2.0.
