//! Safe ownership boundary around Mant's pinned libmandoc parser.
//!
//! The C shim completes and copies a parse before returning. Rust therefore
//! never observes libmandoc's private `roff_node` layout, and the global C
//! parser state is serialized inside this crate.

use std::{
    ffi::CString,
    fmt,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

#[cfg(test)]
mod build_config;

#[allow(unsafe_code)]
mod ffi;

/// Pinned upstream version compiled by this crate's build script.
pub const MANDOC_VERSION: &str = "1.14.6";

static PARSER_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

/// High-level macro package detected by libmandoc.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MacroSet {
    None,
    Mdoc,
    Man,
}

/// Renderer-neutral node role copied from the libmandoc syntax tree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NodeKind {
    Root,
    Block,
    Head,
    Body,
    Tail,
    Element,
    Text,
    Comment,
    Table,
    Equation,
}

/// Normalized mdoc list behavior copied independently of upstream enum values.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum NormalizedListKind {
    Bullet,
    Ordered,
    Definition,
    Column,
    Plain,
}

/// Whether an mdoc display preserves source line layout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DisplayKind {
    Literal,
    Filled,
}

/// Source and renderer flags needed by the future AST lowering pass.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct NodeFlags {
    pub generated: bool,
    pub sentence_end: bool,
    pub no_print: bool,
    pub no_fill: bool,
}

/// An owned syntax node with no pointers into the C parser.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Node {
    pub kind: NodeKind,
    pub macro_name: Option<String>,
    pub text: Option<String>,
    pub line: u32,
    pub column: u32,
    pub flags: NodeFlags,
    pub list_kind: Option<NormalizedListKind>,
    pub display_kind: Option<DisplayKind>,
    pub compact: bool,
    pub offset: Option<String>,
    pub children: Vec<Self>,
}

/// Metadata copied from a completed libmandoc parse.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Metadata {
    pub title: Option<String>,
    pub section: Option<String>,
    pub volume: Option<String>,
    pub os: Option<String>,
    pub arch: Option<String>,
    pub name: Option<String>,
    pub date: Option<String>,
    pub alias_target: Option<String>,
    pub has_body: bool,
}

/// Complete owned output of the low-level parser session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedDocument {
    pub macro_set: MacroSet,
    pub metadata: Metadata,
    pub diagnostics: String,
    pub root: Node,
}

/// File-level failure reported without leaking C or runtime diagnostics.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParseError {
    pub path: PathBuf,
    pub message: String,
}

impl fmt::Display for ParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.path.display(), self.message)
    }
}

impl std::error::Error for ParseError {}

/// Parse one source file, optionally resolving `.so` includes relative to it.
///
/// libmandoc 1.14.6 keeps diagnostics and character tables in process-global
/// state. Serializing the entire C call ensures concurrent Node-API requests
/// cannot overwrite one another.
///
/// # Errors
///
/// Returns [`ParseError`] when the path cannot be represented for C, the
/// source cannot be opened, or libmandoc does not produce a valid tree.
pub fn parse_file(path: &Path, allow_includes: bool) -> Result<ParsedDocument, ParseError> {
    let c_path = CString::new(path.as_os_str().as_bytes()).map_err(|_| ParseError {
        path: path.to_path_buf(),
        message: "manual source path contains a NUL byte".into(),
    })?;
    let lock = PARSER_LOCK.get_or_init(|| Mutex::new(()));
    let _guard = lock
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    ffi::parse_file(&c_path, allow_includes).map_err(|message| ParseError {
        path: path.to_path_buf(),
        message,
    })
}

#[cfg(test)]
mod tests {
    use std::{fs, process};

    use super::{DisplayKind, MacroSet, Node, NodeKind, NormalizedListKind, parse_file};

