<!-- mant:tldr:start -->
# mant

> Turn local manual pages and cross-platform Markdown into structured documents.

- Read Git's complete manual in the interactive reader:

`mant git`

- Extract a tar option entry as portable Markdown:

`mant tar --explain={{--exclude}} --format markdown`

- Search Git and its linked manuals for a topic, limited to two hops:

`mant git --search {{worktree}} --follow-links --max-depth 2 --max-documents 32 --context 1`

- Discover up to 20 native manuals matching a name as compact JSON:

`mant --find {{'^git'}} --regex --kind manual --limit 20 --format json --compact`

- Validate a local Markdown manual's semantic outline and diagnostics:

`mant --input {{./tool.md}} --outline --outline-entries all --format json --compact`

- Serve local documentation to an MCP client over standard input and output:

`mant --mcp`
<!-- mant:tldr:end -->

# mant

## Name

`mant` — read and query structured manual pages and cross-platform Markdown.

## Synopsis

```text
mant <SELECTOR> [OPTIONS]
mant <MAN_SECTION> <NAME> [OPTIONS]
mant --document <SELECTOR>... [--follow-links] [OPTIONS]
mant --input <PATH|-> [--input-format FORMAT] [OPTIONS]
mant --list [FILTERS]
mant --find PATTERN [FILTERS]
mant --request-json [--format FORMAT] [--compact]
mant --doctor [--format text|json] [--compact]
mant --schema CONTRACT [--compact]
mant --update-docs [--compact]
mant --prune-docs [--dry-run] [--compact]
mant --update-tldr [--compact]
mant --protocol-version [--compact]
mant --mcp
```

## Description

`mant` is ManT's native interactive reader, structured command-line interface,
and MCP server. Linux with glibc, macOS, and Windows parse local man and mdoc
sources through bundled libmandoc. Every supported platform exposes hierarchy,
semantic entries, references, and visible text through one normalized document
model.

Local Markdown enters the same model, so terminal navigation, outlines,
excerpts, search, Markdown/text/JSON output, and MCP tools behave consistently
across both sources. A full query opens the interactive reader when stdin and
stdout are terminals; redirection falls back to plain text. `--display tui` and
`--format` make either behavior explicit.

`mant --help` (or `-h`) lists command-line options and points to this manual:
`mant mant` reads it in the TUI on an interactive terminal, or as text otherwise.
Use `mant mant --outline` to explore its structure before selecting content with
`--node` or `--explain`. Running `mant` without arguments reports a missing action
and shows the same manual hints; it does not automatically read the full manual.

## Semantic Document Model

ManT preserves source meaning before presentation. Markdown and native manuals
both become a content tree of sections, blocks, inline nodes, and definition
items. An addressable definition carries a `EntryFacts`; the
rebuildable semantic index groups those facts into commands, parameter
families, configuration keys, variables, values, and generic terms. Each entry
keeps exact selector aliases separate from complete authored forms and points
back to the content definitions that explain it. Nested definitions retain
ownership such as command → option → value.

Outlines are selective projections of that model, not a second parser or a
copy of the document. The default outline summarizes semantic coverage; an
expanded outline exposes entries; `--node` reads the definition content;
`--explain` requires an entry. The TUI and MCP use the same identities and
selection precedence. See [mant-ir(7)](mant-ir.md) for the in-process model and
[mant-protocol(5)](mant-protocol.md) for versioned projections.

Inline links also retain intent. Sections and anchors are current-document
destinations; registered Markdown documents and manual references are the only
cross-document graph edges; external URIs and email addresses are host actions.
Consequently TUI navigation, history, and `--follow-links` operate on the same
bounded logical graph without deriving destinations from rendered labels or
physical paths.

## Document Discovery

<!-- mant:entries role=option case=sensitive -->
- `--list`: List documents grouped by configured source or native manual section.
- `--find PATTERN`: Filter document names and emit one stable record per match. Matching defaults to a case-insensitive literal substring; combine `--regex` and `--case` to change matching, and `--limit` and `--offset` for pagination.
- `--kind KIND`: Restrict discovery to `markdown` or `manual`. It applies to `--list` and `--find`.

Use `--source SOURCE` to restrict Markdown discovery to one configured source,
or `--man-section MAN_SECTION` for one native manual category.
`--display direct` prints discovery text without the interactive pager.

Discovery uses a case-insensitive literal substring by default. `--find` also
accepts `--regex` and `--case`; `--limit` and `--offset` apply deterministic
pagination. `--list` and `--find` return at most 10,000 rows by default. Inside
one relevance tier, candidates preserving the query's exact case rank ahead of
candidates found only through case folding. Plain `--find` output is
tab-separated as the canonical catalog path and `kind`, while `--format json`
returns `mant.catalog/v0.11`. `--list` groups the same hierarchy beneath
`documents`, `sources/SOURCE`, or `manual/SECTION`.

An empty name match stays silent in ordinary text output. If an explicit source
or manual section is not indexed at all, text output instead identifies the
missing scope and lists the available namespaces. JSON callers can make the
same distinction with `coverage.scopeTotal`: it is counted before applying the
name pattern. Extended manual categories such as `2const` and `3pm` remain
independent exact sections rather than being folded into `2` or `3`.

When stdin and stdout are terminals, discovery text longer than the terminal
height opens in the built-in pager. It supports mouse scrolling, ordinary
less-style movement, and `/` search. Short results print directly. Pipelines,
redirection, `TERM=dumb`, `--format json`, and `--display direct` never enter the
pager.

```sh
mant --list
mant --find process --source pwsh7
mant --find '^git' --regex --kind manual
mant --list --man-section 3 --format json
```

## Input {#input-section}

<!-- mant:entries role=option case=sensitive -->
- `--input PATH`: Read one explicit Markdown or roff file, or use `-` for standard input. Physical filesystem paths are never inferred from positional document selectors. Stdin requires an explicit `--input-format markdown` or `--input-format roff`. Direct file input does not follow redirect-only `.so` pages; those require an indexed MANPATH root.
- `--input-format FORMAT`: Select `auto`, `markdown`, or `roff` for `--input`. Files default to `auto`, which selects the parser from the filename suffix; stdin requires `markdown` or `roff` explicitly. The roff loader accepts plain, gzip, and zstd sources and detects compression safely.

Input is resolved before parsing. An ordinary selector first checks the user's
hierarchical `documents` tree. Configured installed sources then compete with
the native manual index, whose priority is `0`: positive source priorities win,
the native manual wins a zero tie, and non-positive sources provide fallback.
Sources within either side are ordered by descending priority and ascending
bytewise name. The configured-source default is `1`. Linux uses
`${XDG_DATA_HOME:-$HOME/.local/share}/mant`, macOS uses
`~/Library/Application Support/ManT`, and Windows uses `%APPDATA%\ManT` as its
data root. Physical filesystem paths are never inferred from positional
selectors; use `--input PATH` explicitly.

Registered `.md` and `.markdown` files retain their extension-free relative
paths. Exact paths win before unique component suffixes; ambiguous suffixes are
reported with their candidates. Complete selectors use
`documents/PATH`, `sources/SOURCE/PATH`, or `manual/SECTION/NAME`. Root
documents always win; source priority, the native-manual zero baseline, and
source name resolve remaining duplicates in the order above; `.md` wins over
`.markdown` for one logical path.
`--source NAME` selects exactly one configured Git or archive source.
`--manual` or `--man-section` selects the full document from native manuals and
cannot be combined with `--source`. `--manual` also excludes the independent
quick-reference channel; `--man-section` alone does not. A manual category is
distinct from a heading or other node inside the loaded document, which is
selected with `--node`.

