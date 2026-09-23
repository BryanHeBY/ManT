/* Checked borrowed view and destruction for one annotated native result. */
#include "mant_mandoc_annotated_internal.h"
#include "mant_mandoc_structured_session.h"

#include <stdlib.h>
#include <string.h>

static int
count_direct_part(struct structured_session *session,
    struct mant_annotated_result *result, uint32_t key, uint32_t kind,
    uint64_t *edges, uint64_t maximum)
{
	struct mant_annotated_mark *mark;

	if (key == 0)
		return 1;
	if (key > result->mark_count || result->marks == NULL) {
		mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
		    MANT_STRUCTURED_STAGE_CHECK, 0, key, result->mark_count);
		return 0;
	}
	mark = result->marks + key - 1;
	if ((kind == MANT_ANNOTATED_MARK_LINK &&
	    mark->kind != MANT_ANNOTATED_MARK_LINK) ||
	    (kind == 0 && mark->kind != MANT_ANNOTATED_MARK_OWNER &&
	    mark->kind != MANT_ANNOTATED_MARK_HEADING &&
	    mark->kind != MANT_ANNOTATED_MARK_REGION)) {
		mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
		    MANT_STRUCTURED_STAGE_CHECK, 0, key, kind);
		return 0;
	}
	if (!mant_structured_charge(session, edges, 1, maximum, 33,
	    MANT_STRUCTURED_STAGE_CHECK))
		return 0;
	mark->selection_count++;
	return 1;
}

static void
write_direct_part(struct mant_annotated_result *result, uint32_t key,
    const struct mant_annotated_display_run *run)
{
	struct mant_annotated_mark *mark;
	struct mant_annotated_selection_part *part;

	if (key == 0)
		return;
	mark = result->marks + key - 1;
	part = result->selection_parts + mark->selection_first +
	    mark->selection_count;
	part->run = run->key;
	part->join_before = mark->selection_count == 0 ?
	    MANT_ANNOTATED_JOIN_NONE : MANT_ANNOTATED_JOIN_UNKNOWN;
	part->start_byte = 0;
	part->end_byte = run->byte_count;
	mark->selection_count++;
}

/* Scan final, coalesced runs only.  Native buffer slots and overwritten
 * display components have already been retired, so no hidden glyph acquires
 * a public slice.  The result owns one arena partitioned by direct mark;
 * structural ancestors can later refer to children without copying bytes. */
int
mant_annotated_build_selection_parts(struct structured_session *session,
    struct mant_annotated_result *result,
    const struct mant_annotated_display_view *display)
{
	const struct mant_annotated_display_run *run;
	uint64_t edges = 0, maximum, work;
	uint32_t index, first = 0;

	if (session == NULL || result == NULL || display == NULL ||
	    session->status != MANT_STRUCTURED_OK ||
	    result->selection_parts != NULL || result->selection_part_count != 0 ||
	    (result->mark_count != 0 && result->marks == NULL) ||
	    (display->run_count != 0 && display->runs == NULL)) {
		if (session != NULL)
			mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
			    MANT_STRUCTURED_STAGE_CHECK, 0, 0, 0);
		return 0;
	}
	work = (uint64_t)display->run_count * 2 + result->mark_count;
	if (!mant_structured_charge(session, &session->builder_operations, work,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_CHECK))
		return 0;
	maximum = session->limits->max_transfer_edges < UINT32_MAX ?
	    session->limits->max_transfer_edges : UINT32_MAX;
	for (index = 0; index < display->run_count; index++) {
		run = display->runs + index;
		if (run->key != index + 1 || run->byte_count == 0 ||
		    !count_direct_part(session, result, run->label.owner,
		    0, &edges, maximum) ||
		    !count_direct_part(session, result, run->label.link,
		    MANT_ANNOTATED_MARK_LINK, &edges, maximum))
			return 0;
	}
	for (index = 0; index < result->mark_count; index++) {
		struct mant_annotated_mark *mark = result->marks + index;
		uint32_t count = mark->selection_count;

		mark->selection_first = first;
		first += count;
		mark->selection_count = 0;
	}
	result->selection_part_count = first;
	if (first != 0) {
		result->selection_parts = mant_structured_allocate(session,
		    (uint64_t)first * sizeof(*result->selection_parts), 1,
		    MANT_STRUCTURED_STAGE_CHECK);
		if (result->selection_parts == NULL)
			return 0;
	}
	for (index = 0; index < display->run_count; index++) {
		run = display->runs + index;
		write_direct_part(result, run->label.owner, run);
		write_direct_part(result, run->label.link, run);
	}
	return 1;
}

