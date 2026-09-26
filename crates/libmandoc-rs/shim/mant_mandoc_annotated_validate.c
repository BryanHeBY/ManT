/* Validate one annotated native result before exposing a borrowed view. */
#include "mant_mandoc_annotated_internal.h"
#include "roff.h"

/* Keep the private Rust transfer/codec discriminators pinned to roff.h. */
_Static_assert(MAN_TP == 382, "annotated MAN_TP token changed");
_Static_assert(MAN_TQ == 383, "annotated MAN_TQ token changed");
_Static_assert(MAN_IP == 387, "annotated MAN_IP token changed");
_Static_assert(MAN_RS == 401, "annotated MAN_RS token changed");
_Static_assert(MAN_B == 396, "annotated MAN_B token changed");
_Static_assert(MDOC_Dv == 276, "annotated MDOC_Dv token changed");
_Static_assert(MDOC_Va == 295, "annotated MDOC_Va token changed");

static int
man_reading_family(uint32_t token)
{
	if (token == MAN_IP)
		return 1;
	return token == MAN_TP || token == MAN_TQ ? 2 : 0;
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
		    source->format != metadata->macroset ||
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
	struct mant_annotated_display_view display;
	uint32_t i;

	if ((result->mark_count != 0) != (result->marks != NULL))
		return 0;
	if (!mant_annotated_display_finish(result->display, &display))
		return 0;
	/* valid_marks() precedes valid_display(): reject an inconsistent
	 * finished view before resolving a RowColumn against its row array. */
	if ((display.row_count != 0) != (display.rows != NULL))
		return 0;
	for (i = 0; i < result->mark_count; i++) {
		mark = result->marks + i;
		if (mark->key != i + 1 ||
		    mark->kind < MANT_ANNOTATED_MARK_HEADING ||
		    mark->kind > MANT_ANNOTATED_MARK_HEAD_COMPONENT ||
		    mark->parent >= mark->key ||
		    mark->owner >= mark->key ||
		    mark->source > result->common->source_count ||
		    ((mark->line == 0) != (mark->column == 0)) ||
		    (mark->source == 0 && mark->line != 0) ||
		    (((mark->flags & MANT_ANNOTATED_MARK_AUTHORED) != 0) !=
		    (mark->line != 0)) ||
		    mark->region_kind >
		    MANT_ANNOTATED_REGION_HANGING_CONTINUATION ||
		    mark->title_region > result->mark_count ||
		    mark->body_region > result->mark_count ||
		    (mark->flags & ~(MANT_ANNOTATED_MARK_AUTHORED |
		    MANT_ANNOTATED_MARK_MANUAL_TARGET |
		    MANT_ANNOTATED_MARK_SUBSECTION |
		    MANT_ANNOTATED_MARK_DEFINITION |
		    MANT_ANNOTATED_MARK_HANGING_CANDIDATE |
		    MANT_ANNOTATED_MARK_DIRECT_TP_TEXT |
		    MANT_ANNOTATED_MARK_HEAD_ROLE_MASK)) != 0 ||
		    ((mark->flags & MANT_ANNOTATED_MARK_HANGING_CANDIDATE) != 0 &&
		    mark->kind != MANT_ANNOTATED_MARK_OWNER) ||
		    ((mark->flags & MANT_ANNOTATED_MARK_MANUAL_TARGET) != 0 &&
		    mark->kind != MANT_ANNOTATED_MARK_ANCHOR) ||
		    ((mark->flags & MANT_ANNOTATED_MARK_SUBSECTION) != 0 &&
		    mark->kind != MANT_ANNOTATED_MARK_HEADING) ||
		    ((mark->flags & MANT_ANNOTATED_MARK_DEFINITION) != 0 &&
		    mark->kind != MANT_ANNOTATED_MARK_OWNER) ||
		    ((mark->flags & MANT_ANNOTATED_MARK_DIRECT_TP_TEXT) != 0 &&
		    (mark->kind != MANT_ANNOTATED_MARK_OWNER ||
		    (mark->token != MAN_TP && mark->token != MAN_TQ) ||
		    (mark->flags & (MANT_ANNOTATED_MARK_DEFINITION |
		    MANT_ANNOTATED_MARK_HEAD_LEXICAL)) !=
		    (MANT_ANNOTATED_MARK_DEFINITION |
		    MANT_ANNOTATED_MARK_HEAD_LEXICAL))) ||
		    ((mark->flags & MANT_ANNOTATED_MARK_HEAD_ROLE_MASK) != 0 &&
		    ((mark->kind != MANT_ANNOTATED_MARK_OWNER &&
		    mark->kind != MANT_ANNOTATED_MARK_HEAD_COMPONENT) ||
		    (mark->kind == MANT_ANNOTATED_MARK_OWNER &&
		    (mark->flags & MANT_ANNOTATED_MARK_DEFINITION) == 0) ||
		    ((mark->flags & MANT_ANNOTATED_MARK_HEAD_ROLE_MASK) &
		    ((mark->flags & MANT_ANNOTATED_MARK_HEAD_ROLE_MASK) - 1)) != 0)) ||
		    mark->point_reserved != 0)
			return 0;
		if (mark->kind == MANT_ANNOTATED_MARK_OWNER &&
		    (mark->flags & MANT_ANNOTATED_MARK_HANGING_CANDIDATE) != 0 &&
		    (mark->parent == 0 || mark->preceding_owner != 0 ||
		    mark->title_region == 0 || mark->body_region == 0 ||
		    result->marks[mark->parent - 1].kind !=
		    MANT_ANNOTATED_MARK_REGION ||
		    result->marks[mark->parent - 1].region_kind !=
		    MANT_ANNOTATED_REGION_HEADING_BODY ||
		    (((mark->flags & MANT_ANNOTATED_MARK_DEFINITION) != 0) !=
		    ((mark->flags & MANT_ANNOTATED_MARK_HEAD_LEXICAL) != 0)) ||
		    (mark->flags & (MANT_ANNOTATED_MARK_HEAD_OPTION |
		    MANT_ANNOTATED_MARK_HEAD_ENVIRONMENT |
		    MANT_ANNOTATED_MARK_HEAD_LITERAL |
		    MANT_ANNOTATED_MARK_HEAD_VARIABLE |
		    MANT_ANNOTATED_MARK_HEAD_DEFINED_VARIABLE)) != 0))
			return 0;
		if (mark->kind == MANT_ANNOTATED_MARK_REGION &&
		    mark->region_kind ==
		    MANT_ANNOTATED_REGION_HANGING_CONTINUATION) {
			const struct mant_annotated_mark *candidate;

			if (mark->preceding_owner == 0 ||
			    mark->preceding_owner >= mark->key ||
			    mark->parent == 0 || mark->owner != 0 ||
			    mark->token != MAN_RS ||
			    mark->title_region != 0 || mark->body_region != 0 ||
			    result->marks[mark->parent - 1].kind !=
			    MANT_ANNOTATED_MARK_REGION ||
			    result->marks[mark->parent - 1].region_kind !=
			    MANT_ANNOTATED_REGION_HEADING_BODY)
				return 0;
			candidate = result->marks + mark->preceding_owner - 1;
			if (candidate->kind != MANT_ANNOTATED_MARK_OWNER ||
			    candidate->parent != mark->parent ||
			    (candidate->flags &
			    (MANT_ANNOTATED_MARK_HANGING_CANDIDATE |
			    MANT_ANNOTATED_MARK_DEFINITION |
			    MANT_ANNOTATED_MARK_HEAD_LEXICAL)) !=
			    (MANT_ANNOTATED_MARK_HANGING_CANDIDATE |
			    MANT_ANNOTATED_MARK_DEFINITION |
			    MANT_ANNOTATED_MARK_HEAD_LEXICAL) ||
			    candidate->title_region == 0 ||
			    candidate->body_region == 0)
				return 0;
		} else if (mark->preceding_owner != 0) {
			const struct mant_annotated_mark *preceding;
			int family = man_reading_family(mark->token);

			if (mark->kind != MANT_ANNOTATED_MARK_OWNER ||
			    family == 0 ||
			    (mark->flags & (MANT_ANNOTATED_MARK_DEFINITION |
			    MANT_ANNOTATED_MARK_HEAD_LEXICAL)) !=
			    (MANT_ANNOTATED_MARK_DEFINITION |
			    MANT_ANNOTATED_MARK_HEAD_LEXICAL) ||
			    mark->preceding_owner >= mark->key)
				return 0;
			preceding = result->marks + mark->preceding_owner - 1;
			if (preceding->kind != MANT_ANNOTATED_MARK_OWNER ||
			    man_reading_family(preceding->token) != family ||
			    (preceding->flags & (MANT_ANNOTATED_MARK_DEFINITION |
			    MANT_ANNOTATED_MARK_HEAD_LEXICAL)) !=
			    (MANT_ANNOTATED_MARK_DEFINITION |
			    MANT_ANNOTATED_MARK_HEAD_LEXICAL) ||
			    preceding->parent != mark->parent ||
			    preceding->owner != mark->owner)
				return 0;
		}
		if (mark->kind == MANT_ANNOTATED_MARK_HEAD_COMPONENT &&
		    (mark->parent == 0 || mark->owner != mark->parent ||
		    mark->source == 0 ||
		    (mark->flags & MANT_ANNOTATED_MARK_HEAD_ROLE_MASK) == 0 ||
		    ((mark->flags & MANT_ANNOTATED_MARK_HEAD_VARIABLE) != 0 &&
		    mark->token != MDOC_Va) ||
		    ((mark->flags & MANT_ANNOTATED_MARK_HEAD_DEFINED_VARIABLE) != 0 &&
		    mark->token != MDOC_Dv) ||
		    result->marks[mark->parent - 1].kind !=
		    MANT_ANNOTATED_MARK_REGION ||
		    result->marks[mark->parent - 1].region_kind !=
		    MANT_ANNOTATED_REGION_OWNER_TERM ||
		    mark->title_region != 0 || mark->body_region != 0 ||
		    mark->region_kind != 0))
			return 0;
		switch (mark->point_kind) {
		case MANT_ANNOTATED_POINT_NONE:
			if (mark->point_row != 0 || mark->point_column != 0 ||
			    mark->kind == MANT_ANNOTATED_MARK_ANCHOR ||
			    mark->kind == MANT_ANNOTATED_MARK_REGION)
				return 0;
			break;
		case MANT_ANNOTATED_POINT_ROW_COLUMN:
			if ((mark->kind != MANT_ANNOTATED_MARK_ANCHOR &&
			    mark->kind != MANT_ANNOTATED_MARK_OWNER &&
			    mark->kind != MANT_ANNOTATED_MARK_REGION) ||
			    mark->point_row == 0 ||
			    mark->point_row > display.row_count ||
			    mark->point_column >
			    display.rows[mark->point_row - 1].column_count)
				return 0;
			break;
		case MANT_ANNOTATED_POINT_DOCUMENT_END:
			if ((mark->kind != MANT_ANNOTATED_MARK_ANCHOR &&
			    mark->kind != MANT_ANNOTATED_MARK_OWNER &&
			    mark->kind != MANT_ANNOTATED_MARK_REGION) ||
			    mark->point_row != display.row_count ||
			    mark->point_column != 0)
				return 0;
			break;
		default:
			return 0;
		}
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
		if (mark->kind == MANT_ANNOTATED_MARK_REGION &&
		    mark->region_kind == MANT_ANNOTATED_REGION_MARGIN &&
		    (mark->parent == 0 || mark->owner != mark->parent ||
		    result->marks[mark->parent - 1].kind !=
		    MANT_ANNOTATED_MARK_REGION || mark->source != 0 ||
		    mark->line != 0 || mark->column != 0 ||
		    mark->token != 0 || mark->flags != 0 ||
		    mark->title_region != 0 || mark->body_region != 0))
			return 0;
		if (mark->kind == MANT_ANNOTATED_MARK_ANCHOR) {
			if (mark->name == NULL || mark->name_length == 0 ||
			    !mant_structured_valid_utf8(mark->name,
			    mark->name_length))
				return 0;
		} else if (mark->kind == MANT_ANNOTATED_MARK_HEADING) {
			if ((mark->name == NULL) !=
			    (mark->name_length == 0) ||
			    (mark->name != NULL &&
			    !mant_structured_valid_utf8(mark->name,
			    mark->name_length)))
				return 0;
		} else if (mark->kind == MANT_ANNOTATED_MARK_OWNER &&
		    (mark->flags & (MANT_ANNOTATED_MARK_HEAD_OPTION |
		    MANT_ANNOTATED_MARK_HEAD_ENVIRONMENT |
		    MANT_ANNOTATED_MARK_HEAD_LEXICAL)) != 0) {
			if ((mark->name == NULL) !=
			    (mark->name_length == 0) ||
			    (mark->name != NULL &&
			    !mant_structured_valid_utf8(mark->name,
			    mark->name_length)))
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
all_layout_spaces(const uint8_t *bytes, uint64_t length)
{
	uint64_t index;

	for (index = 0; index < length; index++)
		if (bytes[index] != ' ')
			return 0;
	return 1;
}

static int
valid_display_surface(const struct mant_annotated_result *result)
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
			    (run->label.style & ~(MANT_ANNOTATED_STYLE_BOLD |
			    MANT_ANNOTATED_STYLE_UNDERLINE)) != 0 ||
			    run->label.source > result->common->source_count ||
			    (run->label.role != MANT_ANNOTATED_BODY &&
			    run->label.role != MANT_ANNOTATED_DIRECT_DRAW &&
			    run->label.role != MANT_ANNOTATED_LAYOUT) ||
			    (run->label.role == MANT_ANNOTATED_LAYOUT &&
			    (run->label.owner != 0 || run->label.link != 0 ||
			    run->label.head_component != 0 ||
			    run->label.source != 0 || run->label.style != 0 ||
			    !all_layout_spaces(display.bytes + next_byte,
			    run->byte_count))) ||
			    !mant_structured_valid_utf8(display.bytes + next_byte,
			    run->byte_count))
				return 0;
			next_byte += run->byte_count;
		}
	}
	return next_run == display.run_count && next_byte == display.byte_count;
}