Windows document packages should retain executable suffixes in canonical
filenames, such as `cargo.exe.md`. An extensionless query such as `mant cargo`
first tries the exact name, then appends extensions from `PATHEXT` in order, so
it can resolve `cargo.exe`; an explicit `mant cargo.exe` remains exact. This is
a Windows platform rule independent of the calling shell and PowerShell
version. Script suffixes such as `.ps1` participate only when present in
`PATHEXT`; an unset or empty value uses `.COM`, `.EXE`, `.BAT`, and `.CMD`.
Other platforms never elide these suffixes.

### Manual Pages

On Linux with glibc, macOS, and Windows, manual page names are located through
ManT's native manual index. ManT reads raw, gzip, and zstd roff sources and
resolves redirect-only `.so` alias chains within the indexed manual root. All
file and decompression I/O for manual sources remains in ManT; bundled
libmandoc receives only the final plain roff bytes. Neither a system `man` nor
a system `mandoc` executable is required for ordinary use.

An unqualified name with pages in several native categories follows `MANSECT`
when that variable contains a colon-separated order. Without it, ManT uses the
stable man-db order `1:1p:n:l:8:3:3p:0:0p:2:3type:5:4:9:6:7`; categories absent
from that list follow in lexical order. An explicit `--man-section`, leading
section argument, or `manual/SECTION/NAME` catalog path always selects exactly
that category.

The accepted section spellings are deliberately unambiguous:

```sh
mant 8 btrfs
mant 'btrfs(8)'
mant btrfs --man-section 8
mant manual/8/btrfs
```

A dotted selector such as `btrfs.8` is an exact logical document name, not a
manual shorthand. This preserves Markdown names, dotted executable names, and
tldr collision pages such as `command.1` without a context-dependent guess.
Use `--input PATH` for a physical roff file.

Windows automatically checks `%APPDATA%\ManT\man`, then the compatible
`%USERPROFILE%\.local\share\man` root. It also accepts additional roots through
`MANPATH` or `MANT_MANPATH`, and an optional ManT-owned
`%APPDATA%\ManT\man.conf` can provide persistent roots without requiring a
shell profile.

<!-- mant:entries role=option case=sensitive -->
- `--man-section MAN_SECTION`: Select the full document from one exact native
  manual category such as `1` or `3p`. In an ordinary combined query, a selected
  section `1` or `8` family page may still receive its command quick reference.
- `--manual`: Require only a native manual instead of registered Markdown with
  the same name or an attached quick reference.
- `--tldr`: Print only the highest-precedence embedded or cached quick
  reference and permit it when no full document exists. Personal documents
  precede positive-priority sources, the cached tldr baseline at priority `0`,
  and then non-positive sources. Markdown without an embedded quick reference
  does not participate. Combine `--source SOURCE` to select only that source.
  The default is styled terminal text on a color TTY and plain text through a pipe; use
  `--color always|never` or an explicit `--format` to override it. A cached
  tldr entry alone does not satisfy an ordinary document query: ManT reports
  the missing document and suggests this explicit command.
  When combined with `--tldr`, a section qualifier written as `1 NAME`,
  `8 NAME`, `NAME(1)`, `NAME(8)`, or `--man-section` is accepted only for
  command-section families `1` and `8`; it validates the kind of page but is
  not joined into the tldr topic.
- `--source SOURCE`: Require one configured installed Markdown source.

Recoverable parser findings remain structured in JSON output. ManT does not
invoke a host renderer or maintain an alternate HTML parsing path.

For an ordinary combined native-manual query, ManT automatically attaches a
cached tldr page only when the selected manual belongs to section family `1` or
`8`. This rule is identical for an unqualified lookup and an explicitly selected
manual category. Those categories represent user and administration commands;
other native categories do not inherit a same-named command quick reference.
Explicit `--tldr` accepts only those command-family qualifiers.

### Local Roff Trees

Use `MANT_MANPATH` or `MANPATH` to make project-local man or mdoc sources
visible without copying them into a system directory. Each entry names a root;
pages can live directly below it as `widget.1`, or in section directories such
as `man1/widget.1`. The entry must not name an individual source file.

For example, expose `widget.1` as document name `widget`:

```sh
mkdir -p ./project-man/man1
cp ./widget.1 ./project-man/man1/widget.1
MANT_MANPATH="$PWD/project-man" mant widget --man-section 1
```

The equivalent PowerShell setup is:

```powershell
New-Item .\project-man\man1 -ItemType Directory -Force | Out-Null
Copy-Item .\widget.1 .\project-man\man1\widget.1
$env:MANT_MANPATH = (Resolve-Path .\project-man).Path
mant widget --man-section 1
```

`MANT_MANPATH` is a complete ManT-specific override. `MANPATH` likewise
replaces the derived roots, except that an empty component inserts the complete
host-derived default sequence at that point. Unix lists use colons; Windows
lists use semicolons. This preserves the established leading, trailing, and
double-delimiter behaviour without invoking `man`, `manpath`, or any external
program.

#### Host Manual Paths

When neither override is set, ManT reads the host's manual-path configuration
before adding its supplemental user roots. On Linux, it first reads a user
`~/.manpath` when present; otherwise it recognizes the common man-db locations
`/etc/man_db.conf`, `/etc/manpath.config`, and `/usr/local/etc/man_db.conf`.
It applies every `MANPATH_MAP`, then `MANDATORY_MANPATH`, and honors the
man-db `SYSTEM` expansion. If no man-db configuration is available, it follows
mandoc-style `/etc/man.conf` `manpath` entries. This covers the standard
man-db families (including Debian/Ubuntu, Fedora/RHEL, and Arch) as well as
mandoc-based Linux installations such as Alpine.

On macOS, ManT follows the native order: the first existing manual directory
derived from each `$PATH` component, the active developer tree, the system
defaults, then `/etc/man.conf` `MANPATH` entries and its `MANCONFIG` fragments
(including the default `/usr/local/etc/man.d/*.conf` extension directory).
The active developer tree follows `DEVELOPER_DIR`, xcode-select's persisted
selection, and Apple's standard Xcode or Command Line Tools fallbacks; it
includes both tool and SDK manual directories. ManT reads that state directly
instead of invoking `xcode-select`. Only when no native root is available does
it fall back to `/etc/manpaths` and sorted `/etc/manpaths.d` files.

Manual-path configuration readers accept only regular UTF-8 files within the
read budget, including symlinks to regular files. Directories and special files
such as FIFOs are skipped. Unix opens are nonblocking and recheck the opened
file so a FIFO cannot make discovery wait for a writer, including when a path
changes after its initial check. This does not impose a deadline on regular
filesystem or network-mounted storage operations.

BSD/mandoc configuration expansion shares a 4096-step work budget across
directives, patterns and directory entries, including nonmatches. Incomplete
patterns are discarded, not interpreted as partial roots. macOS additionally
reads at most 256 one-level fragments and 8 MiB across the configuration tree
(1 MiB per file). `mant --doctor` reports exhausted expansion and fragment
budgets, including when no path matched; normal queries retain completed roots
and skip later work. Complete environment overrides do not inspect inactive
configuration or report its findings.

Windows has no system `man(1)` convention. If present,
`%APPDATA%\ManT\man.conf` is a ManT-owned portable configuration. Its
case-insensitive path directives are:

- `MANPATH DIRECTORY` (or `manpath`) adds an unconditional root;
- `MANCONFIG PATTERN` imports sorted matching configuration fragments;
- `MANPATH_MAP EXECUTABLE-DIRECTORY MANUAL-DIRECTORY` adds the manual root
  when the executable directory occurs in the current `PATH`; and