static int
zero_bytes(const uint8_t *data, size_t length)
{
	size_t i;

	for (i = 0; i < length; i++)
		if (data[i] != 0)
			return 0;
	return 1;
}

static int
valid_string(struct mant_bytes_view string)
{
	return mant_structured_valid_bytes(string) &&
	    mant_structured_valid_utf8(string.ptr, string.len);
}

static int
valid_common(const struct mant_structured_result *common)
{
	const struct mant_structured_metadata_view *metadata;
	const struct mant_structured_source_view *source;
	const struct mant_structured_span_view *span;
	const struct mant_structured_provenance_view *provenance;
	const struct mant_structured_diagnostic_view *diagnostic;
	struct mant_bytes_view strings[8];
	static const uint32_t flags[8] = {
		MANT_METADATA_TITLE_PRESENT, MANT_METADATA_SECTION_PRESENT,
		MANT_METADATA_VOLUME_PRESENT, MANT_METADATA_OS_PRESENT,
		MANT_METADATA_ARCH_PRESENT, MANT_METADATA_NAME_PRESENT,
		MANT_METADATA_DATE_PRESENT, MANT_METADATA_ALIAS_PRESENT
	};
	uint32_t i;

	if (common == NULL || common->magic != MANT_STRUCTURED_MAGIC ||
	    common->root_source != 1 || common->source_count == 0 ||
	    common->sources == NULL || common->source_maps == NULL ||
	    common->source_map_count < common->source_count ||
	    (common->profile != MANT_PROFILE_UTF8 &&
	    common->profile != MANT_PROFILE_ASCII) || common->width == 0 ||
	    (common->span_count != 0) != (common->spans != NULL) ||
	    (common->provenance_count != 0) != (common->provenances != NULL) ||
	    (common->diagnostic_count != 0) != (common->diagnostics != NULL))
		return 0;
	/* The common shell owns no second body or stale structured relations. */
	if (common->owner_count != 0 || common->content_root_count != 0 ||
	    common->content_atom_count != 0 || common->block_count != 0 ||
	    common->list_count != 0 || common->item_count != 0 ||
	    common->table_count != 0 || common->fixed_view_count != 0 ||
	    common->placement_count != 0 || common->link_count != 0 ||
	    common->anchor_count != 0)
		return 0;
	metadata = &common->metadata;
	if ((metadata->macroset != MANT_FORMAT_MAN &&
	    metadata->macroset != MANT_FORMAT_MDOC) ||
	    (metadata->presence_flags & ~UINT32_C(0xff)) != 0 ||
	    metadata->has_body > 1 || metadata->reserved != 0 ||
	    !zero_bytes(metadata->reserved_bytes,
	    sizeof(metadata->reserved_bytes)))
		return 0;
	strings[0] = metadata->title;
	strings[1] = metadata->section;
	strings[2] = metadata->volume;
	strings[3] = metadata->operating_system;
	strings[4] = metadata->architecture;
	strings[5] = metadata->name;
	strings[6] = metadata->date;
	strings[7] = metadata->alias_target;
	for (i = 0; i < 8; i++)
		if (!valid_string(strings[i]) ||
		    ((metadata->presence_flags & flags[i]) == 0 &&
		    (strings[i].ptr != NULL || strings[i].len != 0)))
			return 0;
	for (i = 0; i < common->source_count; i++) {
		source = common->sources + i;
		if (source->key != i + 1 || source->reserved != 0 ||
		    source->identity_kind < MANT_IDENTITY_PATH ||
		    source->identity_kind > MANT_IDENTITY_ANONYMOUS ||
		    (source->format != MANT_FORMAT_MAN &&
		    source->format != MANT_FORMAT_MDOC) ||
		    source->coordinate_kind !=
		    MANT_COORD_NATIVE_NORMALIZED_BYTES ||
		    !mant_structured_valid_identity_name(source->identity_kind,
		    source->logical_name) || source->hash_present > 1 ||
		    !zero_bytes(source->reserved_bytes,
		    sizeof(source->reserved_bytes)) ||
		    (source->hash_present == 0 &&
		    !zero_bytes(source->hash, sizeof(source->hash))))
			return 0;
	}
	for (i = 0; i < common->span_count; i++) {
		span = common->spans + i;
		if (span->reserved != 0 || span->source == 0 ||
		    span->source > common->source_count ||
		    span->line_column_present > 1 ||
		    span->byte_range_present != 0 ||
		    span->byte_start != 0 || span->byte_end != 0 ||
		    !zero_bytes(span->reserved_bytes,
		    sizeof(span->reserved_bytes)))
			return 0;
		if (span->line_column_present != 0 &&
		    (span->line_start == 0 || span->column_start == 0 ||
		    ((span->line_end == 0) != (span->column_end == 0)) ||
		    (span->line_end != 0 &&
		    (span->line_end < span->line_start ||
		    (span->line_end == span->line_start &&
		    span->column_end < span->column_start))) ||
		    !mant_structured_source_position_in_maps(common->source_maps,
		    common->source_map_count, span->source, span->line_start,
		    span->column_start - 1) ||
		    (span->line_end != 0 &&
		    !mant_structured_source_position_in_maps(common->source_maps,
		    common->source_map_count, span->source, span->line_end,
		    span->column_end - 1))))
			return 0;
		if (span->line_column_present == 0 &&
		    (span->line_start != 0 || span->column_start != 0 ||
		    span->line_end != 0 || span->column_end != 0))
			return 0;
	}
	for (i = 0; i < common->provenance_count; i++) {
		provenance = common->provenances + i;
		if (provenance->reserved != 0)
			return 0;
		switch (provenance->kind) {
		case MANT_PROVENANCE_AUTHORED:
			if (provenance->authored_span == 0 ||
			    provenance->authored_span > common->span_count ||
			    provenance->generated_trigger_span != 0)
				return 0;
			break;
		case MANT_PROVENANCE_GENERATED:
			if (provenance->authored_span != 0 ||
			    provenance->generated_trigger_span > common->span_count)
				return 0;
			break;
		case MANT_PROVENANCE_UNKNOWN:
			if (provenance->authored_span != 0 ||
			    provenance->generated_trigger_span != 0)
				return 0;
			break;
		default:
			return 0;
		}
	}
	for (i = 0; i < common->diagnostic_count; i++) {
		diagnostic = common->diagnostics + i;
		if (diagnostic->reserved != 0 || diagnostic->owner != 0 ||
		    diagnostic->level < MANT_DIAGNOSTIC_STYLE ||
		    diagnostic->level > MANT_DIAGNOSTIC_UNSUPPORTED ||
		    diagnostic->code < MANT_DIAGNOSTIC_CODE_NATIVE_FIRST ||
		    diagnostic->code > MANT_DIAGNOSTIC_CODE_NATIVE_LAST ||
		    diagnostic->span > common->span_count ||
		    !valid_string(diagnostic->message))
			return 0;
	}
	return 1;
}