static int
valid_display_annotations(const struct mant_annotated_result *result)
{
	struct mant_annotated_display_view display;
	const struct mant_annotated_display_run *run;
	uint32_t i;

	if (!mant_annotated_display_finish(result->display, &display) ||
	    (result->mark_count != 0 && result->marks == NULL))
		return 0;
	for (i = 0; i < display.run_count; i++) {
		run = display.runs + i;
		if ((run->label.owner != 0 &&
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
		    (run->label.head_component != 0 &&
		    (run->label.head_component > result->mark_count ||
		    result->marks[run->label.head_component - 1].kind !=
		    MANT_ANNOTATED_MARK_HEAD_COMPONENT ||
		    result->marks[run->label.head_component - 1].owner !=
		    run->label.owner)))
			return 0;
	}
	return 1;
}

static int
non_layout_between(const struct mant_annotated_display_view *display,
    uint32_t previous, uint32_t current)
{
	uint32_t key;

	/* Each final run can be scanned once per direct owner, link and HEAD
	 * component because every channel partitions its own final runs. */
	for (key = previous + 1; key < current; key++)
		if (display->runs[key - 1].label.role != MANT_ANNOTATED_LAYOUT)
			return 1;
	return 0;
}

static int
valid_selection_parts(const struct mant_annotated_result *result)
{
	struct mant_annotated_display_view display;
	const struct mant_annotated_selection_part *part;
	const struct mant_annotated_display_run *run;
	const struct mant_annotated_run_endpoint *endpoints;
	const struct mant_annotated_mark *mark;
	uint64_t expected = 0, payload = 0, spaces;
	uint32_t first = 0, previous, i, j, endpoint_count;

	if (!mant_annotated_display_finish(result->display, &display) ||
	    (result->selection_part_count != 0) !=
	    (result->selection_parts != NULL) ||
	    (result->join_text_count != 0) != (result->join_text != NULL) ||
	    result->join_text_count > UINT32_MAX)
		return 0;
	endpoints = mant_annotated_display_endpoints(result->display,
	    &endpoint_count);
	if (endpoint_count != display.run_count ||
	    (endpoint_count != 0 && endpoints == NULL))
		return 0;
	for (i = 0; i < display.run_count; i++) {
		run = display.runs + i;
		expected += (run->label.owner != 0) +
		    (run->label.link != 0) +
		    (run->label.head_component != 0);
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
			    part->start_byte != 0)
				return 0;
			spaces = 0;
			if (part->join_before != (j == 0 ?
			    MANT_ANNOTATED_JOIN_NONE : mant_annotated_native_join(endpoints,
			    previous, part->run,
			    mark->kind == MANT_ANNOTATED_MARK_HEAD_COMPONENT ?
			    mark->owner : mark->key, mark->kind,
			    non_layout_between(&display, previous, part->run),
			    &spaces)) ||
			    part->join_text_len != spaces)
				return 0;
			if (spaces == 0) {
				if (part->join_text_start != 0)
					return 0;
			} else {
				uint64_t byte;
				if (part->join_text_start != payload ||
				    spaces > result->join_text_count - payload)
					return 0;
				for (byte = 0; byte < spaces; byte++)
					if (result->join_text[payload + byte] != ' ')
						return 0;
				payload += spaces;
			}
			run = display.runs + part->run - 1;
			if (part->end_byte != run->byte_count ||
			    (mark->kind == MANT_ANNOTATED_MARK_LINK ?
			    run->label.link :
			    mark->kind == MANT_ANNOTATED_MARK_HEAD_COMPONENT ?
			    run->label.head_component : run->label.owner) != mark->key ||
			    (mark->kind != MANT_ANNOTATED_MARK_LINK &&
			    mark->kind != MANT_ANNOTATED_MARK_HEAD_COMPONENT &&
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
	return first == result->selection_part_count &&
	    payload == result->join_text_count;
}

int
mant_annotated_result_is_surface_valid(
    const struct mant_annotated_result *result)
{
	return result != NULL && result->magic == MANT_ANNOTATED_MAGIC &&
	    valid_common(result->common) && valid_display_surface(result);
}

int
mant_annotated_result_is_valid(const struct mant_annotated_result *result)
{
	return mant_annotated_result_is_surface_valid(result) &&
	    result->annotation_degraded <= 1 && valid_marks(result) &&
	    valid_display_annotations(result) &&
	    mant_annotated_coverage_is_valid(result) &&
	    valid_selection_parts(result) &&
	    (result->annotation_degraded == 0 ||
	    (result->mark_count == 0 && result->selection_part_count == 0 &&
	    result->join_text_count == 0));
}
