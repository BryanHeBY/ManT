/* Checked borrowed view and destruction for one annotated native result. */
#include "mant_mandoc_annotated_internal.h"
#include "mant_mandoc_structured_session.h"

#include <stdlib.h>
#include <string.h>

uint32_t
mant_annotated_abi_version(void)
{
	return 12;
}

/* Only a fully finished and independently checked display may take this
 * path.  Keeping any old owner/link key would make discarded annotations
 * appear actionable to a consumer.  This operation does not edit body bytes,
 * geometry, style or source identities and needs no new allocation. */
int
mant_annotated_result_strip_annotations(struct mant_annotated_result *result)
{
	if (result == NULL || result->checked != 0 ||
	    !mant_annotated_result_is_surface_valid(result) ||
	    !mant_annotated_display_clear_annotations(result->display))
		return 0;
	mant_annotated_marks_free(result->marks, result->mark_count);
	result->marks = NULL;
	result->mark_count = 0;
	free(result->selection_parts);
	result->selection_parts = NULL;
	result->selection_part_count = 0;
	free(result->join_text);
	result->join_text = NULL;
	result->join_text_count = 0;
	free(result->coverage_issues);
	result->coverage_issues = NULL;
	result->coverage_issue_count = 0;
	result->coverage_issue_capacity = 0;
	memset(result->coverage_checks, 0, sizeof(result->coverage_checks));
	result->annotation_degraded = 1;
	return 1;
}

uint32_t
mant_annotated_result_check(const struct mant_annotated_result *result,
    struct mant_structured_failure_view *failure)
{
	if (failure == NULL)
		return MANT_STRUCTURED_INVALID_INPUT;
	mant_structured_clear_failure(failure);
	if (!mant_annotated_result_is_valid(result) || result->checked == 0) {
		failure->status = MANT_STRUCTURED_RELATION;
		failure->stage = MANT_STRUCTURED_STAGE_CHECK;
		return failure->status;
	}
	return MANT_STRUCTURED_OK;
}

#define VIEW_SLICE(data, count, type) ((struct mant_slice_view){ \
	(data), (count), sizeof(type) })

uint32_t
mant_annotated_result_view(const struct mant_annotated_result *result,
    struct mant_annotated_result_view *view)
{
	const struct mant_structured_result *common;

	if (view == NULL)
		return MANT_STRUCTURED_INVALID_INPUT;
	memset(view, 0, sizeof(*view));
	if (result == NULL || result->checked == 0 ||
	    !mant_annotated_result_is_valid(result))
		return MANT_STRUCTURED_RELATION;
	common = result->common;
	view->root_source = common->root_source;
	view->profile = common->profile;
	view->width = common->width;
	view->annotation_degraded = result->annotation_degraded;
	view->metadata = common->metadata;
	view->sources = VIEW_SLICE(common->sources, common->source_count,
	    struct mant_structured_source_view);
	view->spans = VIEW_SLICE(common->spans, common->span_count,
	    struct mant_structured_span_view);
	view->provenances = VIEW_SLICE(common->provenances,
	    common->provenance_count, struct mant_structured_provenance_view);
	view->diagnostics = VIEW_SLICE(common->diagnostics,
	    common->diagnostic_count, struct mant_structured_diagnostic_view);
	view->marks = VIEW_SLICE(result->marks, result->mark_count,
	    struct mant_annotated_mark);
	view->coverage_checks = VIEW_SLICE(result->coverage_checks,
	    sizeof(result->coverage_checks) /
	    sizeof(result->coverage_checks[0]),
	    struct mant_annotated_coverage_check);
	view->coverage_issues = VIEW_SLICE(result->coverage_issues,
	    result->coverage_issue_count,
	    struct mant_annotated_coverage_issue);
	view->selection_parts = VIEW_SLICE(result->selection_parts,
	    result->selection_part_count,
	    struct mant_annotated_selection_part);
	view->join_text = VIEW_SLICE(result->join_text,
	    result->join_text_count, uint8_t);
	if (!mant_annotated_display_finish(result->display, &view->display))
		return MANT_STRUCTURED_RELATION;
	return MANT_STRUCTURED_OK;
}

