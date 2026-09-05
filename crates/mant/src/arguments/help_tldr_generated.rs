// Generated from docs/manuals/mant.md; do not edit.
// Regenerate: cargo run --locked -p mant --example generate_help_tldr
#[rustfmt::skip]
pub(super) const EXAMPLES: &[HelpExample] = &[
    ("Read Git's complete manual in the interactive reader", &[
        ("mant git", false),
    ]),
    ("Extract a tar option entry as portable Markdown", &[
        ("mant tar --explain=", false),
        ("--exclude", true),
        (" --format markdown", false),
    ]),
    ("Search Git and its linked manuals for a topic, limited to two hops", &[
        ("mant git --search ", false),
        ("worktree", true),
        (" --follow-links --max-depth 2 --max-documents 32 --context 1", false),
    ]),
    ("Discover up to 20 native manuals matching a name as compact JSON", &[
        ("mant --find ", false),
        ("'^git'", true),
        (" --regex --kind manual --limit 20 --format json --compact", false),
    ]),
    ("Validate a local Markdown manual's semantic outline and diagnostics", &[
        ("mant --input ", false),
        ("./tool.md", true),
        (" --outline --outline-entries all --format json --compact", false),
    ]),
    ("Serve local documentation to an MCP client over standard input and output", &[
        ("mant --mcp", false),
    ]),
];