- `MANDATORY_MANPATH DIRECTORY` adds an unconditional root after mapped
  roots.

Direct roots from the primary file come first, followed by direct roots from
at most 256 unique one-level `MANCONFIG` fragments, mapped roots in current
`PATH` order, mandatory roots, `%APPDATA%\ManT\man`, and finally
`%USERPROFILE%\.local\share\man`. A fragment cannot recursively import more
fragments. Expansion shares a 4096-step work budget across patterns, path
components, and enumerated directory entries, including nonmatches. Wildcard
matching additionally shares a budget of 4 Mi comparison cells; `?` matches one
Unicode scalar. These work limits are independent of the 256 unique fragments
that may be loaded. An incomplete pattern contributes no paths, and later
patterns are not traversed after exhaustion. `mant --doctor` reports truncated
discovery. Each configuration file is bounded to 1 MiB of actually read bytes;
the Windows configuration tree is bounded to 8 MiB and 4096 input lines.
Expanded paths are limited to 4096 encoded bytes. Only `MANCONFIG` expands
wildcards in this Windows configuration;
all root and map directives describe literal directories.
`MANDB_MAP`, `DEFINE`, `SECTION`, and
formatter or pager directives do not describe source roots and are ignored.

A single-path directive consumes the whole remainder of its line, so an
unquoted path may contain spaces. Double quotes may optionally delimit a path;
`MANPATH_MAP` paths containing spaces must be quoted separately. Backslashes
are literal path separators, single quotes have no special meaning, and only a
line whose first non-space character is `#` is a comment. Inline comments are
therefore not supported.

Within this Windows-only file, `%NAME%` expands any defined process environment
variable using a case-insensitive name match. Expansion is deliberately one
pass: text supplied by an environment value is not rescanned. Write `%%` for a
literal percent sign. ManT does not expand `~`. An undefined variable,
malformed quote or expansion, wrong argument count, or non-absolute Windows
path omits that directive without breaking document lookup; `mant --doctor`
reports the configuration file and line as `manuals.configuration`.

For example:

```text
# Direct roots can contain spaces without quoting.
manpath C:\Program Files\Git\usr\share\man
MANCONFIG "%APPDATA%\ManT\man.d\*.conf"
MANPATH_MAP "%USERPROFILE%\scoop\shims" "%SCOOP%\apps\cmake\current\man"
MANDATORY_MANPATH "%PROGRAMDATA%\ManT\man"
```

These percent expansions do not apply to `MANT_MANPATH`, `MANPATH`, Unix
configuration files, registered Markdown sources, or roff include paths.
Windows process-environment names themselves are matched without ASCII case
sensitivity, so preserved spellings such as `Path`, `AppData`, `UserProfile`,
`ManPath`, and `Mant_ManPath` have their normal Windows meaning. Unix
environment lookup remains case-sensitive.

The index accepts a leaf page symlink whose target is a regular file, including
one outside the configured root. It does not traverse directory symlinks or
index broken links. If that leaf is a redirect-only `.so` page, its target is
resolved from the symlink's logical location and must remain inside the
configured root; the same boundary applies to every later redirect.

Logical queries accept `mant widget --man-section 1`, `mant 1 widget`,
`mant 'widget(1)'`, and the canonical `mant manual/1/widget`. For isolated
files, `mant --input ./widget.1` is also valid; only MANPATH queries may resolve
redirect-only `.so` aliases.

## Markdown Documents

Local files and standard input enter the same document, outline, excerpt,
search, and output pipeline as manual pages. The supported subset is
deliberately structural rather than a complete browser-oriented Markdown
implementation.

Registered personal documents may be leaf-file symlinks whose targets are
regular files, including targets outside `documents/`. The link's `.md` or
`.markdown` name defines its logical identity. Directory and broken links are
ignored. Installed source caches never follow links; source updates require
selected Markdown entries to be regular files.

### Supported Structure

- When the first heading is H1, it supplies the document title and does not
  consume an outline number.
- All remaining H1 through H6 headings form the recursive, one-based section
  tree; H2 is the conventional top level after a document title. Repeated
  headings receive unique document-local IDs.
- Prose before the first heading, and prose belonging to the document title,
  becomes the selectable `root` overview when it is non-empty.
- Paragraphs, thematic breaks, fenced and indented code blocks, ordered and
  unordered lists, nested lists, and GFM tables are structured nodes.
- A fenced code block retains the first language token from its info string.
  Table column alignment and ordered-list start values are also retained.

### Supported Inline Syntax

Plain text, strong and emphasized text, inline code, explicit hard line
breaks, standard links, email links, document-local fragments, and hierarchical
relative `.md` or `.markdown` links remain semantic inline nodes. Relative
document links resolve lexically inside the current registered source;
`..` is accepted only while it remains within that source. Soft source line
breaks become spaces, while hard breaks remain visible.

### ManT Extensions

A complete bullet list becomes semantic Unix command-line options when every
item begins with one or more code spans containing dash-option names, followed
by `:`, `—`, or `–` and a description:

```markdown
- `-h`, `--help`: Show help.
- `--color WHEN`: Select a colour mode.
```

This legacy shorthand is case-sensitive. Aliases may be separated by commas,
the historical `-h/--help` notation, or vertical bars.

Role-aware entries, including Windows switches, commands, variables, and
environment variables, use an explicit invisible directive before one
complete bullet list. Both the role and matching policy are required:

```markdown
<!-- mant:entries role=option case=insensitive -->
- `/query`: Query scheduled tasks.
- `/?`: Display help.
- `/S COMPUTER`: Select a remote computer.
- `/server:NAME`: Select a server.
- `/reg:32`, `/reg:64`: Select registry views.
- `type= TYPE`: Select a service type.
- `//B`: Select Windows Script Host batch mode.
- `+r`: Set an attribute.
- `/+N`: Select a character offset.
- `/driver.exclude`: Select Driver Verifier exclusions.
- `-ca.cert`, `-ca.chain`: Preserve exact dotted dash-option names.
- `--config.file=FILE`: Omit a placeholder while preserving the dotted name.
- `!--reloadEnvironment`: Preserve an explicitly negated dash option.

<!-- mant:entries role=option case=insensitive attached=fixed -->
- `/F`: Run an extended scan.
- `/F:Y`: Run an extended scan and clean detected malware.
- `/server:<NAME>`: Select a server while keeping an explicit placeholder.
- `perf=default`: Select a fixed named policy.

<!-- mant:entries role=command case=insensitive -->
- `query`: Read keys and values.
- `winget install`: Install a package.

<!-- mant:entries role=environment-variable case=insensitive -->
- `PATH`, `$env:PATH`: Control executable discovery.
- `${Env:ProgramData}`: Locate shared application data.
- `%ProgramFiles(x86)%`: Locate 32-bit programs.
- `RUST_LOG=debug`: Select a diagnostic filter.

<!-- mant:entries role=variable case=insensitive -->
- `$?`: Hold the last PowerShell success state.
- `$LASTEXITCODE`: Hold the last native process exit code.
- `$PSVersionTable`: Describe the running PowerShell version.

<!-- mant:entries role=configuration-key case=insensitive -->
- `AuthorizedKeysFile`: Select authorized-key paths.

<!-- mant:entries role=marker case=sensitive -->
- `--`: End option parsing.

<!-- mant:entries role=operand case=sensitive -->
- `FILE`: Name an input file.

<!-- mant:entries role=value case=insensitive -->
- `always`: Select an accepted value.

