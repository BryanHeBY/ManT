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
    CEquationBoxView, CNodeView, CTableCellView, CTableRuleCellView,
    mant_mandoc_eqn_box_view_align, mant_mandoc_eqn_box_view_offsets,
    mant_mandoc_eqn_box_view_size, mant_mandoc_node_view_align, mant_mandoc_node_view_offsets,
    mant_mandoc_node_view_size, mant_mandoc_table_cell_view_align,
    mant_mandoc_table_cell_view_offsets, mant_mandoc_table_cell_view_size,
    mant_mandoc_table_rule_cell_view_align, mant_mandoc_table_rule_cell_view_offsets,
    mant_mandoc_table_rule_cell_view_size,
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
        CEquationBoxView, CNodeView, CTableCellView, CTableRuleCellView, InputFormat, Node,
        mant_mandoc_eqn_box_view_align, mant_mandoc_eqn_box_view_offsets,
        mant_mandoc_eqn_box_view_size, mant_mandoc_node_view_align, mant_mandoc_node_view_offsets,
        mant_mandoc_node_view_size, mant_mandoc_table_cell_view_align,
        mant_mandoc_table_cell_view_offsets, mant_mandoc_table_cell_view_size,
        mant_mandoc_table_rule_cell_view_align, mant_mandoc_table_rule_cell_view_offsets,
        mant_mandoc_table_rule_cell_view_size, parse_buffer,
    };

    macro_rules! check_view {
        ($name:literal, $view:ty, $size:path, $align:path, $offsets:path, [$($field:ident),+ $(,)?]) => {{
            assert_eq!(unsafe { $size() }, std::mem::size_of::<$view>(), "{} size", $name);
            assert_eq!(unsafe { $align() }, std::mem::align_of::<$view>(), "{} alignment", $name);
            let expected = [$(std::mem::offset_of!($view, $field)),+];
            let mut count = 0;
            let native = unsafe { $offsets(&mut count) };
            assert_eq!(count, expected.len(), "{} field count", $name);
            assert!(!native.is_null(), "{} field offsets", $name);
            // The shim returns immutable static storage, and the checked
            // length bounds this slice before any native offset is read.
            let actual = unsafe { std::slice::from_raw_parts(native, count) };
            assert_eq!(actual, expected, "{} field offsets", $name);
        }};
    }

    #[test]
    fn borrowed_node_view_matches_the_native_abi() {
        check_view!(
            "node",
            CNodeView,
            mant_mandoc_node_view_size,
            mant_mandoc_node_view_align,
            mant_mandoc_node_view_offsets,
            [
                kind,
                section,
                end_kind,
                end_body,
                reference_quotes_title,
                macro_name,
                text,
                tag,
                line,
                column,
                flow_epoch,
                table_escape,
                table_source_recovery_safe,
                table_row_kind,
                flags,
                list_kind,
                definition_list_style,
                display_kind,
                font_kind,
                author_mode,
                compact,
                offset,
                width,
                enclosure_open,
                enclosure_close,
                equation,
                table_cells,
                table_rule_cells,
                child,
                next,
            ]
        );
    }

    #[test]
    fn borrowed_table_cell_view_matches_the_native_abi() {
        check_view!(
            "table cell",
            CTableCellView,
            mant_mandoc_table_cell_view_size,
            mant_mandoc_table_cell_view_align,
            mant_mandoc_table_cell_view_offsets,
            [
                text,
                kind,
                text_block,
                source_recovery_safe,
                vertical_continuation,
                column_span,
                row_span,
                alignment,
                font,
                next,
            ]
        );
    }

    #[test]
    fn borrowed_table_rule_cell_view_matches_the_native_abi() {
        check_view!(
            "table rule cell",
            CTableRuleCellView,
            mant_mandoc_table_rule_cell_view_size,
            mant_mandoc_table_rule_cell_view_align,
            mant_mandoc_table_rule_cell_view_offsets,
            [kind, next]
        );
    }

    #[test]
    fn borrowed_equation_view_matches_the_native_abi() {
        check_view!(
            "equation",
            CEquationBoxView,
            mant_mandoc_eqn_box_view_size,
            mant_mandoc_eqn_box_view_align,
            mant_mandoc_eqn_box_view_offsets,
            [
                kind,
                font,
                position,
                size,
                expected_args,
                actual_args,
                text,
                left,
                right,
                top,
                bottom,
                first,
                next,
            ]
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
            + node
                .equation
                .as_ref()
                .map_or(0, |equation| equation.readable_text().len())
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