void
mant_annotated_result_free(struct mant_annotated_result *result)
{
	if (result == NULL)
		return;
	mant_annotated_display_free(result->display);
	mant_annotated_marks_free(result->marks, result->mark_count);
	free(result->selection_parts);
	free(result->join_text);
	free(result->coverage_issues);
	mant_structured_result_free(result->common);
	result->magic = 0;
	free(result);
}

size_t mant_annotated_sizeof_result_view(void)
{ return sizeof(struct mant_annotated_result_view); }
size_t mant_annotated_alignof_result_view(void)
{ return _Alignof(struct mant_annotated_result_view); }
size_t mant_annotated_offsetof_result_view_display(void)
{ return offsetof(struct mant_annotated_result_view, display); }
size_t mant_annotated_sizeof_display_row(void)
{ return sizeof(struct mant_annotated_display_row); }
size_t mant_annotated_alignof_display_row(void)
{ return _Alignof(struct mant_annotated_display_row); }
size_t mant_annotated_offsetof_display_row_break_after(void)
{ return offsetof(struct mant_annotated_display_row, break_after); }
size_t mant_annotated_sizeof_display_run(void)
{ return sizeof(struct mant_annotated_display_run); }
size_t mant_annotated_alignof_display_run(void)
{ return _Alignof(struct mant_annotated_display_run); }
size_t mant_annotated_offsetof_display_run_label(void)
{ return offsetof(struct mant_annotated_display_run, label); }
size_t mant_annotated_sizeof_display_label(void)
{ return sizeof(struct mant_annotated_display_label); }
size_t mant_annotated_alignof_display_label(void)
{ return _Alignof(struct mant_annotated_display_label); }
size_t mant_annotated_offsetof_display_label_glyph_origin(void)
{ return offsetof(struct mant_annotated_display_label, glyph_origin); }
size_t mant_annotated_offsetof_display_label_head_component(void)
{ return offsetof(struct mant_annotated_display_label, head_component); }
size_t mant_annotated_sizeof_mark(void)
{ return sizeof(struct mant_annotated_mark); }
size_t mant_annotated_alignof_mark(void)
{ return _Alignof(struct mant_annotated_mark); }
size_t mant_annotated_offsetof_mark_name(void)
{ return offsetof(struct mant_annotated_mark, name); }
size_t mant_annotated_offsetof_mark_table_offset(void)
{ return offsetof(struct mant_annotated_mark, table_offset); }
size_t mant_annotated_offsetof_mark_target_a(void)
{ return offsetof(struct mant_annotated_mark, target_a); }
size_t mant_annotated_offsetof_mark_selection_first(void)
{ return offsetof(struct mant_annotated_mark, selection_first); }
size_t mant_annotated_offsetof_mark_point_kind(void)
{ return offsetof(struct mant_annotated_mark, point_kind); }
size_t mant_annotated_sizeof_selection_part(void)
{ return sizeof(struct mant_annotated_selection_part); }
size_t mant_annotated_alignof_selection_part(void)
{ return _Alignof(struct mant_annotated_selection_part); }
size_t mant_annotated_offsetof_selection_part_end_byte(void)
{ return offsetof(struct mant_annotated_selection_part, end_byte); }
size_t mant_annotated_offsetof_selection_part_join_text_start(void)
{ return offsetof(struct mant_annotated_selection_part, join_text_start); }
size_t mant_annotated_sizeof_coverage_check(void)
{ return sizeof(struct mant_annotated_coverage_check); }
size_t mant_annotated_alignof_coverage_check(void)
{ return _Alignof(struct mant_annotated_coverage_check); }
size_t mant_annotated_sizeof_coverage_issue(void)
{ return sizeof(struct mant_annotated_coverage_issue); }
size_t mant_annotated_alignof_coverage_issue(void)
{ return _Alignof(struct mant_annotated_coverage_issue); }
size_t mant_annotated_offsetof_result_view_coverage_checks(void)
{ return offsetof(struct mant_annotated_result_view, coverage_checks); }
size_t mant_annotated_offsetof_result_view_coverage_issues(void)
{ return offsetof(struct mant_annotated_result_view, coverage_issues); }
size_t mant_annotated_offsetof_result_view_selection_parts(void)
{ return offsetof(struct mant_annotated_result_view, selection_parts); }
size_t mant_annotated_offsetof_result_view_join_text(void)
{ return offsetof(struct mant_annotated_result_view, join_text); }