<!-- mant:entries role=term case=sensitive -->
- `exit status`: Describe a general addressable term.
```

`role` is `option`, `marker`, `operand`, `command`, `configuration-key`,
`environment-variable`, `variable`, `value`, or `term`; `case` is `sensitive`
or `insensitive`. An environment-variable entry accepts bare
`NAME`, shell `$NAME`, PowerShell `$Env:NAME` or `${Env:NAME}`, Windows
`%NAME%`, and assignment `NAME=value` spellings. The selector for an assignment
omits its value while the authored form retains it; wrapper-free lookup such as
`PATH` is also available for wrapped spellings. Names start with an ASCII
letter or underscore and then use ASCII letters, digits, `_`, `-`, or
parentheses. This grammar is shared with native-manual inference, but is
applied only inside an environment-variable list or environment section, never
to arbitrary prose. A general variable begins with `$` and may use an
ASCII identifier or one of the special names `$?`, `$$`, and `$^`; `$_` is an
ordinary identifier form. `$env:PATH` remains an environment variable rather
than a general variable. Option declarations may additionally use
`attached=fixed`. It retains unbracketed values attached with `:` or `=` as
part of the semantic name, so `/F:Y` and `perf=default` remain distinct from
`/F` and `perf=`. An angle-bracketed value such as `<NAME>` remains an explicit
placeholder. Omitting the field preserves the legacy inference in which an
uppercase attached value is a placeholder. `attached=infer` states that
default explicitly. The directive must be the only construct on its line and
targets a bullet list beginning on the next non-empty line. Blank lines are
allowed, but a heading, paragraph, or other intervening construct invalidates
the declaration.

Every item in a declared list must begin with one or more inline-code terms,
optionally separated by commas or `|`, and then contain `:`, `—`, or `–` in the
same leading paragraph. The delimiter is required even when further
description blocks follow. For example:

```markdown
<!-- mant:entries role=option case=insensitive -->
- `/query`, `/Q`: Query the current state.
```

A relative document link wrapping exactly one code term is also accepted and becomes an explicit entry destination, while links in the description remain ordinary content:

```markdown
<!-- mant:entries role=command case=insensitive -->
- [`winget.exe`](winget.exe.md): Open the command manual.
```

Semantic lists may nest to the normal Markdown depth budget; each nested list declares its own role. An entry can also declare a cross-document value space inside its list item:

```markdown
- `-o OPTION`: Set an SSH configuration key.

  <!-- mant:domain entries=manual/5/ssh_config roles=configuration-key -->
```

The domain target is a relative Markdown document or exact `manual/<section>/<name>` path, and `roles` is a comma-separated semantic-role list. Fragments and inferred prose relationships are rejected.

This is intentionally not a semantic entry because the leading paragraph has
no description delimiter:

```markdown
<!-- mant:entries role=option case=insensitive -->
- `/query`, `/Q`
  Query the current state.
```

Value placeholders remain visible while normalized selectors omit them.
Whitespace placeholders such as `/S COMPUTER` become `/S`. A colon suffix in
uppercase placeholder form, optionally enclosed in angle brackets, is also
omitted: `/server:NAME` becomes `/server`, as does `/server:<NAME>`.
Lowercase colon suffixes are fixed values, so `/server:name` remains the full
selector. Numeric and lowercase alphabetic values such as `/reg:32` and
`/mode:auto` likewise remain part of the selector.

Declared option lists also accept conservative Windows-native token families.
An explicit `!` may negate an otherwise valid dash option, so
`!--reloadEnvironment` remains an exact executable spelling; `!name` without a
dash option is rejected.
An ASCII identifier followed by `=` retains the equals sign while omitting an
uppercase or angle-bracket placeholder, so `type= TYPE` and `board=N` become
`type=` and `board=`. Windows Script Host `//` names preserve both slashes and
apply the same colon-placeholder rule. Safe leading-plus, slash-plus, and
dotted-slash tokens such as `+r`, `/+N`, and `/driver.exclude` remain complete
selectors. Dash options likewise preserve non-empty dotted segments, so
`-ca.cert` and `--config.file=FILE` become `-ca.cert` and `--config.file`.
Arbitrary trailing punctuation, paths, empty dotted segments, and lowercase
equals-value placeholders are rejected instead of being silently truncated.

Case policy belongs only to the declared list. It does not change section
paths or IDs. Use `sensitive` when `-p` and `-P` differ, and `insensitive` for
Windows `/S` versus `/s`, PowerShell command and parameter names, or
environment spellings such as `PATH`, `$env:PATH`, and `$ENV:PATH`. Outlines
always retain the canonical spelling written by the author.

Unknown fields, missing policies, and malformed declared lists produce a
source-located recoverable diagnostic without dropping or reordering content.

Recognized entries receive role-specific stable IDs and aliases, appear
beneath their owning section in `--outline`, and are selectable through
`--node` and `--explain`. A mixed ordinary/option list remains an ordinary list
rather than being guessed. Paths and IDs resolve first, followed by exact
aliases and then normalized conveniences such as omitting leading dashes. This
lets an exact command `?` coexist with option spelling `-?`. When aliases at
the same precedence remain ambiguous, or a structural ID shadows an entry
alias, the outline carries a source diagnostic and selection returns or points
to candidate paths and IDs for exact qualification. `semanticsComplete: false`
is reserved for rejected declarations or native definitions that could not be
classified without guessing.

Semantic tables are not currently inferred or declared. Convert an interface
table to a declared bullet list when its rows must appear in outlines and work
with `--explain`; ordinary tables remain ordinary document content.

An optional leading tldr preface is parsed independently with the tldr-pages
dialect. Invisible CommonMark HTML comments delimit it, so GitHub renders the
contents normally without exposing extension syntax:

```markdown
<!-- mant:tldr:start -->
# tool

> One-line command description.
> More information: https://example.test/tool.

- Run the command for a file:

`tool {{path/to/file}}`
<!-- mant:tldr:end -->

# Tool
```

The opening line must contain only `<!-- mant:tldr:start -->` and be the first
non-empty construct. The closing line must contain only
`<!-- mant:tldr:end -->`. The embedded page uses a command H1, quoted
description lines, optional `More information:`, example descriptions, code
commands, and standard `{{placeholder}}` or `{{[-s|--long]}}` placeholders.
Adjacent source lines in one quoted or example-description paragraph are joined
according to CommonMark; source formatting therefore does not force terminal
line breaks.
It receives path `0` and alias `tldr`, uses the same renderer and search model
as cached tldr-pages data, and records document-owned provenance without
claiming tldr-pages licence attribution. Ordinary headings named `TLDR` have
no special meaning. Community tldr-pages content carries its `CC BY 4.0`
attribution in Markdown, plain-text, and interactive terminal presentations;
embedded document-owned quick references do not claim that attribution.

### Preserved Unsupported Syntax

Block quotes, task lists, images, raw HTML, strikethrough, footnotes, native
Markdown definition lists, metadata blocks, math, superscript, subscript, and
wiki links are outside the semantic subset. The two exact leading tldr
boundary comments are consumed before ordinary Markdown lowering; all other
raw HTML follows the unsupported-syntax rule. Its source remains visible as an
unsupported node or inline fragment, and JSON results include a source
diagnostic. ManT does not silently discard it or pretend to render browser
features it does not implement.

Source spans keep original Markdown line and column positions. Extracting an
embedded tldr preface does not shift those coordinates.

## Interactive Reader

With a complete logical selector or `--input` file and a terminal on stdin and stdout,
`mant` opens its Ratatui reader. `--display tui` requires this mode explicitly. A
projection option or `--format` selects deterministic output instead.