static int
valid_marks(const struct mant_annotated_result *result)
{
	const struct mant_annotated_mark *mark;
	uint32_t i;

	if ((result->mark_count != 0) != (result->marks != NULL))
		return 0;
	for (i = 0; i < result->mark_count; i++) {
		mark = result->marks + i;
		if (mark->key != i + 1 ||
		    mark->kind < MANT_ANNOTATED_MARK_HEADING ||
		    mark->kind > MANT_ANNOTATED_MARK_REGION ||
		    mark->parent >= mark->key ||
		    mark->owner >= mark->key ||
		    mark->source > result->common->source_count ||
		    ((mark->line == 0) != (mark->column == 0)) ||
		    (mark->source == 0 && mark->line != 0) ||
		    mark->region_kind > MANT_ANNOTATED_REGION_TABLE_CELL ||
		    mark->title_region > result->mark_count ||
		    mark->body_region > result->mark_count ||
		    (mark->flags & ~(MANT_ANNOTATED_MARK_AUTHORED |
		    MANT_ANNOTATED_MARK_FINAL_POINT_UNVERIFIED)) != 0 ||
		    mark->reserved != 0)
			return 0;
		if (mark->kind == MANT_ANNOTATED_MARK_REGION &&
		    mark->region_kind == MANT_ANNOTATED_REGION_TABLE_CELL) {
			if (mark->table_position_present != 1 ||
			    mark->parent == 0 || mark->owner != mark->parent ||
			    result->marks[mark->parent - 1].kind !=
			    MANT_ANNOTATED_MARK_REGION ||
			    result->marks[mark->parent - 1].region_kind !=
			    MANT_ANNOTATED_REGION_TABLE_SPAN ||
			    mark->line != 0 || mark->column != 0 ||
			    (mark->flags & MANT_ANNOTATED_MARK_AUTHORED) != 0)
				return 0;
		} else if (mark->table_column != 0 ||
		    mark->table_position_present != 0 ||
		    mark->table_offset != 0)
			return 0;
		if (mark->kind == MANT_ANNOTATED_MARK_ANCHOR) {
			if (mark->name == NULL || mark->name_length == 0 ||
			    !mant_structured_valid_utf8(mark->name,
			    mark->name_length))
				return 0;
		} else if (mark->name != NULL || mark->name_length != 0)
			return 0;
		if (mark->kind == MANT_ANNOTATED_MARK_LINK) {
			if (mark->target_kind > MANT_LINK_SECTION ||
			    mark->target_b_present > 1 ||
			    !valid_string(mark->target_a) ||
			    !valid_string(mark->target_b) ||
			    (mark->target_kind == 0 &&
			    (mark->target_a.len != 0 ||
			    mark->target_b_present != 0)) ||
			    (mark->target_kind >= MANT_LINK_DOCUMENT &&
			    mark->target_a.len == 0) ||
			    (mark->target_b_present !=
			    (mark->target_kind == MANT_LINK_MANUAL)) ||
			    (mark->target_b_present != 0 &&
			    mark->target_b.len == 0) ||
			    (mark->target_b_present == 0 &&
			    mark->target_b.len != 0))
				return 0;
		} else if (mark->target_kind != 0 ||
		    mark->target_b_present != 0 || mark->target_a.ptr != NULL ||
		    mark->target_a.len != 0 || mark->target_b.ptr != NULL ||
		    mark->target_b.len != 0)
			return 0;
		if (mark->line != 0 &&
		    !mant_structured_source_position_in_maps(
		    result->common->source_maps,
		    result->common->source_map_count, mark->source,
		    mark->line, mark->column - 1))
			return 0;
	}
	return 1;
}