    fn source_path(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("mant-{label}-{}.1", process::id()))
    }

    fn find_macro<'a>(node: &'a Node, name: &str) -> Option<&'a Node> {
        (node.macro_name.as_deref() == Some(name))
            .then_some(node)
            .or_else(|| {
                node.children
                    .iter()
                    .find_map(|child| find_macro(child, name))
            })
    }

    #[test]
    fn upstream_version_is_pinned() {
        assert_eq!(super::MANDOC_VERSION, "1.14.6");
    }

    #[test]
    fn parser_session_returns_an_owned_man_tree() {
        let path = source_path("mandoc-session");
        fs::write(
            &path,
            ".TH MANT 1 \"2026-07-19\"\n.SH NAME\nmant \\- manual viewer\n",
        )
        .expect("write temporary manual source");

        let document = parse_file(&path, false).expect("parse temporary manual");
        fs::remove_file(path).expect("remove temporary manual source");

        assert_eq!(document.macro_set, MacroSet::Man);
        assert_eq!(document.metadata.title.as_deref(), Some("MANT"));
        assert_eq!(document.metadata.section.as_deref(), Some("1"));
        assert!(document.metadata.has_body);
        assert_eq!(document.root.kind, NodeKind::Root);
        assert!(!document.root.children.is_empty());
    }

    #[test]
    fn parser_session_reports_file_errors_as_values() {
        let path = source_path("missing-mandoc-session");
        let error = parse_file(&path, false).expect_err("missing source must fail");

        assert_eq!(error.path, path);
        assert!(!error.message.is_empty());
    }

    #[test]
    fn concurrent_callers_are_serialized_around_libmandoc_globals() {
        let path = source_path("concurrent-mandoc-session");
        fs::write(&path, ".TH THREADS 1\n.SH NAME\nthreads \\- test\n")
            .expect("write temporary manual source");

        let workers: Vec<_> = (0..4)
            .map(|_| {
                let path = path.clone();
                std::thread::spawn(move || parse_file(&path, false))
            })
            .collect();
        for worker in workers {
            let document = worker
                .join()
                .expect("parser worker must not panic")
                .expect("concurrent parse must succeed");
            assert_eq!(document.metadata.title.as_deref(), Some("THREADS"));
        }

        fs::remove_file(path).expect("remove temporary manual source");
    }

    #[test]
    fn source_relative_includes_do_not_change_process_cwd() {
        let repository = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
        let alias = repository.join("tests/fixtures/roff/alias-mdoc.1");
        let cwd = std::env::current_dir().expect("current directory before parse");

        let document = parse_file(&alias, true).expect("resolve source-relative include");

        assert_eq!(document.macro_set, MacroSet::Mdoc);
        assert_eq!(
            document.metadata.title.as_deref(),
            Some("MANT-MDOC-FIXTURE")
        );
        assert_eq!(
            std::env::current_dir().expect("current directory after parse"),
            cwd
        );
    }

    #[test]
    fn parser_copies_normalized_list_and_display_attributes() {
        let path = source_path("normalized-mandoc-session");
        fs::write(
            &path,
            ".Dd July 19, 2026\n.Dt NORMALIZED 1\n.Os\n.Sh ITEMS\n\
             .Bl -enum -compact -offset indent\n.It\nfirst\n.El\n\
             .Bd -literal -offset indent\ncode line\n.Ed\n",
        )
        .expect("write normalized mdoc source");

        let document = parse_file(&path, false).expect("parse normalized mdoc source");
        fs::remove_file(path).expect("remove normalized mdoc source");

        let list = find_macro(&document.root, "Bl").expect("normalized list node");
        assert_eq!(list.list_kind, Some(NormalizedListKind::Ordered));
        assert!(list.compact);
        assert_eq!(list.offset.as_deref(), Some("indent"));
        let display = find_macro(&document.root, "Bd").expect("normalized display node");
        assert_eq!(display.display_kind, Some(DisplayKind::Literal));
        assert_eq!(display.offset.as_deref(), Some("indent"));
    }
}