The resizable Outline sidebar forms one tree of addressable nodes: document
sections, semantic entry groups, nested parameters and values, and optional
tldr content. A collapsed entry group shows its direct-entry count in compact
mode; selecting it reveals the nested-entry and authored-form totals.
Expanding it reveals the same hierarchy returned by an `all` outline
projection. Semantic entries use their exact aliases as
compact labels, while the selected entry expands to its complete authored
form. **View → Full Outline Labels** wraps every visible complete label for
side-by-side review. This mode changes presentation only; entry identity,
folding, selection, and document targets remain unchanged. Full-label changes,
whole-tree expansion or collapse, and Outline-width changes keep the selected
node on the same viewport row whenever terminal bounds permit. Selecting a
node puts its target at the top of the content pane.
After content scrolling settles, the outline follows the first visible
document node.
Underlined references can be followed directly. Markdown fragments and mdoc
`Sx` references jump inside the current page. Relative Markdown links preserve
their hierarchy but resolve only inside the current registered source. mdoc
`Xr`, GNU man `MR`, recognized strong `name(section)` references, and validated
legacy Sphinx empty-destination references resolve to an exact manual section
wherever they occur in a page. Bare `name(section)` prose is not inferred as a
link. HTTP(S) and email links open through the platform's
default handler; local-file and executable URL schemes remain visible but
inert. Successful document and in-page jumps are recorded in a bounded
backward/forward history; missing targets and failed loads leave the current
document and history unchanged.

### Navigation

- `j`, `Down`: Select the next visible node.
- `k`, `Up`: Select the previous visible node.
- `h`, `Left`: Collapse the current branch or select its parent.
- `l`, `Right`: Expand the current branch or select its first child.
- `d`, `PageDown`: Scroll the content down.
- `u`, `PageUp`: Scroll the content up.
- `Ctrl+O`: Open the document finder.
- `Alt+Left`: Return to the previous document or in-page jump.
- `Alt+Right`: Move forward after returning.

The upper-right tab strip keeps up to 64 successfully opened documents in
stable first-open order. Reopening a document activates its existing tab rather
than adding a duplicate. Clicking a tab restores its last selected Outline node
through the same transactional host-open boundary as other document links, so
a failed load leaves the current document, active tab, and history unchanged.
Long terminal-aware labels are middle-truncated; when the strip overflows, the
`‹` and `›` controls reveal adjacent tabs without changing the current document.

Mouse input selects and folds outline nodes, follows in-page,
cross-document, and safe external links, scrolls either pane, drags
scrollbars, resizes the Outline boundary, and selects rendered document text.
Safe external links are classified before they can cross the host boundary:
only structurally valid HTTP and HTTPS targets with RFC 3986 component and
percent-escape syntax, a host, and valid optional port, plus mailto targets
whose recipients decode exactly once to ASCII dot-atom mailboxes, are
activatable. Typed email targets use the same validator and percent-encoding
serializer. Rejected targets remain visible and inert. They are handed
asynchronously to the platform URL handler; native Windows uses the absolute
System32 handler path, WSL uses the Windows handler when it is available, and
the child process cannot read from or write into the TUI terminal streams. A
success notice confirms that handoff started, not that a browser accepted it.
A completed drag selection is copied immediately as plain text and shows a
short success popup. `y`, `Ctrl+Shift+C`, or **Edit → Copy Selection** copies
the current selection again. Right-clicking inside the document does the same
when the terminal delivers that mouse event to ManT; `Shift+click` or
`Shift+drag` retains the original mouse-down anchor and moves the active
endpoint, matching a text editor's directional selection model. `Escape`
clears it. Holding a drag on the first or last content row continuously scrolls
and extends the selection. Selection follows terminal cells, omits
presentation-only tldr panel borders, and never attempts to reconstruct partial
Markdown.

The Edit menu can also copy the complete current Outline node as deterministic
text or structurally complete CommonMark. These node actions operate on the
semantic document subtree rather than the visible wrapped rows, so Markdown
syntax is never cut at an arbitrary visual boundary. Synthetic Outline groups
cannot be copied; select a complete section, entry, document overview, or tldr
node instead.

Clipboard delivery follows the terminal topology. Local sessions use the
native system clipboard first and fall back to the write-only OSC 52 terminal
protocol when native access is unavailable. WSL, SSH, and VS Code remote
sessions prefer OSC 52 so a compatible outer terminal or multiplexer can place
the text on the user's clipboard. OSC 52 payloads are limited to 400 KiB before
Base64 encoding so common terminal parsers do not silently discard an oversized
control string. Terminals can disable OSC 52 writes; in that case ManT can
confirm that it emitted the request but cannot observe whether the outer
terminal accepted it.

### Page Search

- `Ctrl+F`, `/`: Open the bottom search field.
- `Enter`: Confirm a query or select the next match.
- `n`: Select the next confirmed match.
- `Shift+N`: Select the previous confirmed match.
- `Escape`: Close search and remove match highlighting.

Search runs only after confirmation. Matches stay highlighted while the field
is open, and the active match uses a stronger background and moves into view.
Closing the field removes highlighting but retains the confirmed query, so
`n`, `Shift+N`, and the Search menu can resume navigation without rerunning it.

### Document Finder

The `Ctrl+O` window searches the complete local catalog while text is entered
and keeps the displayed result page bounded. Each match shows its configured
Markdown source or exact native manual section. Exact names precede prefixes,
which precede other substring matches. `Up` and `Down` select a result, `Enter`
opens it, and `Escape` closes the window. The Manual menu exposes the finder;
Navigate contains backward/forward history and current-document movement.

### Interface

- `F10`: Open the menu bar.
- `?`: Show keyboard shortcuts.
- `q`: Quit.

The View menu can hide the Outline sidebar, reset its width, switch between
compact and fully wrapped labels, and expand or collapse the complete tree.
The Navigate and Search menus expose the same operations as their shortcuts.
The Edit menu exposes visual plain-text copy and complete-node Text/Markdown
copy. Terminal setup is restored on normal exit, errors, and Rust panics.

## Document Selection

<!-- mant:entries role=option case=sensitive -->
- `--document SELECTOR`: Supply an ordered set of initial registered documents by repeating this option. It is the multi-document form of the ordinary positional selector.

  ```sh
  mant --document git --document git-lfs --search worktree --follow-links --max-depth 2 --max-documents 32
  mant --document git --document git-config --explain core.worktree --follow-links --max-depth 1 --max-documents 16
  ```

- `--follow-links`: Expand either one positional selector or the repeated `--document` set through typed manual references and same-source Markdown document links. Expansion is breadth-first in initial-document and source-link order. Exact logical addresses deduplicate cycles and diamonds. Ordinary prose resembling `name(section)`, filename prefixes such as `git-*`, page-local links, and external links never create graph edges. Bound expansion with `--max-depth` and `--max-documents`.
- `--max-depth DEPTH`: Limit followed link edges from an initial document; defaults to 8. Zero loads only initial documents, while one also loads their one-hop neighbours. Requires `--follow-links`.
- `--max-documents COUNT`: Limit distinct loaded documents, including initial documents; defaults to 64 and cannot exceed 256. Requires `--follow-links`. Links excluded by this bound remain visible in `frontier`.

Scope resolution also retains at most 64 MiB of normalized semantic content. A linked page that would exceed that aggregate budget remains visible in `frontier` with a `max-content-bytes` reason; an initial document excluded by the same budget appears in `unresolved` and the request fails only if no initial document remains readable. JSON results distinguish missing initial documents and links through `unresolved.from`, and retain every logical link excluded by a depth, document, or content bound. Search applies one global `--limit` and `--offset` over breadth-first document order. Explain applies them after global evidence classification: direct entries, explicitly related entries, other-entry mentions, ordinary mentions; within each class it preserves document BFS and original IR order. Explain preserves each independent owner, even when several owners in one document or different documents share a name. Ordinary literal content is supporting evidence, not a failure or a new definition. Readable sources with no evidence remain represented in coverage; all sources failing is a source error, not a no-evidence result.