static int
valid_display(const struct mant_annotated_result *result)
{
	struct mant_annotated_display_view display;
	const struct mant_annotated_display_row *row;
	const struct mant_annotated_display_run *run;
	uint64_t next_byte = 0;
	uint32_t next_run = 0, i, j;

	if (!mant_annotated_display_finish(result->display, &display) ||
	    (display.byte_count != 0) != (display.bytes != NULL) ||
	    (display.row_count != 0) != (display.rows != NULL) ||
	    (display.run_count != 0) != (display.runs != NULL))
		return 0;
	for (i = 0; i < display.row_count; i++) {
		row = display.rows + i;
		if (row->key != i + 1 || row->first_run != next_run ||
		    row->run_count > display.run_count - next_run ||
		    row->break_after > 1 ||
		    (row->break_after == 0 && i + 1 != display.row_count))
			return 0;
		for (j = 0; j < row->run_count; j++) {
			run = display.runs + next_run++;
			if (run->key != next_run || run->reserved != 0 ||
			    run->byte_count == 0 ||
			    run->byte_start != next_byte ||
			    run->byte_count > display.byte_count - next_byte ||
			    run->column > row->column_count ||
			    run->width > row->column_count - run->column ||
			    run->label.glyph_origin != 0 ||
			    run->label.flags != 0 || run->label.reserved != 0 ||
			    run->label.source > result->common->source_count ||
			    (run->label.owner != 0 &&
			    (run->label.owner > result->mark_count ||
		    (result->marks[run->label.owner - 1].kind !=
		    MANT_ANNOTATED_MARK_OWNER &&
		    result->marks[run->label.owner - 1].kind !=
		    MANT_ANNOTATED_MARK_HEADING &&
		    result->marks[run->label.owner - 1].kind !=
		    MANT_ANNOTATED_MARK_REGION))) ||
			    (run->label.link != 0 &&
			    (run->label.link > result->mark_count ||
			    result->marks[run->label.link - 1].kind !=
			    MANT_ANNOTATED_MARK_LINK)) ||
			    (run->label.role != MANT_ANNOTATED_BODY &&
			    run->label.role != MANT_ANNOTATED_DIRECT_DRAW) ||
			    !mant_structured_valid_utf8(display.bytes + next_byte,
			    run->byte_count))
				return 0;
			next_byte += run->byte_count;
		}
	}
	return next_run == display.run_count && next_byte == display.byte_count;
}

