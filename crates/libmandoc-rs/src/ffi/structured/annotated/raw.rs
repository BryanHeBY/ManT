//! Private versioned annotated ABI layouts and extern declarations.

use super::super::raw::{InputView, Limits};
use super::super::{BytesView, FailureView, MetadataView, SliceView};

#[repr(C)]
pub(super) struct ResultHandleRaw {
    pub(super) _private: [u8; 0],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct LabelView {
    pub(super) owner: u32,
    pub(super) link: u32,
    pub(super) source: u32,
    pub(super) style: u32,
    pub(super) role: u32,
    pub(super) glyph_origin: u64,
    pub(super) flags: u32,
    pub(super) reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct RowView {
    pub(super) key: u32,
    pub(super) first_run: u32,
    pub(super) run_count: u32,
    pub(super) column_count: u32,
    pub(super) break_after: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct RunView {
    pub(super) key: u32,
    pub(super) column: u32,
    pub(super) width: u32,
    pub(super) reserved: u32,
    pub(super) byte_start: u64,
    pub(super) byte_count: u64,
    pub(super) label: LabelView,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct MarkView {
    pub(super) key: u32,
    pub(super) kind: u32,
    pub(super) parent: u32,
    pub(super) owner: u32,
    pub(super) source: u32,
    pub(super) line: u32,
    pub(super) column: u32,
    pub(super) token: u32,
    pub(super) region_kind: u32,
    pub(super) title_region: u32,
    pub(super) body_region: u32,
    pub(super) flags: u32,
    pub(super) reserved: u32,
    pub(super) table_column: u32,
    pub(super) table_position_present: u32,
    pub(super) table_offset: u64,
    pub(super) name: *const u8,
    pub(super) name_length: u64,
    pub(super) target_kind: u32,
    pub(super) target_b_present: u32,
    pub(super) target_a: BytesView,
    pub(super) target_b: BytesView,
    pub(super) selection_first: u32,
    pub(super) selection_count: u32,
    pub(super) point_kind: u32,
    pub(super) point_row: u32,
    pub(super) point_column: u32,
    pub(super) point_reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct SelectionPartView {
    pub(super) run: u32,
    pub(super) join_before: u32,
    pub(super) start_byte: u64,
    pub(super) end_byte: u64,
    pub(super) join_text_start: u64,
    pub(super) join_text_len: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct CoverageCheckView {
    pub(super) producer: u32,
    pub(super) dimension: u32,
    pub(super) state: u32,
    pub(super) reserved: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct CoverageIssueView {
    pub(super) producer: u32,
    pub(super) dimension: u32,
    pub(super) reason: u32,
    pub(super) scope: u32,
    pub(super) scope_key: u32,
    pub(super) source: u32,
    pub(super) line: u32,
    pub(super) column: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct DisplayView {
    pub(super) bytes: *const u8,
    pub(super) byte_count: u64,
    pub(super) rows: *const RowView,
    pub(super) row_count: u32,
    pub(super) runs: *const RunView,
    pub(super) run_count: u32,
    pub(super) input_bytes: u64,
    pub(super) work: u64,
    pub(super) peak_allocated_bytes: u64,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(super) struct ResultView {
    pub(super) root_source: u32,
    pub(super) profile: u32,
    pub(super) width: u32,
    pub(super) reserved: u32,
    pub(super) metadata: MetadataView,
    pub(super) sources: SliceView,
    pub(super) spans: SliceView,
    pub(super) provenances: SliceView,
    pub(super) diagnostics: SliceView,
    pub(super) marks: SliceView,
    pub(super) coverage_checks: SliceView,
    pub(super) coverage_issues: SliceView,
    pub(super) display: DisplayView,
    pub(super) selection_parts: SliceView,
    pub(super) join_text: SliceView,
}

unsafe extern "C" {
    pub(super) fn mant_annotated_abi_version() -> u32;
    pub(super) fn mant_annotated_render(
        input: *const InputView,
        limits: *const Limits,
        result: *mut *mut ResultHandleRaw,
        failure: *mut FailureView,
    ) -> u32;
    pub(super) fn mant_annotated_result_check(
        result: *const ResultHandleRaw,
        failure: *mut FailureView,
    ) -> u32;
    pub(super) fn mant_annotated_result_view(
        result: *const ResultHandleRaw,
        view: *mut ResultView,
    ) -> u32;
    pub(super) fn mant_annotated_result_free(result: *mut ResultHandleRaw);
    pub(super) fn mant_annotated_sizeof_result_view() -> usize;
    pub(super) fn mant_annotated_alignof_result_view() -> usize;
    pub(super) fn mant_annotated_offsetof_result_view_display() -> usize;
    pub(super) fn mant_annotated_sizeof_display_row() -> usize;
    pub(super) fn mant_annotated_alignof_display_row() -> usize;
    pub(super) fn mant_annotated_offsetof_display_row_break_after() -> usize;
    pub(super) fn mant_annotated_sizeof_display_run() -> usize;
    pub(super) fn mant_annotated_alignof_display_run() -> usize;
    pub(super) fn mant_annotated_offsetof_display_run_label() -> usize;
    pub(super) fn mant_annotated_sizeof_display_label() -> usize;
    pub(super) fn mant_annotated_alignof_display_label() -> usize;
    pub(super) fn mant_annotated_offsetof_display_label_glyph_origin() -> usize;
    pub(super) fn mant_annotated_sizeof_mark() -> usize;
    pub(super) fn mant_annotated_alignof_mark() -> usize;
    pub(super) fn mant_annotated_offsetof_mark_name() -> usize;
    pub(super) fn mant_annotated_offsetof_mark_table_offset() -> usize;
    pub(super) fn mant_annotated_offsetof_mark_target_a() -> usize;
    pub(super) fn mant_annotated_offsetof_mark_selection_first() -> usize;
    pub(super) fn mant_annotated_offsetof_mark_point_kind() -> usize;
    pub(super) fn mant_annotated_sizeof_selection_part() -> usize;
    pub(super) fn mant_annotated_alignof_selection_part() -> usize;
    pub(super) fn mant_annotated_offsetof_selection_part_end_byte() -> usize;
    pub(super) fn mant_annotated_offsetof_selection_part_join_text_start() -> usize;
    pub(super) fn mant_annotated_sizeof_coverage_check() -> usize;
    pub(super) fn mant_annotated_alignof_coverage_check() -> usize;
    pub(super) fn mant_annotated_sizeof_coverage_issue() -> usize;
    pub(super) fn mant_annotated_alignof_coverage_issue() -> usize;
    pub(super) fn mant_annotated_offsetof_result_view_coverage_checks() -> usize;
    pub(super) fn mant_annotated_offsetof_result_view_coverage_issues() -> usize;
    pub(super) fn mant_annotated_offsetof_result_view_selection_parts() -> usize;
    pub(super) fn mant_annotated_offsetof_result_view_join_text() -> usize;
}