Multi-document deterministic output supports `--search` and `--explain`. Outline, node, tldr, full Markdown, and man-format output remain single-document operations instead of silently selecting or concatenating pages. `--display tui` opens the first initial document; confirmed text search spans the resolved set, cross-document results participate in history, and the ordinary document finder remains global.

<!-- mant:entries role=option case=sensitive -->
- `--outline`: Print section topology plus a compact semantic-entry summary for
  each non-empty scope.
- `--outline-entries MODE`: Select `none`, `summary`, `all`, or a
  comma-separated list of `command`, `option`, `marker`, `operand`,
  `configuration-key`, `environment-variable`, `variable`, `value`, and
  `term`. A kind filter retains only matching entries and their structural
  ancestors. With no matches it reports an explicit zero result instead of an
  empty-looking copy of the complete section tree.
- `--outline-root SELECTOR`: Start the projection at one exact section or entry
  path, stable ID, or unambiguous semantic alias.
- `--node SELECTOR`: Return an outline node selected by path, stable ID, or
  semantic-entry alias; repeat the option to select several nodes.
- `--explain ENTRY`: Collect independent evidence for a documented name, complete form, exact entry ID/path, or bounded literal. Multiple owners and no evidence are normal results (exit 0). Names/forms follow the owner's case policy; literal supporting content is case-sensitive and is not promoted to a definition. Results show four category counts and retain separate owners. Direct and explicitly related entries show complete original content when the budget allows; mentions show bounded original match windows and read coordinates. For example, GCC's `-Q` mentioning `--help` is not another `--help` definition. Use `--node` for strict navigation or to read the complete mentioned entry.
- `--explain-content-bytes BYTES`: Bound aggregate original forms/facts, match previews and body copies in an explanation page. The default is 1 MiB, the maximum 4 MiB, and the minimum one byte. An oversized body is omitted atomically with its location retained; increase the budget or read that node separately.

For progressive agent exploration, begin with the default summary, then reuse
the bracketed ID of a relevant section as `--outline-root` and request
only the needed entry kinds. A second rooted outline can expand one returned
entry before `--node` reads its complete content. The IDs below illustrate
values returned by one installed `bash(1)`; callers reuse the values from their
own preceding response:

```sh
mant bash --outline
mant bash --outline --outline-root shell-builtin-commands --outline-entries command
mant bash --outline --outline-root command-set --outline-entries all
mant bash --node command-set --format markdown
```

Rooting changes only the returned tree boundary. Paths and IDs remain unchanged
between projections of the same current document, unrelated siblings are
omitted, and every call independently rebuilds the local source. Reuse a path
or ID from the current outline response instead of guessing from a display
heading. Rediscover after the underlying document changes.

Outline path `0` and node ID `tldr` designate the reserved tldr outline node,
which contains either an external tldr page or a Markdown document's explicitly
marked tldr preface. It is not a native manual section. Remaining headings use
one-based paths such as `2.3`, and semantic entries use paths such as `2.3/e4`.
Nested entries append another component, for example `2.3/e4/e2`. One semantic
entry may retain several author-written forms without creating duplicate
nodes; aliases remain exact selectable spellings rather than display forms.
Paths are source-order coordinates and can move when an installed manual
changes; `2.3/e4` is not a stable ID. The separately printed bracketed ID is a
document-local selector derived from semantic identity. Automatically inferred
native entry IDs use the complete recognized name plus a role prefix, so a
navigation anchor such as `set` cannot turn `set-mark` into a misleading entry
ID or shadow the `set` command. When two semantic identities would otherwise
have the same ID, ManT adds a deterministic content fingerprint rather than a
source-order suffix. Native section IDs count only equal headings, and section
and entry allocation do not renumber each other. These properties keep IDs
stable across unrelated sibling insertion and reordering; an independently
updated manual can still change or remove the logical identity, so callers
must rediscover after document changes.
`--tldr` selects that reserved node alone and, unlike a general node
projection, explicitly permits a quick reference without a full document. It
uses the normal document priority chain, but considers only Markdown documents
that actually contain an embedded tldr preface; cached tldr occupies the same
priority-zero built-in position as native manuals.

`--node` and `--outline-root` share strict resolution: exact path, exact ID,
exact semantic alias, then normalized entry shorthand. Duplicate matches at
one precedence return candidate paths and IDs. A structural ID can shadow an
entry alias in strict navigation; the entry remains reachable by its path/ID.

`--explain` is independent evidence collection. Exact names, complete authored
forms, entry IDs/paths, literal content and validated explicit alias relations
are separate match bases. One owner's bases are combined; different owners,
including parent and child entries, remain separate in source order. A section
whose ID equals a documented name cannot hide the name's evidence. Name/form
matching follows the entry's declared case; literal content is case-sensitive
with token boundaries (`-a` does not match `--all`, and `-I` differs from `-i`).
There is no shorthand, fuzzy search, inferred synonym or command execution.
All three inputs reject controls and values over 512 Unicode scalars before
document resolution.

Explanation JSON is `mant.explanation/v0.11`, not an excerpt. Check `outcome`
(`evidence` or `no-evidence`), `total`, `returned`, `nextOffset`, independent
`truncation` flags and diagnostics. The outcome is computed before pagination,
so an empty later page can still have `outcome: evidence`. The collector keeps
at most 10,000 matching owners per document and follows at most 4,096 explicit
relationship edges with chains capped at 32. `semanticsComplete` describes
validation/producer coverage, not exhaustive recall. Use `--search` for broader
text investigation or `--node` to retrieve one complete owner. Scope JSON puts
the aggregate under `result.explanation` and includes each readable document's
evidence and coverage. MCP has the same semantics, with separate character
paging of the completed text.

## Search {#search-section}

<!-- mant:entries role=option case=sensitive -->
- `--search PATTERN`, `--grep PATTERN`: Search visible text and report reusable nodes plus Markdown coordinates. Both spellings select the same search operation. <!-- mant:entry {"aliasGroups":[["--search","--grep"]]} -->
- `--regex`: Interpret the pattern as a regular expression.
- `--case POLICY`: Use `insensitive`, `sensitive`, or `smart` case handling.
- `--word`: Require Unicode-aware word boundaries.
- `--scope SCOPE`: Search `visible` text or generated `markdown`.
- `--context LINES`: Include surrounding Markdown lines.
- `--limit COUNT`: Limit returned search lines, catalog rows, or explanation owners. Explanation defaults to 50 owners and accepts 1–256; search/catalog limits remain independent.
- `--offset COUNT`: Skip search lines, catalog rows, or explanation evidence for deterministic pagination. Explain uses one global cursor after evidence classification: direct entries, explicitly related entries, other-entry mentions, then ordinary mentions. Within each class it preserves document BFS order and original IR order; returned `nextOffset` continues that ordered result. Search instead remains document-BFS-first. MCP character paging is independent.

`markdown` searches the generated source, not a superset of visible text:
styling and escapes may interrupt an otherwise contiguous visible identifier.
Use `visible` to find names such as `NAME_PID` across those boundaries.