static int
valid_selection_parts(const struct mant_annotated_result *result)
{
	struct mant_annotated_display_view display;
	const struct mant_annotated_selection_part *part;
	const struct mant_annotated_display_run *run;
	const struct mant_annotated_mark *mark;
	uint64_t expected = 0;
	uint32_t first = 0, previous, i, j;

	if (!mant_annotated_display_finish(result->display, &display) ||
	    (result->selection_part_count != 0) !=
	    (result->selection_parts != NULL))
		return 0;
	for (i = 0; i < display.run_count; i++) {
		run = display.runs + i;
		expected += (run->label.owner != 0) +
		    (run->label.link != 0);
	}
	if (expected != result->selection_part_count)
		return 0;
	for (i = 0; i < result->mark_count; i++) {
		mark = result->marks + i;
		if (mark->selection_first != first ||
		    mark->selection_count > result->selection_part_count - first)
			return 0;
		previous = 0;
		for (j = 0; j < mark->selection_count; j++) {
			part = result->selection_parts + first + j;
			if (part->run <= previous ||
			    part->run > display.run_count ||
			    part->start_byte != 0 ||
			    part->join_before != (j == 0 ?
			    MANT_ANNOTATED_JOIN_NONE :
			    MANT_ANNOTATED_JOIN_UNKNOWN))
				return 0;
			run = display.runs + part->run - 1;
			if (part->end_byte != run->byte_count ||
			    (mark->kind == MANT_ANNOTATED_MARK_LINK ?
			    run->label.link : run->label.owner) != mark->key ||
			    (mark->kind != MANT_ANNOTATED_MARK_LINK &&
			    mark->kind != MANT_ANNOTATED_MARK_HEADING &&
			    mark->kind != MANT_ANNOTATED_MARK_OWNER &&
			    mark->kind != MANT_ANNOTATED_MARK_REGION))
				return 0;
			previous = part->run;
		}
		first += mark->selection_count;
	}
	/* Strict per-mark ordering prevents duplicates; the total equals all
	 * direct run labels, so no marked final run can be omitted. */
	return first == result->selection_part_count;
}

int
mant_annotated_result_is_valid(const struct mant_annotated_result *result)
{
	return result != NULL && result->magic == MANT_ANNOTATED_MAGIC &&
	    valid_common(result->common) && valid_marks(result) &&
	    mant_annotated_coverage_is_valid(result) && valid_display(result) &&
	    valid_selection_parts(result);
}

uint32_t
mant_annotated_abi_version(void)
{
	return 5;
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
	view->coverage_checks = VIEW_SLICE(result->coverage_checks, 24,
	    struct mant_annotated_coverage_check);
	view->coverage_issues = VIEW_SLICE(result->coverage_issues,
	    result->coverage_issue_count,
	    struct mant_annotated_coverage_issue);
	view->selection_parts = VIEW_SLICE(result->selection_parts,
	    result->selection_part_count,
	    struct mant_annotated_selection_part);
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
size_t mant_annotated_sizeof_selection_part(void)
{ return sizeof(struct mant_annotated_selection_part); }
size_t mant_annotated_alignof_selection_part(void)
{ return _Alignof(struct mant_annotated_selection_part); }
size_t mant_annotated_offsetof_selection_part_end_byte(void)
{ return offsetof(struct mant_annotated_selection_part, end_byte); }
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
