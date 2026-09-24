/* Build direct mark selections from the final coalesced display. */
#include "mant_mandoc_annotated_internal.h"
#include "mant_mandoc_structured_session.h"

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
	if ((kind != 0 && mark->kind != kind) ||
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

uint32_t
mant_annotated_native_join(const struct mant_annotated_run_endpoint *endpoints,
    uint32_t previous_run, uint32_t current_run, uint32_t mark_key,
    uint32_t mark_kind, int non_layout_gap, uint64_t *spaces)
{
	const struct mant_annotated_run_endpoint *prior =
	    endpoints + previous_run - 1;
	const struct mant_annotated_run_endpoint *current =
	    endpoints + current_run - 1;

	*spaces = 0;
	/* The origin edge is a logical relation, not proof that no other
	 * content was displayed between the two final runs.  A nested macro
	 * or another owner can contribute visible output in the gap. */
	if (non_layout_gap)
		return MANT_ANNOTATED_JOIN_UNKNOWN;
	if (prior->last_origin == 0 || current->first_origin == 0 ||
	    current->first_edge.predecessor_origin != prior->last_origin)
		return MANT_ANNOTATED_JOIN_UNKNOWN;
	switch (current->first_edge.join) {
	case MANT_DISPLAY_JOIN_DIRECT:
		return MANT_ANNOTATED_JOIN_DIRECT_CONTACT;
	case MANT_DISPLAY_JOIN_HARD:
		return MANT_ANNOTATED_JOIN_HARD_BOUNDARY;
	case MANT_DISPLAY_JOIN_SEPARATOR:
	case MANT_DISPLAY_JOIN_GENERATED_SEPARATOR:
		if ((mark_kind == MANT_ANNOTATED_MARK_LINK ?
		    current->first_edge.separator_link :
		    current->first_edge.separator_owner) != mark_key)
			return MANT_ANNOTATED_JOIN_UNKNOWN;
		*spaces = current->first_edge.separator_spaces;
		return *spaces == 0 ? MANT_ANNOTATED_JOIN_UNKNOWN :
		    current->first_edge.join == MANT_DISPLAY_JOIN_SEPARATOR ?
		    MANT_ANNOTATED_JOIN_AUTHORED_SEPARATOR :
		    MANT_ANNOTATED_JOIN_GENERATED_SEPARATOR;
	default:
		return MANT_ANNOTATED_JOIN_UNKNOWN;
	}
}

static void
write_direct_part(struct mant_annotated_result *result, uint32_t key,
    const struct mant_annotated_display_run *run,
    const struct mant_annotated_run_endpoint *endpoints,
    uint32_t last_non_layout_run)
{
	struct mant_annotated_mark *mark;
	struct mant_annotated_selection_part *part;

	if (key == 0)
		return;
	mark = result->marks + key - 1;
	part = result->selection_parts + mark->selection_first +
	    mark->selection_count;
	part->run = run->key;
	part->join_before = MANT_ANNOTATED_JOIN_NONE;
	if (mark->selection_count != 0)
		part->join_before = mant_annotated_native_join(endpoints,
		    (part - 1)->run, run->key,
		    mark->kind == MANT_ANNOTATED_MARK_HEAD_COMPONENT ?
		    mark->owner : mark->key, mark->kind,
		    last_non_layout_run > (part - 1)->run,
		    &part->join_text_len);
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
	const struct mant_annotated_run_endpoint *endpoints;
	uint64_t edges = 0, maximum, work, join_bytes = 0;
	uint32_t endpoint_count;
	uint32_t index, first = 0, last_non_layout_run = 0;

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
	endpoints = mant_annotated_display_endpoints(result->display,
	    &endpoint_count);
	if (endpoint_count != display->run_count ||
	    (endpoint_count != 0 && endpoints == NULL)) {
		mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
		    MANT_STRUCTURED_STAGE_CHECK, 0, endpoint_count,
		    display->run_count);
		return 0;
	}
	work = (uint64_t)display->run_count * 3 + result->mark_count;
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
		    MANT_ANNOTATED_MARK_LINK, &edges, maximum) ||
		    !count_direct_part(session, result,
		    run->label.head_component,
		    MANT_ANNOTATED_MARK_HEAD_COMPONENT, &edges, maximum))
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
		write_direct_part(result, run->label.owner, run, endpoints,
		    last_non_layout_run);
		write_direct_part(result, run->label.link, run, endpoints,
		    last_non_layout_run);
		write_direct_part(result, run->label.head_component, run,
		    endpoints, last_non_layout_run);
		if (run->label.role != MANT_ANNOTATED_LAYOUT)
			last_non_layout_run = run->key;
	}
	for (index = 0; index < result->selection_part_count; index++) {
		struct mant_annotated_selection_part *part =
		    result->selection_parts + index;
		if (join_bytes > UINT32_MAX ||
		    part->join_text_len > UINT32_MAX - join_bytes ||
		    part->join_text_len > session->limits->max_content_bytes -
		    join_bytes) {
			mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET,
			    MANT_STRUCTURED_STAGE_CHECK, 10,
			    part->join_text_len > UINT64_MAX - join_bytes ?
			    UINT64_MAX : join_bytes + part->join_text_len,
			    session->limits->max_content_bytes < UINT32_MAX ?
			    session->limits->max_content_bytes : UINT32_MAX);
			return 0;
		}
		if (part->join_text_len != 0)
			part->join_text_start = join_bytes;
		join_bytes += part->join_text_len;
	}
	if (join_bytes != 0) {
		if (!mant_structured_charge(session,
		    &session->builder_operations, join_bytes,
		    session->limits->max_builder_operations, 8,
		    MANT_STRUCTURED_STAGE_CHECK))
			return 0;
		result->join_text = mant_structured_allocate(session,
		    join_bytes, 1, MANT_STRUCTURED_STAGE_CHECK);
		if (result->join_text == NULL)
			return 0;
		memset(result->join_text, ' ', (size_t)join_bytes);
	}
	result->join_text_count = join_bytes;
	return 1;
}