Search defaults to a case-insensitive literal over visible text, returns at
most 100 matching lines, and includes no context lines. Context cannot exceed
100 lines on either side of a match. Multiple occurrences on the same rendered
line form one pagination result. `smart` case becomes
case-sensitive when the pattern contains uppercase text. In regex mode, `^`
and `$` match the beginning and end of each rendered line. Regex patterns must
preserve Unicode mode and UTF-8 character boundaries; byte-oriented forms that
disable Unicode, such as `(?-u:.)`, are rejected before document matching.
Structured results always retain generated Markdown coordinates. Plain-text
visible searches show columns in the displayed text instead, while
Markdown-scope text searches show canonical Markdown columns; line numbers are
shared by both presentations. Reproduce the exact addressable
`mant.markdown/v1` coordinate text with
`mant SELECTOR --format markdown --preserve-anchors`.

Use the `=` form when a value begins with a hyphen:

```sh
mant tar --search=--acls
mant tar --explain=--exclude
```

## Output

<!-- mant:entries role=option case=sensitive -->
- `--format FORMAT`: Select `markdown`, `text`, `man`, or `json`.
- `--color WHEN`: Select `auto`, `always`, or `never` for human-readable terminal output.
- `--display MODE`: Select `auto`, `direct`, `pager`, or `tui`, independently of the content format.
- `--compact`: Omit JSON indentation.
- `--preserve-anchors`: Retain addressable HTML anchors in full-document or excerpt Markdown output.

Clean Markdown output omits internal HTML anchors by default. The `man` format
applies only to a complete native roff manual and emits plain manual content
without an external tldr preface; it rejects Markdown documents and partial
document views. All document queries default to text, including full documents,
outlines, nodes, explanations, searches, and queries supplied by `--request-json`.
Full interactive reading still opens the TUI; piped input never opens it
automatically. Select `--format markdown` when Markdown structure is useful;
`--preserve-anchors` also explicitly selects Markdown if no format is given.
JSON must be selected explicitly for document queries. Maintenance reports,
schemas, and protocol descriptions retain their JSON output. `--compact` removes indentation from
JSON queries, schemas, protocol descriptions, doctor reports, and update reports.

### Display Policy

`mant --help` prints the product summary, usage, option groups, an embedded
TLDR quick reference, and finally the self-manual commands. With no arguments,
`mant` prints a shorter usage diagnostic followed by the same TLDR and manual
footer; it exits with status `2` on stderr rather than opening a reader.
The quick reference is compiled from this manual and remains available when
the self manual is not installed. The final `mant mant` and
`mant mant --outline` suggestions require an installed self manual. Help uses
its own heading and command colours; TLDR descriptions and replaceable
values retain the terminal's normal foreground colour.

`auto` is the default. When both stdin and stdout are usable terminals,
ordinary full reading opens the TUI. Text outlines, excerpts, explanations,
searches, quick references, and catalog results use the built-in less-like
pager if their rendered content exceeds the screen; short content prints
directly. An explicit `--format text` selects this text/pager route rather
than the TUI. Terminal width, wrapping, and ANSI styling are handled by the
pager rather than estimated from byte counts.

Pipes, redirection, and `TERM=dumb` make automatic display direct. Standard-input
documents and request JSON always stay noninteractive, even if stdout is a
terminal. Help, diagnostics, doctor, and maintenance reports do not page
automatically. Explicit Markdown, man, and JSON output also stays direct in
automatic mode.

- `direct`: Never open the pager or TUI, including for full reading. It does not disable colour; use `--color never` when necessary.
- `pager`: Require a usable stdin/stdout terminal and show textual output through the pager. Short output still prints directly. Text, Markdown, and man output are supported; JSON and stdin document/request input are rejected. Doctor text may be explicitly paged.
- `tui`: Require full document reading and a usable stdin/stdout terminal. It cannot be combined with an explicit format, a partial projection, tldr-only reading, or stdin input.

Explicit interactive modes fail before document loading when no usable terminal
exists; only `auto` falls back to direct printing. Unsupported combinations
report usage errors rather than silently ignoring an option. Machine reports
accept only automatic or direct display; MCP does not accept `--display`.
Pager exit preserves the command's result status and restores the terminal;
diagnostics remain on stderr rather than entering the paged body.

The former `--ui` and `--no-pager` flags are removed, not aliases. Replace them
with `--display tui` and `--display direct`, respectively.

```sh
mant gcc --explain=-O --display pager
mant mant --outline --display direct
mant git --format markdown --display direct
```

### Colour and Terminal Safety

Help, diagnostics, the default tldr presentation, and document text
projections share the colour policy. Outline trees, selected nodes,
explanations, and search results use semantic ANSI roles without changing their
visible text. `auto` emits styling only to a capable terminal and respects
`NO_COLOR`, `CLICOLOR`, and `TERM=dumb`; `always` and `never` explicitly
override automatic detection. JSON, Markdown, man-format, and MCP results never
gain ANSI presentation styling. When Markdown is written
directly to a terminal, control characters in dynamic document identities are
masked so a path or catalog label cannot issue terminal commands. Redirected
full/excerpt Markdown preserves those data bytes exactly. Explanation text and
CommonMark always mask controls, including unchecked public IR producer data.

Colour controls text styling, not the TUI theme or terminal-control sequences
needed to operate an interactive screen. A pager is still a terminal destination:
its text receives the same terminal-safety treatment even with `--color never`.
stdout and stderr apply colour detection independently.

## Diagnostics

<!-- mant:entries role=option case=sensitive -->
- `--doctor`: Perform an offline, read-only inspection of the effective data root,
  source configuration and installations, registered documents, bundled
  libmandoc, native manual index, conditional Git requirement, and tldr roots. It
  does not create directories or lock files, invoke external programs, access the
  network, update caches, or remove orphaned sources. Suggested repairs name the
  existing explicit maintenance command instead of running it.
  An installed source reported as consistent matches its active local
  configuration, metadata, and recorded document count; this does not claim the
  remote source is current. The corresponding check explicitly says that remote
  freshness was not checked.

  Human-readable text is the default. `--format json` returns the independent
  `mant.doctor/v1` contract; add `--compact` for one-line JSON, and inspect its
  authoritative schema with `mant --schema doctor`. Warnings describe degraded or
  actionable local state and exit successfully. An error means a promised runtime
  capability is broken and exits with status `1`; invalid usage exits with status
  `2`.

  Doctor JSON intentionally includes local filesystem paths for diagnosis. It is
  a native CLI interface and is not exposed through the read-only MCP server.

`--update-tldr` JSON uses the independent `mant.tldr-update/v1` maintenance
contract. It reports `action` plus optional `cacheDir`, `client`, `output`, and
`revision`; inspect its schema with `mant --schema tldr-update`. MCP remains
read-only and cannot invoke this operation.

## Integration

<!-- mant:entries role=option case=sensitive -->
- `--request-json`: Read one closed `mant.request/v0.11` or `mant.scope-request/v0.11` object from standard input.
- `--schema CONTRACT`: Print a generated JSON Schema for `doctor`, `tldr-update`, `request`, `query`, `outline`, `excerpt`, `explanation`, `search`, `scope-request`, `scope-query`, `catalog`, or `all`.
- `--protocol-version`: Print the exact native protocol versions.
- `--mcp`: Serve read-only ManT tools over silent MCP stdio. Successful calls
  return bounded plain text or CommonMark without ordinary lowering
  diagnostics. Inspect complete structured details with CLI or request JSON.

### Protocol Discovery

The current protocol descriptor is:

