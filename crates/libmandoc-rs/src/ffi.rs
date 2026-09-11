//! Immediate ownership transfer from an opaque native session.
mod owned;
mod raw;
#[cfg(feature = "render")]
mod render;
mod session;
#[cfg(windows)]
mod windows_root;

#[cfg(test)]
use crate::{InputFormat, Node};
#[cfg(test)]
use raw::{
    CNodeView, CTableCellView, mant_mandoc_node_view_size, mant_mandoc_table_cell_view_size,
};
#[cfg(windows)]
use raw::{CResolvedSource, CSourceResolver};
#[cfg(all(feature = "render", test))]
pub(crate) use render::ctype_locale;
#[cfg(all(feature = "render", unix))]
pub(super) use render::render_file;
#[cfg(feature = "render")]
pub(super) use render::{NativeRenderError, render_buffer, render_bundle};
#[cfg(unix)]
pub(super) use session::parse_file;
pub(super) use session::{parse_buffer, parse_bundle};

pub(super) fn is_native_roff_request(name: &str) -> bool {
    // The C shim consumes the borrowed byte slice synchronously and takes an
    // explicit length, so it neither stores nor requires a NUL terminator.
    unsafe { raw::mant_mandoc_is_native_roff_request(name.as_ptr().cast(), name.len()) != 0 }
}

#[cfg(test)]
mod tests {
    use std::{
        ffi::CString,
        fs,
        io::Read,
        path::{Path, PathBuf},
    };

    use flate2::read::MultiGzDecoder;

    use super::{
        CNodeView, CTableCellView, InputFormat, Node, mant_mandoc_node_view_size,
        mant_mandoc_table_cell_view_size, parse_buffer,
    };

    #[test]
    fn borrowed_snapshot_views_match_the_native_abi() {
        assert_eq!(
            unsafe { mant_mandoc_node_view_size() },
            std::mem::size_of::<CNodeView>()
        );
        assert_eq!(
            unsafe { mant_mandoc_table_cell_view_size() },
            std::mem::size_of::<CTableCellView>()
        );
    }

    #[test]
    fn owned_transfer_preserves_semantic_edges_after_parser_release() {
        assert_owned_transfer(
            "semantic-man.1",
            br".TH TRANSFER 1
.SH NAME
transfer \- ownership boundary
.TS
tab(|);
l l.
left|right
.TE
.EQ
x sup 2
.EN
",
        );
        assert_owned_transfer(
            "semantic-mdoc.1",
            br".Dd August 23, 2026
.Dt TRANSFER 1
.Os
.Sh NAME
.Nm transfer
.Nd ownership boundary
.Bl -bullet -compact -offset indent -width Ds
.It item
.El
.Bd -literal -offset indent
literal display
.Ed
.Bf -emphasis
emphasis
.Ef
.Es ( )
.En wrapped
.An -split
",
        );

        let mut nested = String::from(".TH DEEP 1\n");
        for _ in 0..180 {
            nested.push_str(".RS\n");
        }
        nested.push_str("bounded\n");
        for _ in 0..180 {
            nested.push_str(".RE\n");
        }
        assert_owned_transfer("deep.1", nested.as_bytes());
    }

    #[test]
    fn owned_transfer_survives_parser_release_for_real_fixtures() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join("tests/fixtures/roff/real");
        if !root.is_dir() {
            return;
        }
        let mut fixtures = Vec::new();
        collect_manuals(&root, &mut fixtures);
        fixtures.sort();
        assert!(
            fixtures.len() >= 20,
            "the repository fixture corpus unexpectedly contains only {} manuals",
            fixtures.len()
        );
        for fixture in fixtures {
            let source = read_fixture(&fixture);
            assert_owned_transfer(&fixture.to_string_lossy(), &source);
        }
    }

    fn assert_owned_transfer(label: &str, source: &[u8]) {
        let path = CString::new(label).expect("fixture labels contain no NUL bytes");
        // `parse_buffer` destroys its private native parser handle before it
        // returns. Traversing every owned string and cell afterwards catches
        // any borrowed pointer that accidentally escaped the FFI boundary.
        let parsed = parse_buffer(&path, source, None, false, InputFormat::Auto, None)
            .unwrap_or_else(|error| panic!("owned transfer failed for {label}: {error}"));
        let (nodes, bytes) = touch_owned_node(&parsed.document.root);
        assert!(
            nodes > 1,
            "owned syntax tree is unexpectedly empty for {label}"
        );
        assert!(
            bytes > 0,
            "owned syntax tree has no string data for {label}"
        );
        let _ = parsed.diagnostics.len();
    }

    fn touch_owned_node(node: &Node) -> (usize, usize) {
        let mut bytes = node.macro_name.as_ref().map_or(0, String::len)
            + node.text.as_ref().map_or(0, String::len)
            + node.tag.as_ref().map_or(0, String::len)
            + node.offset.as_ref().map_or(0, String::len)
            + node.width.as_ref().map_or(0, String::len)
            + node.equation.as_ref().map_or(0, String::len)
            + node.enclosure.as_ref().map_or(0, |enclosure| {
                enclosure.opening.len() + enclosure.closing.as_ref().map_or(0, String::len)
            })
            + node
                .table_cells
                .iter()
                .map(|cell| cell.text.as_ref().map_or(0, String::len))
                .sum::<usize>();
        let mut nodes = 1;
        for child in &node.children {
            let (child_nodes, child_bytes) = touch_owned_node(child);
            nodes += child_nodes;
            bytes += child_bytes;
        }
        (nodes, bytes)
    }

    fn collect_manuals(directory: &Path, output: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(directory).expect("read real fixture directory") {
            let path = entry.expect("read fixture entry").path();
            if path.is_dir() {
                collect_manuals(&path, output);
            } else if is_manual(&path) {
                output.push(path);
            }
        }
    }

    fn is_manual(path: &Path) -> bool {
        path.extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                matches!(extension, "gz" | "zst")
                    || extension.as_bytes().first().is_some_and(u8::is_ascii_digit)
            })
    }

    fn read_fixture(path: &Path) -> Vec<u8> {
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("gz") => {
                let mut decoded = Vec::new();
                MultiGzDecoder::new(fs::File::open(path).expect("open gzip fixture"))
                    .read_to_end(&mut decoded)
                    .expect("decode gzip fixture");
                decoded
            }
            Some("zst") => {
                zstd::stream::decode_all(fs::File::open(path).expect("open zstd fixture"))
                    .expect("decode zstd fixture")
            }
            _ => fs::read(path).expect("read plain fixture"),
        }
    }
}