```json
{
  "protocol": "mant.cli/v0.11",
  "nativeApiVersion": "0.11",
  "requestSchema": "mant.request/v0.11",
  "querySchema": "mant.query/v0.11",
  "documentSchema": "mant.document/v0.11",
  "outlineSchema": "mant.outline/v0.11",
  "excerptSchema": "mant.excerpt/v0.11",
  "searchSchema": "mant.search/v0.11",
  "scopeRequestSchema": "mant.scope-request/v0.11",
  "scopeQuerySchema": "mant.scope-query/v0.11",
  "catalogSchema": "mant.catalog/v0.11"
}
```

The native request and response family follows ManT's pre-stable minor release
line: ManT 0.11.x uses v0.11, and patch releases remain backward compatible.
They may add documented optional response fields but do not change requests,
required fields, tagged unions, or existing field semantics. The
former experimental bare v1 through v7 query schemas are no longer accepted.
Excerpt and search results now share a complete outline trail, so both human
output and structured consumers receive the same ancestor chain and terminal
node. Independent contracts such as the unchanged `mant.markdown/v1`
search-coordinate schema keep their own identifiers. Consumers must compare
every exact schema identifier; generated JSON Schema Draft 2020-12 definitions
remain authoritative.
The bundled [mant-protocol(5)](mant-protocol.md)
supplies the complete field reference, examples, compatibility policy,
coordinate rules, and MCP tool contracts.

Standard output is reserved for the requested result. Concise diagnostics use
standard error. `--request-json` accepts the same input and projection model
used by external process integrations. MCP exposes `mant_find`, `mant_outline`,
`mant_read`, `mant_explain`, and `mant_search`. `mant_find` merges local
Markdown candidates with the native manual index and returns canonical logical
IDs. It accepts literal or regex name matching, explicit case policy, and a
result offset. `mant_search` likewise exposes visible or generated-Markdown
scope plus a global matching-line-group offset. Focused tools accept one
unqualified selector or canonical ID, never an
arbitrary local path. Successful calls contain one bounded plain-text or
CommonMark result rather than a complete AST or schema envelope. Every result
starts with `chars`, `totalChars`, and optional `nextChar` metadata;
`startChar` and `maxChars` let the client select any Unicode-scalar range.
The largest page body is 32,768 scalars and therefore at most 131,072 UTF-8
bytes before MCP/JSON framing; it is not a 32 KiB byte page. `maxResults` and
`maxMatches` truncate the canonical result before character paging, so callers
must raise those limits or narrow the query to reach omitted rows or matches.
Result `offset` skips catalog rows, explanation owners or matching-line groups before that
materialization; `startChar` then pages only the resulting canonical text.
Paging is stateless: MCP reads current local files on every call, has no update
tool, and makes no cross-call snapshot guarantee.

## Data

<!-- mant:entries role=option case=sensitive -->
- `--update-docs`: Update Git or direct archive sources declared in `sources.toml` and print a complete JSON report.
- `--prune-docs`: Explicitly remove installed source directories absent from `sources.toml`; add `--dry-run` to report exact targets without removal.
- `--dry-run`: Preview the exact targets of `--prune-docs` without removing them. Requires `--prune-docs` and does not apply to source updates.
- `--update-tldr`: Update through an installed tldr client when available, otherwise through ManT's private cache.

Git-backed document sources require a `git` executable on `PATH`; direct
archives use built-in download and extraction support. The private tldr cache
fallback also requires Git when no installed client performs the update.

Normal queries prefer compatible installed-client data and always retain
ManT's private cache as the final fallback. Manual content remains usable when
no tldr page is available.

## Storage

ManT keeps durable documents and source metadata separate from disposable
caches. On Linux they live below
`${XDG_DATA_HOME:-$HOME/.local/share}/mant`; the private tldr checkout lives
below `${XDG_CACHE_HOME:-$HOME/.cache}/mant/tldr-pages`.

On macOS, documents live below `~/Library/Application Support/ManT` and the
private tldr checkout lives below `~/Library/Caches/ManT/tldr-pages`.

On Windows, documents live below `%APPDATA%\ManT`. ManT's private
tldr checkout lives below `%LOCALAPPDATA%\ManT\cache\tldr-pages`. The native
manual roots automatically include `%APPDATA%\ManT\man`, followed by the
compatible `%USERPROFILE%\.local\share\man` fallback; an optional
`%APPDATA%\ManT\man.conf` adds persistent native-manual roots ahead of both.

`sources.toml` lives at the data root. Personal documents remain below
`documents/`; installed source directories remain below `sources/`. See the online
[document-source guide](https://github.com/BryanHeBY/ManT/blob/main/docs/sources.md)
for the schema and update lifecycle.

## Environment

<!-- mant:entries role=environment-variable case=sensitive -->
- `MANT_MANPATH`: Completely replace ManT's manual roots. Lists use colons on
  Unix and semicolons on Windows. A root may contain flat roff files or section
  directories such as `man1/`, but is never an individual roff file.
- `MANPATH`: Override the derived manual roots. Empty components insert the
  complete host-derived default sequence at that position.
- `SYSTEM`: On Linux man-db hosts, expand derived manual roots through the
  named comma- or colon-separated operating-system subtrees; `man` retains the
  native root in that expansion.
- `MANT_TLDR_DIR`: Use one explicit tldr checkout for reads and updates.
- `XDG_CACHE_HOME`: Relocate cache discovery and ManT's Linux fallback cache.
- `XDG_DATA_HOME`: Relocate the user document directory from the default
  `$HOME/.local/share/mant/documents` on Linux.
- `XDG_DATA_DIRS`: Add installed-client tldr discovery roots; it does not add
  ManT document roots.
- `LC_ALL`, `LC_MESSAGES`, `LANGUAGE`, `LANG`: Select localized manual sources
  and translated tldr pages before English fallback.
- `MANSECT`: Order native manual categories for an unqualified name as a
  colon-separated list; unspecified categories follow after it.
- `PATHEXT`: On Windows, order executable suffix fallback for extensionless
  registered-document and native-manual queries.
- `NO_COLOR`: Disable automatic colour in human-readable terminal output.
- `CLICOLOR`: Set to `0` to disable automatic colour or a nonzero value to
  request it on a terminal.
- `CLICOLOR_FORCE`: Request colour even when automatic terminal detection
  would disable it; an explicit `--color` remains authoritative.
- `HOME`: On Unix, supply conventional document, manual, and cache locations
  when their XDG overrides are absent.
- `APPDATA`: Select the per-user ManT data root and its automatic `man/`
  manual root on Windows. Variables referenced as `%NAME%` in the ManT-owned
  Windows `man.conf` are read from the same process environment.
- `LOCALAPPDATA`: Select ManT and installed-client cache roots on Windows.
- `USERPROFILE`: Supply the compatible Windows manual fallback and
  installed-client tldr cache locations.

## General

<!-- mant:entries role=option case=sensitive -->
- `-h`, `--help`: Show command help and exit. <!-- mant:entry {"aliasGroups":[["-h","--help"]]} -->
- `-V`, `--version`: Show the installed ManT version and exit. <!-- mant:entry {"aliasGroups":[["-V","--version"]]} -->

## Exit Status

`0` indicates success, `2` indicates invalid input or usage, and `1` indicates
an operational failure.

Readable explanation queries with zero or multiple evidence owners return `0`;
inspect `outcome` and coverage instead of treating them as selection errors.
Strict node navigation still rejects unresolved or ambiguous selectors.

## See Also

[mant-protocol(5)](mant-protocol.md), [mant-ir(7)](mant-ir.md), [mant-markdown(7)](mant-markdown.md), and [mant-roff(7)](mant-roff.md) describe the machine boundary, normalized model, and accepted input languages. The [document-source guide](https://github.com/BryanHeBY/ManT/blob/main/docs/sources.md) defines `sources.toml` and its update lifecycle.
