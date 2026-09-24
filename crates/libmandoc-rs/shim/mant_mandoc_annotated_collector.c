/* Active terminal labels; no historical formatter event stream is retained. */
#include "config.h"

#include <ctype.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "mandoc.h"
#include "roff.h"
#include "mdoc.h"
#include "libmdoc.h"
#include "tbl.h"
#include "out.h"
#include "tag.h"
#include "term.h"

#include "mant_mandoc_annotated_collector.h"
#include "mant_mandoc_output.h"
#include "mant_mandoc_structured_link.h"
#include "mant_mandoc_structured_session.h"

struct annotated_slot {
	uint64_t origin;
	uint32_t owner;
	uint32_t link;
	uint32_t source;
	uint32_t head_component;
	int value;
	uint8_t flags;
	uint8_t occupied;
	uint8_t authored_space;
	uint8_t generated_space;
	uint8_t layout_space;
	uint32_t first_point;
	uint64_t skipped_visual;
};

struct annotated_column {
	struct annotated_slot *slots;
	uint32_t capacity;
	uint32_t live;
	uint64_t last_output_origin;
	uint64_t pending_spaces;
	uint32_t separator_owner;
	uint32_t separator_link;
	struct mant_annotated_display_edge last_origin_edge;
	uint32_t pending_join;
	uint32_t skip_start;
	uint64_t skipped_visual;
	uint32_t skip_cell_width;
	uint8_t skipping;
};

struct annotated_frame {
	const struct roff_node *node;
	const struct roff_node *saved_link_node;
	const struct roff_node *last_direct_ip_node;
	uint32_t saved_owner;
	uint32_t saved_head_component;
	uint32_t saved_link;
	uint32_t saved_heading;
	uint64_t saved_link_epoch;
	uint32_t owner_mark;
	uint32_t anchor_mark;
	uint32_t region_mark;
	uint32_t last_direct_ip_owner;
};

struct annotated_point_state {
	struct mant_annotated_display_checkpoint checkpoint;
	uint32_t next;
	uint8_t state; /* 0 absent, 1 active buffer gap, 2 captured. */
	uint8_t component_started; /* Collector-only logical macro state. */
};

struct annotated_cell {
	const struct tbl_dat *data;
	uint32_t mark;
};

struct mant_annotated_collector {
	struct structured_session *session;
	struct mant_annotated_display *display;
	struct annotated_column *columns;
	uint32_t column_capacity;
	struct annotated_frame *frames;
	uint32_t frame_count;
	uint32_t frame_capacity;
	struct mant_annotated_mark *marks;
	uint32_t mark_count;
	uint32_t mark_capacity;
	struct annotated_point_state *points;
	uint32_t point_capacity;
	uint32_t pending_point_count;
	struct annotated_cell *cells;
	uint32_t cell_capacity;
	uint32_t cell_count;
	const struct roff_node *table_node;
	const struct tbl_dat *active_cell;
	uint32_t cell_saved_owner;
	uint8_t table_prepared;
	uint32_t active_owner;
	uint32_t active_head_component;
	uint32_t active_link;
	const struct roff_node *active_link_node;
	uint64_t active_link_epoch;
	uint64_t phrase_epoch;
	uint32_t active_heading;
	uint32_t last_top_heading;
	uint32_t unsectioned_region;
	/* ROOT has no terminal frame: retain its last completed direct sibling. */
	const struct roff_node *last_root_ip_node;
	uint32_t last_root_ip_owner;
	uint64_t next_origin;
	uint64_t pending_origin;
	uint32_t pending_owner;
	uint32_t pending_head_component;
	uint32_t pending_link;
	uint32_t pending_source;
	uint32_t margin_owner;
	uint32_t margin_mark;
	uint64_t allocated_display_bytes;
	uint64_t accounted_display_work;
	uint64_t live_slots;
	uint64_t slot_bytes;
	struct mant_annotated_collector_metrics metrics;
	struct mant_annotated_display_label letter_label;
	struct mant_annotated_display_edge letter_edge;
	struct mant_annotated_display_label skipped[256];
	uint16_t skipped_cells;
	uint16_t advance_count;
	uint32_t advance_role;
	uint32_t skipped_column;
	uint32_t letter_column;
	uint32_t letter_pos;
	uint8_t letter_pending;
	uint8_t letter_from_field;
	uint8_t in_header;
	uint8_t in_footer;
	uint8_t in_margin;
	uint8_t footer_drained;
	uint8_t html_nofill;
};

static void
fail_relation(struct mant_annotated_collector *collector, uint64_t observed,
    uint64_t allowed)
{
	mant_structured_set_failure(collector->session, MANT_STRUCTURED_RELATION,
	    MANT_STRUCTURED_STAGE_RENDER, 0, observed, allowed);
}

static void
join_unknown(struct annotated_column *column)
{
	column->pending_join = MANT_DISPLAY_JOIN_UNKNOWN;
	column->pending_spaces = 0;
	column->separator_owner = column->separator_link = 0;
}

static void
join_hard(struct annotated_column *column)
{
	column->pending_join = MANT_DISPLAY_JOIN_HARD;
	column->pending_spaces = 0;
	column->separator_owner = column->separator_link = 0;
}

static struct mant_annotated_display_edge
origin_edge(const struct annotated_column *column, uint64_t origin)
{
	struct mant_annotated_display_edge edge = {0};

	if (origin == 0 || column->last_output_origin == 0)
		return edge;
	if (origin == column->last_output_origin &&
	    column->pending_join == MANT_DISPLAY_JOIN_DIRECT) {
		edge = column->last_origin_edge;
		edge.same_origin_continuation = 1;
		return edge;
	}
	edge.predecessor_origin = column->last_output_origin;
	edge.join = column->pending_join;
	edge.separator_spaces = column->pending_spaces;
	edge.separator_owner = column->separator_owner;
	edge.separator_link = column->separator_link;
	return edge;
}

static void
commit_origin(struct annotated_column *column, uint64_t origin,
    struct mant_annotated_display_edge edge)
{
	if (origin == 0) {
		join_unknown(column);
		return;
	}
	if (column->last_output_origin != origin ||
	    column->pending_join != MANT_DISPLAY_JOIN_DIRECT)
		column->last_origin_edge = edge;
	column->last_output_origin = origin;
	column->pending_join = MANT_DISPLAY_JOIN_DIRECT;
	column->pending_spaces = 0;
	column->separator_owner = column->separator_link = 0;
}

static int
join_wrap_space(struct mant_annotated_collector *collector,
    struct annotated_column *column, const struct annotated_slot *slot)
{
	uint32_t join;

	if (column->pending_join == MANT_DISPLAY_JOIN_HARD ||
	    column->pending_join == MANT_DISPLAY_JOIN_UNKNOWN)
		return 1;
	/* Pinned term.c::term_word() emits AUTO_SPACE before an operand.
	 * term_flushln() may consume that actual buffer slot at a soft wrap.
	 * A reused blank has neither proof: BUFFER_CURSOR clears both flags. */
	join = slot->authored_space ? MANT_DISPLAY_JOIN_SEPARATOR :
	    slot->generated_space ? MANT_DISPLAY_JOIN_GENERATED_SEPARATOR :
	    MANT_DISPLAY_JOIN_UNKNOWN;
	if (join == MANT_DISPLAY_JOIN_UNKNOWN ||
	    (column->pending_spaces != 0 && column->pending_join != join)) {
		join_unknown(column);
		return 1;
	}
	if (column->pending_spaces == collector->session->limits->max_content_bytes) {
		mant_structured_set_failure(collector->session,
		    MANT_STRUCTURED_BUDGET, MANT_STRUCTURED_STAGE_RENDER, 10,
		    column->pending_spaces == UINT64_MAX ? UINT64_MAX :
		    column->pending_spaces + 1,
		    collector->session->limits->max_content_bytes);
		return 0;
	}
	if (column->pending_spaces == 0) {
		column->separator_owner = slot->owner;
		column->separator_link = slot->link;
	} else if (column->separator_owner != slot->owner ||
	    column->separator_link != slot->link) {
		join_unknown(column);
		return 1;
	}
	column->pending_join = join;
	column->pending_spaces++;
	return 1;
}

static int
charge_work(struct mant_annotated_collector *collector, uint64_t amount)
{
	struct structured_session *session = collector->session;

	return mant_structured_charge(session, &session->builder_operations,
	    amount, session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_RENDER);
}

static int
charge_mutations(struct mant_annotated_collector *collector, uint64_t amount)
{
	struct structured_session *session = collector->session;

	return mant_structured_charge(session, &session->annotation_mutations,
	    amount, session->limits->max_annotation_mutations, 29,
	    MANT_STRUCTURED_STAGE_RENDER);
}

/* Iterate the HEAD in the same child/sibling order as roff.c::deroff(),
 * without using the C stack for arbitrarily deep inline macro trees. */
static const struct roff_node *
heading_next(struct mant_annotated_collector *collector,
    const struct roff_node *root, const struct roff_node *node)
{
	if (node->string == NULL && node->child != NULL)
		return node->child;
	while (node != root) {
		if (node->next != NULL)
			return node->next;
		node = node->parent;
		if (!charge_work(collector, 1))
			return NULL;
		if (node == NULL) {
			fail_relation(collector, 0, 1);
			return NULL;
		}
	}
	return NULL;
}

/* Preserve deroff()'s leading escape/whitespace and trailing rules exactly.
 * The first nonempty leaf uses strndup(cp, sz); later leaves use %*s, which
 * is a minimum width and therefore retains their original trailing bytes. */
static int
heading_leaf(struct mant_annotated_collector *collector, const char *string,
    size_t *start, size_t *trimmed_end, size_t *full_end)
{
	size_t length, offset, end;

	for (length = 0; string[length] != '\0'; length++)
		if (!charge_work(collector, 1))
			return 0;
	for (offset = 0; offset < length; offset++) {
		if (!charge_work(collector, 1))
			return 0;
		if (string[offset] == '\\' && offset + 1 < length &&
		    strchr(" %&0^|~", string[offset + 1]) != NULL)
			offset++;
		else if (!isspace((unsigned char)string[offset]))
			break;
	}
	end = length;
	if (end > offset && string[end - 1] == '\\')
		end--;
	while (end > offset) {
		if (!charge_work(collector, 1))
			return 0;
		if (!isspace((unsigned char)string[end - 1]))
			break;
		end--;
	}
	*start = offset;
	*trimmed_end = end;
	*full_end = length;
	return 1;
}

static int
copy_heading_phrase(struct mant_annotated_collector *collector,
    struct mant_annotated_mark *mark, const struct roff_node *head)
{
	const struct roff_node *node;
	struct structured_session *session = collector->session;
	uint8_t *phrase;
	uint64_t length = 0, part;
	size_t start, trimmed_end, full_end, used = 0;
	int first = 1;

	if (head == NULL)
		return 1;
	for (node = head; node != NULL;
	    node = heading_next(collector, head, node)) {
		if (!charge_work(collector, 1))
			return 0;
		if (node->string == NULL)
			continue;
		if (!heading_leaf(collector, node->string, &start,
		    &trimmed_end, &full_end))
			return 0;
		if (trimmed_end == start)
			continue;
		part = first ? trimmed_end - start : full_end - start + 1;
		if (part > session->limits->max_content_bytes ||
		    length > session->limits->max_content_bytes - part) {
			mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET,
			    MANT_STRUCTURED_STAGE_RENDER, 10,
			    part > UINT64_MAX - length ? UINT64_MAX : length + part,
			    session->limits->max_content_bytes);
			return 0;
		}
		length += part;
		first = 0;
	}
	if (session->status != MANT_STRUCTURED_OK || length == 0)
		return session->status == MANT_STRUCTURED_OK;
	phrase = mant_structured_allocate(session, length, 0,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (phrase == NULL)
		return 0;
	first = 1;
	for (node = head; node != NULL;
	    node = heading_next(collector, head, node)) {
		if (!charge_work(collector, 1))
			goto failure;
		if (node->string == NULL)
			continue;
		if (!heading_leaf(collector, node->string, &start,
		    &trimmed_end, &full_end))
			goto failure;
		if (trimmed_end == start)
			continue;
		if (!first)
			phrase[used++] = ' ';
		part = first ? trimmed_end - start : full_end - start;
		if (!charge_work(collector, part))
			goto failure;
		memcpy(phrase + used, node->string + start, (size_t)part);
		used += part;
		first = 0;
	}
	if (session->status != MANT_STRUCTURED_OK)
		goto failure;
	if (!charge_work(collector, length))
		goto failure;
	if (!mant_structured_valid_utf8(phrase, (size_t)length)) {
		free(phrase);
		return 1;
	}
	if (!mant_structured_charge(session, &session->content_bytes,
	    length, session->limits->max_content_bytes, 10,
	    MANT_STRUCTURED_STAGE_RENDER))
		goto failure;
	mark->name = phrase;
	mark->name_length = length;
	return 1;
failure:
	free(phrase);
	return 0;
}

static struct annotated_column *
column_at(struct mant_annotated_collector *collector, size_t index)
{
	struct annotated_column *grown;
	uint32_t old_capacity;

	if (index >= UINT32_MAX) {
		fail_relation(collector, index, UINT32_MAX - 1);
		return NULL;
	}
	if (index < collector->column_capacity)
		return collector->columns + index;
	old_capacity = collector->column_capacity;
	grown = mant_structured_grow_array(collector->session,
	    collector->columns, (uint32_t)index,
	    &collector->column_capacity, UINT32_MAX,
	    sizeof(*collector->columns),
	    collector->session->limits->max_builder_allocated_bytes, 9,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (grown == NULL)
		return NULL;
	collector->columns = grown;
	memset(grown + old_capacity, 0,
	    (collector->column_capacity - old_capacity) * sizeof(*grown));
	return grown + index;
}

static struct annotated_slot *
slot_at(struct mant_annotated_collector *collector,
    struct annotated_column *column, size_t index)
{
	struct annotated_slot *grown;
	uint32_t old_capacity;
	uint64_t added;

	if (index >= UINT32_MAX) {
		fail_relation(collector, index, UINT32_MAX - 1);
		return NULL;
	}
	if (index < column->capacity)
		return column->slots + index;
	old_capacity = column->capacity;
	grown = mant_structured_grow_array(collector->session,
	    column->slots, (uint32_t)index, &column->capacity, UINT32_MAX,
	    sizeof(*column->slots),
	    collector->session->limits->max_builder_allocated_bytes, 9,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (grown == NULL)
		return NULL;
	column->slots = grown;
	memset(grown + old_capacity, 0,
	    (column->capacity - old_capacity) * sizeof(*grown));
	added = (uint64_t)(column->capacity - old_capacity) * sizeof(*grown);
	collector->slot_bytes += added;
	if (collector->metrics.peak_slot_bytes < collector->slot_bytes)
		collector->metrics.peak_slot_bytes = collector->slot_bytes;
	return grown + index;
}

static void
discard_slot(struct mant_annotated_collector *collector,
    struct annotated_column *column, struct annotated_slot *slot)
{
	if (slot->occupied) {
		column->live--;
		collector->live_slots--;
	}
	memset(slot, 0, sizeof(*slot));
}

static void
discard_range(struct mant_annotated_collector *collector,
    struct annotated_column *column, size_t first, size_t end)
{
	size_t index;

	if (first >= column->capacity)
		return;
	if (end > column->capacity)
		end = column->capacity;
	for (index = first; index < end; index++)
		discard_slot(collector, column, column->slots + index);
}

static int
capture_point_slot(struct mant_annotated_collector *collector,
    struct annotated_slot *slot,
    struct mant_annotated_display_checkpoint checkpoint)
{
	struct annotated_point_state *state;
	uint32_t key, next;

	for (key = slot->first_point; key != 0; key = next) {
		if (key > collector->mark_count ||
		    collector->pending_point_count == 0) {
			fail_relation(collector, key, collector->mark_count);
			return 0;
		}
		state = collector->points + key - 1;
		if (state->state != 1) {
			fail_relation(collector, state->state, 1);
			return 0;
		}
		next = state->next;
		state->checkpoint = checkpoint;
		state->next = 0;
		state->state = 2;
		collector->pending_point_count--;
	}
	slot->first_point = 0;
	return 1;
}

static int
capture_point_range(struct mant_annotated_collector *collector,
    struct annotated_column *column, size_t first, size_t end)
{
	struct mant_annotated_display_checkpoint checkpoint;
	size_t index;

	if (collector->pending_point_count == 0 ||
	    first >= column->capacity)
		return 1;
	if (end > column->capacity)
		end = column->capacity;
	if (end <= first)
		return 1;
	/* BUFFER_RESET/COL_FREE can cover capacity, not just live slots.
	 * Charge every inspected gap to the same cumulative work budget as
	 * collector events, even when a pending point belongs to another col. */
	if (!charge_work(collector, end - first))
		return 0;
	if (!mant_annotated_display_checkpoint(collector->display, 0,
	    &checkpoint)) {
		fail_relation(collector, first, end);
		return 0;
	}
	for (index = first; index < end; index++)
		if (!capture_point_slot(collector, column->slots + index,
		    checkpoint))
			return 0;
	return 1;
}

static void
arm_point(struct mant_annotated_collector *collector, struct termp *p,
    const struct term_collector_event *event)
{
	struct annotated_frame *frame;
	struct annotated_point_state *state;
	struct annotated_column *column;
	struct annotated_slot *slot;
	uint32_t key;

	if (collector->frame_count == 0 ||
	    collector->frames[collector->frame_count - 1].node != event->node) {
		fail_relation(collector, collector->frame_count, 1);
		return;
	}
	frame = collector->frames + collector->frame_count - 1;
	key = event->op == TERM_COLLECT_TAG_POINT ?
	    frame->anchor_mark : event->op == TERM_COLLECT_OWNER_POINT ?
	    frame->owner_mark : frame->region_mark;
	/* tag.c::tag_put() leaves `tag` NULL for an unchanged implicit
	 * heading ID; term_tag_write() falls back to the first child string.
	 * R01 AnchorMarks cover only stored target declarations, as push_node()
	 * does, so this real pager-tag event has no exposed mark to locate. */
	if (key == 0 && event->op == TERM_COLLECT_TAG_POINT &&
	    event->node->tag == NULL)
		return;
	if (key == 0 || key > collector->mark_count) {
		fail_relation(collector, key, collector->mark_count);
		return;
	}
	state = collector->points + key - 1;
	if (state->state != 0) {
		fail_relation(collector, state->state, 0);
		return;
	}
	if ((p->flags & TERMP_NOBUF) != 0 ||
	    (event->pos == 0 && p->tcol->lastcol == 0) ||
	    event->pos < event->end) {
		if (!mant_annotated_display_checkpoint(collector->display,
		    collector->advance_count, &state->checkpoint)) {
			fail_relation(collector, event->pos, event->end);
			return;
		}
		state->state = 2;
		return;
	}
	column = column_at(collector, event->column);
	if (column == NULL ||
	    (slot = slot_at(collector, column, event->pos)) == NULL)
		return;
	state->next = slot->first_point;
	slot->first_point = key;
	state->state = 1;
	collector->pending_point_count++;
}

static uint32_t
current_role(const struct mant_annotated_collector *collector)
{
	if (collector->in_header)
		return MANT_ANNOTATED_HEADER;
	if (collector->in_footer && collector->footer_drained)
		return MANT_ANNOTATED_FOOTER;
	return MANT_ANNOTATED_BODY;
}

static uint32_t
source_key(const struct roff_node *node)
{
	return node == NULL || (node->flags & NODE_NOSRC) != 0 ? 0 :
	    node->mant_source_key;
}

static int
within_node(const struct roff_node *node, const struct roff_node *ancestor)
{
	for (; node != NULL; node = node->parent)
		if (node == ancestor)
			return 1;
	return 0;
}

/* The link macro identifies the occurrence, but only selected terminal
 * characters belong to its clickable label.  These operand boundaries are
 * the same ones used by pinned man_html.c::man_MR_pre/man_UR_pre and
 * mdoc_html.c::mdoc_lk_pre; formatter-generated decoration is not inferred
 * from a source span or from a contiguous final display interval. */
static uint32_t
visible_link(const struct mant_annotated_collector *collector,
    const struct roff_node *node, enum term_collector_reason reason)
{
	const struct roff_node *macro = collector->active_link_node;
	const struct roff_node *first, *second, *punct, *body, *head;

	if (macro == NULL || collector->active_link == 0 ||
	    collector->active_link_epoch != collector->phrase_epoch)
		return 0;
	/* mdoc_html.c::mdoc_mt_pre opens a separate anchor for every text
	 * operand.  The terminal's automatic space before each operand is
	 * outside both adjacent anchors, even though term_word_node() reports
	 * that space with the operand as its current node. */
	if (macro->type == ROFFT_TEXT && macro->parent != NULL &&
	    macro->parent->tok == MDOC_Mt)
		return reason != TERM_COLLECT_AUTO_SPACE &&
		    within_node(node, macro) ? collector->active_link : 0;
	/* term.c::term_word() inserts an AUTO_SPACE before the first operand.
	 * man_html.c::man_MR_pre and mdoc_html.c::mdoc_xr_pre open the anchor
	 * after that separator.  Their terminal pre handlers keep NOSPACE
	 * between name and section, so no interior auto-space belongs here. */
	if ((macro->tok == MAN_MR || macro->tok == MDOC_Xr) &&
	    reason == TERM_COLLECT_AUTO_SPACE)
		return 0;
	/* man_term.c::pre_MR and mdoc_term.c::termp_xr_pre emit parentheses
	 * with term_word(), unlike operands emitted via term_word_node().
	 * Their logical event has no node, yet HTML keeps them in the anchor. */
	if (node == NULL)
		return macro->tok == MAN_MR || macro->tok == MDOC_Xr ?
		    collector->active_link : 0;
	switch (macro->tok) {
	case MAN_MR:
		/* man_term.c::pre_MR prints name(section), then a separate suffix. */
		first = macro->child;
		second = first == NULL ? NULL : first->next;
		return node == macro || within_node(node, first) ||
		    within_node(node, second) ? collector->active_link : 0;
	case MAN_UR:
	case MAN_MT:
		/* man_term.c::post_UR prints the target in angle brackets after
		 * the link body.  An empty body replays the head as the label. */
		head = macro->head;
		body = macro->body;
		if (body != NULL && body->child != NULL)
			return node != body && within_node(node, body) ?
			    collector->active_link : 0;
		return head != NULL && node != head &&
		    within_node(node, head) ? collector->active_link : 0;
	case MDOC_Lk:
		first = macro->child;
		if (first == NULL)
			return 0;
		punct = macro->last;
		while (punct != first && (punct->flags & NODE_DELIMC) != 0)
			punct = punct->prev;
		punct = punct->next;
		/* mdoc_html.c::mdoc_lk_pre uses the destination as label only
		 * when there is no description; trailing delimiters are outside. */
		if (first->next == punct)
			return within_node(node, first) ? collector->active_link : 0;
		for (second = first->next; second != punct;
		    second = second->next)
			if (within_node(node, second))
				return collector->active_link;
		return 0;
	default:
		return within_node(node, macro) ? collector->active_link : 0;
	}
}

static uint32_t
add_mark(struct mant_annotated_collector *collector,
    const struct roff_node *node, const struct roff_node *origin,
    uint32_t kind, uint32_t parent, uint32_t region_kind,
    const struct roff_node *link_operand)
{
	struct mant_annotated_mark *marks, *mark;
	struct annotated_point_state *points;
	const struct roff_node *first, *second;
	size_t name_length;
	uint32_t maximum;

	maximum = collector->session->limits->max_transfer_objects >
	    UINT32_MAX ? UINT32_MAX :
	    (uint32_t)collector->session->limits->max_transfer_objects;
	marks = mant_structured_grow_array(collector->session,
	    collector->marks, collector->mark_count,
	    &collector->mark_capacity, maximum, sizeof(*marks),
	    collector->session->limits->max_builder_allocated_bytes, 32,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (marks == NULL)
		return 0;
	collector->marks = marks;
	points = mant_structured_grow_array(collector->session,
	    collector->points, collector->mark_count,
	    &collector->point_capacity, maximum, sizeof(*points),
	    collector->session->limits->max_builder_allocated_bytes, 9,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (points == NULL)
		return 0;
	collector->points = points;
	mark = marks + collector->mark_count;
	memset(mark, 0, sizeof(*mark));
	memset(points + collector->mark_count, 0, sizeof(*points));
	mark->key = ++collector->mark_count;
	mark->kind = kind;
	mark->parent = parent;
	mark->owner = collector->active_owner;
	mark->region_kind = region_kind;
	mark->token = node == NULL ? 0 : node->tok;
	mark->source = source_key(origin);
	if (mark->source != 0 && origin != NULL &&
	    origin->line > 0 && origin->pos >= 0 &&
	    (uint64_t)origin->line <= UINT32_MAX &&
	    (uint64_t)origin->pos < UINT32_MAX) {
		mark->line = origin->line;
		mark->column = origin->pos + 1;
	}
	if (mark->source != 0 && mark->line != 0)
		mark->flags |= MANT_ANNOTATED_MARK_AUTHORED;
	if (mark->source > collector->session->result->source_count) {
		fail_relation(collector, mark->source,
		    collector->session->result->source_count);
		return 0;
	}
	if (kind == MANT_ANNOTATED_MARK_HEADING) {
		/* roff.c::deroff() is also the pinned structured heading-evidence
		 * rule.  Read the HEAD, never the BLOCK's subsequent BODY. */
		if (!copy_heading_phrase(collector, mark,
		    node == NULL ? NULL : node->head))
			return 0;
	} else if (kind == MANT_ANNOTATED_MARK_ANCHOR) {
		mark->flags |= MANT_ANNOTATED_MARK_FINAL_POINT_UNVERIFIED;
		/* tag.c::tag_put()/tag_move_id() own the final NUL-terminated
		 * spelling; a moved .Tg keeps its authored source separately. */
		if (node == NULL || node->tag == NULL ||
		    (name_length = strlen(node->tag)) == 0 ||
		    !mant_structured_valid_utf8(
		    (const uint8_t *)node->tag, name_length)) {
			mant_structured_set_failure(collector->session,
			    MANT_STRUCTURED_UNSUPPORTED,
			    MANT_STRUCTURED_STAGE_RENDER, 0, 0, 0);
			return 0;
		}
		mark->name = mant_structured_copy_bytes(collector->session,
		    (const uint8_t *)node->tag, name_length, 1,
		    MANT_STRUCTURED_STAGE_RENDER);
		if (mark->name == NULL)
			return 0;
		mark->name_length = name_length;
		if (mant_tag_is_manual(node->tag))
			mark->flags |= MANT_ANNOTATED_MARK_MANUAL_TARGET;
	} else if (kind == MANT_ANNOTATED_MARK_LINK) {
		/* The macro identifies the kind; .Mt uses each operand as its
		 * authored origin.  Copy the destination while the parsed tree is
		 * alive, using the existing structured link decoder. */
		first = second = NULL;
		switch (node->tok) {
		case MAN_UR:
		case MAN_MT:
			mark->target_kind = node->tok == MAN_UR ?
			    MANT_LINK_EXTERNAL : MANT_LINK_EMAIL;
			first = node->head == NULL ? NULL : node->head->child;
			break;
		case MAN_MR:
		case MDOC_Xr:
			first = node->child;
			second = first == NULL ? NULL : first->next;
			mark->target_kind = second == NULL ?
			    MANT_LINK_DOCUMENT : MANT_LINK_MANUAL;
			break;
		case MDOC_Lk:
			mark->target_kind = MANT_LINK_EXTERNAL;
			first = node->child;
			break;
		case MDOC_Sx:
			mark->target_kind = MANT_LINK_SECTION;
			if (!mant_structured_copy_deroff_target(
			    collector->session, &mark->target_a, node))
				goto unsupported_target;
			break;
		case MDOC_Mt:
			/* Each child is one native link instance, not one address
			 * shared by the enclosing .Mt macro.  The parser and HTML
			 * formatter both require direct text children here. */
			if (link_operand == NULL || link_operand->parent != node ||
			    link_operand->type != ROFFT_TEXT) {
				fail_relation(collector, mark->key, 0);
				return 0;
			}
			mark->target_kind = MANT_LINK_EMAIL;
			first = link_operand;
			break;
		default:
			break;
		}
		/* A missing operand is not an empty decoded destination.  The
		 * former yields no .UR/.MT/.Lk/.Xr/.Sx occurrence at all (filtered
		 * by push_node), except .MR's valid no-href () anchor. */
		if (node->tok != MDOC_Sx && first == NULL) {
			mark->target_kind = 0;
		}
		if (mark->target_kind != 0 && first != NULL &&
		    !mant_structured_copy_link_target_allow_empty(
		    collector->session,
		    &mark->target_a, first))
			goto unsupported_target;
		if (second != NULL) {
			mark->target_b_present = 1;
			if (!mant_structured_copy_link_target_allow_empty(
		    collector->session, &mark->target_b, second))
				goto unsupported_target;
		}
		/* man_html.c::man_MR_pre and mdoc_html.c::mdoc_xr_pre only
		 * create a destination with nonempty name and section.  A raw
		 * operand such as \& can decode to empty while remaining a valid
		 * visible no-href instance.  Lk/UR/MT instead retain href="". */
		if ((mark->target_kind == MANT_LINK_DOCUMENT ||
		    mark->target_kind == MANT_LINK_MANUAL) &&
		    (mark->target_a.len == 0 ||
		    (mark->target_b_present && mark->target_b.len == 0))) {
			free((void *)mark->target_a.ptr);
			free((void *)mark->target_b.ptr);
			memset(&mark->target_a, 0, sizeof(mark->target_a));
			memset(&mark->target_b, 0, sizeof(mark->target_b));
			mark->target_kind = mark->target_b_present = 0;
		}
	}
	collector->metrics.mark_count = collector->mark_count;
	return mark->key;

unsupported_target:
	if (collector->session->status == MANT_STRUCTURED_OK)
		mant_structured_set_failure(collector->session,
		    MANT_STRUCTURED_UNSUPPORTED, MANT_STRUCTURED_STAGE_RENDER,
		    0, 0, 0);
	return 0;
}

/* roff.c::roff_node_prev() skips a wider set of transparent nodes, including
 * layout controls. For reading-context evidence, only these non-content
 * siblings can stand between direct .IP blocks. A real flow boundary also
 * changes roff_node::flow_epoch at allocation. */
static int
ip_sibling_gap(const struct roff_node *node)
{
	return node->type == ROFFT_COMMENT || node->tok == MAN_PD ||
	    node->tok == MDOC_Sm || node->tok == MDOC_Tg ||
	    node->tok == ROFF_ft;
}

static int
direct_ip_predecessor(struct mant_annotated_collector *collector,
    const struct roff_node *node, const struct roff_node **candidate)
{
	const struct roff_node *previous;

	*candidate = NULL;
	for (previous = node->prev; previous != NULL &&
	    ip_sibling_gap(previous); previous = previous->prev)
		if (!charge_work(collector, 1))
			return 0;
	if (!charge_work(collector, 1))
		return 0;
	if (previous != NULL && previous->type == ROFFT_BLOCK &&
	    previous->tok == MAN_IP && previous->parent == node->parent &&
	    previous->flow_epoch == node->flow_epoch)
		*candidate = previous;
	return 1;
}

static int
observe_html_phrase_boundary(struct mant_annotated_collector *collector,
    const struct roff_node *node)
{
	int nofill, closes;

	/* man_html.c::print_man_node() skips comments and NOPRT before fillmode
	 * or a macro handler can close an in-phrase anchor. NODE_ENTER
	 * precedes output from this node and observes the same normalized tree. */
	if (node->type == ROFFT_COMMENT || (node->flags & NODE_NOPRT) != 0)
		return 1;
	nofill = (node->flags & NODE_NOFILL) != 0;
	closes = nofill != collector->html_nofill;
	collector->html_nofill = nofill;
	/* html.c::html_fillmode() closes phrase tags on both fi->nf and
	 * nf->fi (the latter closes PRE with any nested A); tbl_html.c::
	 * html_tblopen(),
	 * roff_html.c::roff_html_pre_sp(), and the paragraph/list/section
	 * handlers in man_html.c close all active phrase tags, including A.
	 * A later nested link may open, but it cannot revive an old ancestor. */
	if (node->type == ROFFT_TBL ||
	    (node->tok == ROFF_sp && !nofill) ||
	    (node->type == ROFFT_BLOCK &&
	    (node->tok == MAN_PP || node->tok == MAN_LP ||
	    node->tok == MAN_P || node->tok == MAN_HP ||
	    node->tok == MAN_IP || node->tok == MAN_TP ||
	    node->tok == MAN_TQ || node->tok == MAN_SH ||
	    node->tok == MAN_SS || node->tok == MAN_RS ||
	    node->tok == MAN_SY)))
		closes = 1;
	if (!closes || collector->active_link == 0 ||
	    collector->active_link_epoch != collector->phrase_epoch)
		return 1;
	if (collector->phrase_epoch == UINT64_MAX) {
		mant_structured_set_failure(collector->session,
		    MANT_STRUCTURED_BUDGET, MANT_STRUCTURED_STAGE_RENDER, 29,
		    UINT64_MAX, UINT64_MAX - 1);
		return 0;
	}
	collector->phrase_epoch++;
	return 1;
}

/* term.c::term_word() emits no glyph for IGNORE, NOSPACE and font escapes.
 * All other escapes remain significant here, including skipchar/overstrike
 * state and potentially visible special or Unicode characters. */
static int
head_text_has_glyph(const char *text)
{
	enum mandoc_esc esc;
	const unsigned char *plain;

	while (*text != '\0') {
		if (*text != '\\') {
			plain = (const unsigned char *)text;
			if (!isspace(*plain))
				return 1;
			text++;
			continue;
		}
		text++;
		esc = mandoc_escape(&text, NULL, NULL);
		switch (esc) {
		case ESCAPE_IGNORE:
		case ESCAPE_NOSPACE:
		case ESCAPE_FONT:
		case ESCAPE_FONTROMAN:
		case ESCAPE_FONTITALIC:
		case ESCAPE_FONTBOLD:
		case ESCAPE_FONTBI:
		case ESCAPE_FONTCR:
		case ESCAPE_FONTCB:
		case ESCAPE_FONTCI:
		case ESCAPE_FONTPREV:
			break;
		default:
			return 1;
		}
	}
	return 0;
}

/* mdoc_macro.c::blk_full() closes the It HEAD before terminal traversal.
 * Freeze the first significant authored head macro while that native tree
 * remains alive; final bold glyphs alone cannot distinguish Fl/Ev/Ic/Cm
 * from unrelated typography.  This is evidence, not classification. */
static uint32_t
owner_head_role(const struct roff_node *owner,
    const struct roff_node **role_node)
{
	const struct roff_node *head, *node;
	int skip_children;

	head = owner->head;
	*role_node = NULL;
	if (head == NULL)
		return 0;
	for (node = head->child; node != NULL; ) {
		skip_children = 0;
		switch (node->tok) {
		case MDOC_Fl:
			*role_node = node;
			return MANT_ANNOTATED_MARK_HEAD_OPTION;
		case MDOC_Ev:
			*role_node = node;
			return MANT_ANNOTATED_MARK_HEAD_ENVIRONMENT;
		case MDOC_Ic:
		case MDOC_Cm:
			*role_node = node;
			return MANT_ANNOTATED_MARK_HEAD_LITERAL;
		case MDOC_Ar:
		case MDOC_Em:
		case MDOC_Sy:
			return 0;
		case MDOC_Tg:
		case MDOC_Ns:
		case MDOC_Sm:
			skip_children = 1;
			break;
		default:
			break;
		}
		if (node->type == ROFFT_TEXT && node->string != NULL &&
		    head_text_has_glyph(node->string))
			return 0;
		if (!skip_children && node->child != NULL) {
			node = node->child;
			continue;
		}
		while (node != head && node->next == NULL)
			node = node->parent;
		if (node == head)
			break;
		node = node->next;
	}
	return 0;
}

/* man_macro.c::blk_imp keeps TP/TQ HEAD distinct.  man_term.c::pre_B and
 * pre_I select different presentation fonts, not semantic kinds.  This
 * deliberately conservative candidate hint accepts one plain B word only;
 * Rust still checks the complete final spelling before classification. */
static int
owner_lexical_head(const struct roff_node *owner)
{
	const struct roff_node *head, *first;

	if (owner->tok != MAN_TP && owner->tok != MAN_TQ)
		return 0;
	head = owner->head;
	first = head == NULL ? NULL : head->child;
	return first != NULL && first->tok == MAN_B &&
	    first->next == NULL && first->child != NULL &&
	    first->child->type == ROFFT_TEXT &&
	    first->child->string != NULL &&
	    strchr(first->child->string, '\\') == NULL &&
	    first->child->next == NULL;
}

/* man_macro.c::blk_imp retains the first .IP argument as one HEAD text node;
 * man_term.c::pre_IP prints that node but uses the next argument for width.
 * Freeze only an authored leading bold dash spelling.  term.c::term_word()
 * prints \- as '-' and changes font without a glyph.  Any other escape or
 * an unproved boundary leaves the .IP a presentation owner, not a name. */
static int
copy_ip_option_prefix(struct mant_annotated_collector *collector,
    struct mant_annotated_mark *mark, const struct roff_node *owner,
    int *recognized)
{
	const struct roff_node *first;
	const char *cursor, *next, *sequence;
	char candidate[256];
	size_t source_size, length = 0, index;
	int size;
	enum mandoc_esc escape;
	uint8_t *copy;

	*recognized = 0;
	first = owner->head == NULL ? NULL : owner->head->child;
	if (first == NULL || first->type != ROFFT_TEXT ||
	    first->string == NULL || first->string[0] != '\\')
		return 1;
	source_size = strlen(first->string);
	if (!charge_work(collector, source_size))
		return 0;
	cursor = first->string + 1;
	if (mandoc_escape(&cursor, NULL, NULL) != ESCAPE_FONTBOLD)
		return 1;
	while (*cursor != '\0') {
		if (*cursor == '\\') {
			next = cursor + 1;
			escape = mandoc_escape(&next, &sequence, &size);
			if (escape == ESCAPE_FONTROMAN) {
				cursor = next;
				if (*cursor != '\0' && *cursor != '=' &&
				    !isspace((unsigned char)*cursor))
					return 1;
				break;
			}
			if (escape != ESCAPE_SPECIAL || size != 1 ||
			    sequence[0] != '-')
				return 1;
			cursor = next;
			candidate[length] = '-';
		} else if (*cursor == '=' ||
		    isspace((unsigned char)*cursor)) {
			break;
		} else {
			if ((unsigned char)*cursor > 0x7f ||
			    (!isalnum((unsigned char)*cursor) &&
		    strchr("-_.?+", *cursor) == NULL))
				return 1;
			candidate[length] = *cursor++;
		}
		if (++length == sizeof(candidate))
			return 1;
	}
	if (length < 2 || candidate[0] != '-' ||
	    (candidate[1] == '-' && length < 3) ||
	    (candidate[1] != '-' && isdigit((unsigned char)candidate[1])))
		return 1;
	for (index = 1; index < length; index++)
		if (isalnum((unsigned char)candidate[index]) ||
		    candidate[index] == '?')
			break;
	if (index == length)
		return 1;
	if (!mant_structured_charge(collector->session,
	    &collector->session->content_bytes, length,
	    collector->session->limits->max_content_bytes, 10,
	    MANT_STRUCTURED_STAGE_RENDER))
		return 0;
	copy = mant_structured_allocate(collector->session, length, 0,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (copy == NULL)
		return 0;
	memcpy(copy, candidate, length);
	mark->name = copy;
	mark->name_length = length;
	*recognized = 1;
	return 1;
}

/* mdoc_term.c::termp_fl_pre emits its generated dash before traversing the
 * child text; man_term.c::pre_B keeps its own children in the same macro
 * frame.  Native syntax is evidence only: C never splits declaration forms. */
static uint32_t
head_component_role(const struct roff_node *node)
{
	if (node->type != ROFFT_ELEM)
		return 0;
	switch (node->tok) {
	case MDOC_Fl:
		return MANT_ANNOTATED_MARK_HEAD_OPTION;
	case MDOC_Ev:
		return MANT_ANNOTATED_MARK_HEAD_ENVIRONMENT;
	case MDOC_Ic:
	case MDOC_Cm:
		return MANT_ANNOTATED_MARK_HEAD_LITERAL;
	case MAN_B:
	case MAN_BR:
		return MANT_ANNOTATED_MARK_HEAD_LEXICAL;
	default:
		return 0;
	}
}

/* mdoc_term.c::termp_fl_pre emits its own dash before the Fl operand;
 * termp_ns_pre may then glue a different macro's glyphs to it.  Preserve a
 * deliberately simple authored operand while the AST lives.  Rust must still
 * match it against final surviving output before binding any semantic name. */
static int
copy_owner_head_operand(struct mant_annotated_collector *collector,
    struct mant_annotated_mark *mark, const struct roff_node *role_node)
{
	const struct roff_node *operand;
	struct structured_session *session = collector->session;
	const char *value;
	size_t size, total;
	uint8_t *copy;

	if (role_node == NULL || (role_node->tok != MDOC_Fl &&
	    role_node->tok != MDOC_Ev))
		return 1;
	operand = role_node->child;
	/* mdoc_macro.c::macro_or_word() permits an empty Fl element before
	 * closing/middle punctuation.  The next HEAD text node is the exact
	 * delimiter printed against termp_fl_pre()'s generated dash. */
	if (operand == NULL && role_node->tok == MDOC_Fl &&
	    role_node->next != NULL &&
	    role_node->next->type == ROFFT_TEXT &&
	    role_node->next->string != NULL &&
	    (mdoc_isdelim(role_node->next->string) == DELIM_CLOSE ||
	    mdoc_isdelim(role_node->next->string) == DELIM_MIDDLE))
		operand = role_node->next;
	if (operand == NULL || operand->next != NULL ||
	    operand->type != ROFFT_TEXT || operand->string == NULL ||
	    operand->string[0] == '\0' ||
	    strpbrk(operand->string, "\\ \t\r\n") != NULL)
		return 1;
	value = operand->string;
	size = strlen(value);
	if (size == SIZE_MAX || !charge_work(collector, size))
		return 0;
	if (!mant_structured_valid_utf8(
	    (const uint8_t *)value, size))
		return 1;
	total = size + (role_node->tok == MDOC_Fl);
	if (!mant_structured_charge(session, &session->content_bytes,
	    total, session->limits->max_content_bytes, 10,
	    MANT_STRUCTURED_STAGE_RENDER))
		return 0;
	copy = mant_structured_allocate(session, total, 0,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (copy == NULL)
		return 0;
	if (role_node->tok == MDOC_Fl)
		copy[0] = '-';
	memcpy(copy + total - size, value, size);
	mark->name = copy;
	mark->name_length = total;
	return 1;
}

static int
push_node(struct mant_annotated_collector *collector,
    const struct roff_node *node)
{
	struct annotated_frame *frames, *frame;
	struct mant_annotated_mark *parent_mark;
	struct annotated_point_state *state;
	uint32_t key, region_kind, parent;

	if (node == NULL || collector->frame_count >=
	    collector->session->limits->max_nesting_depth ||
	    collector->frame_count == UINT32_MAX) {
		mant_structured_set_failure(collector->session,
		    MANT_STRUCTURED_BUDGET, MANT_STRUCTURED_STAGE_RENDER,
		    0, (uint64_t)collector->frame_count + 1,
		    collector->session->limits->max_nesting_depth);
		return 0;
	}
	if (!observe_html_phrase_boundary(collector, node))
		return 0;
	/* man_term.c::print_man_nodelist() and mdoc_term.c::
	 * print_mdoc_nodelist() traverse the first ROOT child, not ROOT itself.
	 * The first unsectioned text therefore opens a native direct region;
	 * subsequent style nodes retain that owner across their frame leaves. */
	if (node->type == ROFFT_TEXT && collector->active_owner == 0 &&
	    !collector->in_header && !collector->in_footer) {
		if (collector->unsectioned_region == 0) {
			key = add_mark(collector, NULL, NULL,
			    MANT_ANNOTATED_MARK_REGION, 0,
			    MANT_ANNOTATED_REGION_UNSECTIONED, NULL);
			if (key == 0)
				return 0;
			state = collector->points + key - 1;
			if (!mant_annotated_display_checkpoint(collector->display,
			    collector->advance_count, &state->checkpoint)) {
				fail_relation(collector, key, 0);
				return 0;
			}
			state->state = 2;
			collector->unsectioned_region = key;
		}
		collector->active_owner = collector->unsectioned_region;
	}
	frames = mant_structured_grow_array(collector->session,
	    collector->frames, collector->frame_count,
	    &collector->frame_capacity, UINT32_MAX, sizeof(*frames),
	    collector->session->limits->max_builder_allocated_bytes, 9,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (frames == NULL)
		return 0;
	collector->frames = frames;
	frame = frames + collector->frame_count++;
	frame->node = node;
	frame->saved_owner = collector->active_owner;
	frame->saved_head_component = collector->active_head_component;
	frame->saved_link = collector->active_link;
	frame->saved_link_node = collector->active_link_node;
	frame->saved_heading = collector->active_heading;
	frame->saved_link_epoch = collector->active_link_epoch;
	frame->owner_mark = frame->anchor_mark = frame->region_mark = 0;
	frame->last_direct_ip_node = NULL;
	frame->last_direct_ip_owner = 0;

	/* man_macro.c::blk_imp and mdoc_macro.c::blk_full produce a block
	 * with distinct HEAD/BODY scopes.  Their terminal traversal emits
	 * ENTER/LEAVE on every scope, without requiring a formatter flush. */
	if (node->type == ROFFT_BLOCK &&
	    (node->tok == MAN_SH || node->tok == MAN_SS ||
	    node->tok == MDOC_Sh || node->tok == MDOC_Ss)) {
		parent = node->tok == MAN_SS || node->tok == MDOC_Ss ?
		    collector->last_top_heading : 0;
		key = add_mark(collector, node, node,
		    MANT_ANNOTATED_MARK_HEADING, parent, 0, NULL);
		if (key == 0)
			return 0;
		if (node->tok == MAN_SS || node->tok == MDOC_Ss)
			collector->marks[key - 1].flags |=
			    MANT_ANNOTATED_MARK_SUBSECTION;
		collector->active_heading = key;
		collector->active_owner = key;
		if (node->tok == MAN_SH || node->tok == MDOC_Sh)
			collector->last_top_heading = key;
	} else if (node->type == ROFFT_BLOCK &&
	    (node->tok == MAN_TP || node->tok == MAN_IP ||
	    node->tok == MAN_TQ ||
	    node->tok == MDOC_It)) {
		const struct roff_node *bl;
		const struct roff_node *preceding_node, *completed_node;
		const struct annotated_frame *parent_frame;
		const struct roff_node *role_node = NULL;
		int definition;
		uint32_t head_role = 0, preceding_key;

		/* man_term.c::pre_TP/post_TP present HEAD as a named term.
		 * mdoc_term.c::termp_it_pre uses Bl's validated list type;
		 * bullet/enum/column heads are not definition declarations. */
		definition = node->tok == MAN_TP || node->tok == MAN_TQ;
		if (node->tok == MDOC_It) {
			bl = node->parent == NULL ? NULL : node->parent->parent;
			if (bl != NULL && bl->tok == MDOC_Bl && bl->norm != NULL)
				switch (bl->norm->Bl.type) {
				case LIST_tag:
				case LIST_hang:
				case LIST_diag:
				case LIST_inset:
				case LIST_ohang:
					definition = 1;
					break;
				default:
					break;
				}
		}
		key = add_mark(collector, node, node,
		    MANT_ANNOTATED_MARK_OWNER, collector->active_owner, 0, NULL);
		if (key == 0)
			return 0;
		if (node->tok == MAN_IP &&
		    !copy_ip_option_prefix(collector,
		    collector->marks + key - 1, node, &definition))
			return 0;
		if (definition) {
			if (node->tok == MAN_IP) {
				if (!direct_ip_predecessor(collector, node,
				    &preceding_node))
					return 0;
				parent_frame = collector->frame_count > 1 ?
				    collector->frames + collector->frame_count - 2 : NULL;
				if (parent_frame == NULL) {
					completed_node = collector->last_root_ip_node;
					preceding_key = collector->last_root_ip_owner;
				} else if (parent_frame->node == node->parent) {
					completed_node = parent_frame->last_direct_ip_node;
					preceding_key = parent_frame->last_direct_ip_owner;
				} else {
					completed_node = NULL;
					preceding_key = 0;
				}
				/* A completed direct sibling and the AST's nearest eligible
				 * predecessor must identify the same owner.  Merely sharing
				 * a rendered position cannot establish reading context. */
				if (preceding_node != NULL &&
				    preceding_node == completed_node &&
				    preceding_key != 0 &&
				    collector->marks[preceding_key - 1].parent ==
				    collector->marks[key - 1].parent)
					collector->marks[key - 1].preceding_owner =
					    preceding_key;
			}
			if (node->tok == MDOC_It)
				head_role = owner_head_role(node, &role_node);
			else if (node->tok == MAN_IP)
				head_role = MANT_ANNOTATED_MARK_HEAD_LEXICAL;
			else if (owner_lexical_head(node))
				head_role = MANT_ANNOTATED_MARK_HEAD_LEXICAL;
			collector->marks[key - 1].flags |=
			    MANT_ANNOTATED_MARK_DEFINITION |
			    head_role;
			if (!copy_owner_head_operand(collector,
			    collector->marks + key - 1, role_node))
				return 0;
		}
		collector->active_owner = key;
		frame->owner_mark = key;
	} else if (node->type == ROFFT_BLOCK &&
	    (node->tok == MDOC_Bl || node->tok == MDOC_Bd)) {
		region_kind = node->tok == MDOC_Bl ?
		    MANT_ANNOTATED_REGION_LIST : MANT_ANNOTATED_REGION_LITERAL;
		key = add_mark(collector, node, node,
		    MANT_ANNOTATED_MARK_REGION, collector->active_owner,
		    region_kind, NULL);
		if (key == 0)
			return 0;
		collector->active_owner = key;
		frame->region_mark = key;
	} else if (node->type == ROFFT_TBL ||
	    node->type == ROFFT_EQN) {
		if (node->type == ROFFT_TBL && collector->table_node != NULL) {
			fail_relation(collector, collector->frame_count, 0);
			return 0;
		}
		region_kind = node->type == ROFFT_TBL ?
		    MANT_ANNOTATED_REGION_TABLE_SPAN :
		    MANT_ANNOTATED_REGION_EQUATION;
		key = add_mark(collector, node, node,
		    MANT_ANNOTATED_MARK_REGION, collector->active_owner,
		    region_kind, NULL);
		if (key == 0)
			return 0;
		collector->active_owner = key;
		frame->region_mark = key;
		if (node->type == ROFFT_TBL) {
			collector->table_node = node;
			collector->cell_count = 0;
			collector->table_prepared = 0;
		}
	} else if ((node->type == ROFFT_HEAD ||
	    node->type == ROFFT_BODY) &&
	    (node->tok == MAN_SH || node->tok == MAN_SS ||
	    node->tok == MDOC_Sh || node->tok == MDOC_Ss ||
	    node->tok == MAN_TP || node->tok == MAN_IP ||
	    node->tok == MAN_TQ ||
	    node->tok == MDOC_It)) {
		parent = collector->active_owner;
		parent_mark = parent == 0 ? NULL :
		    collector->marks + parent - 1;
		if (parent_mark == NULL) {
			fail_relation(collector, node->tok, parent);
			return 0;
		}
		if (parent_mark->kind == MANT_ANNOTATED_MARK_HEADING)
			region_kind = node->type == ROFFT_HEAD ?
			    MANT_ANNOTATED_REGION_HEADING_TITLE :
			    MANT_ANNOTATED_REGION_HEADING_BODY;
		else if (parent_mark->kind == MANT_ANNOTATED_MARK_OWNER)
			region_kind = node->type == ROFFT_HEAD ?
			    MANT_ANNOTATED_REGION_OWNER_TERM :
			    MANT_ANNOTATED_REGION_OWNER_BODY;
		else {
			fail_relation(collector, parent_mark->kind,
			    MANT_ANNOTATED_MARK_OWNER);
			return 0;
		}
		key = add_mark(collector, node, node,
		    MANT_ANNOTATED_MARK_REGION, parent, region_kind, NULL);
		if (key == 0)
			return 0;
		frame->region_mark = key;
		/* add_mark() may reallocate the array, so reselect parent. */
		parent_mark = collector->marks + parent - 1;
		if (node->type == ROFFT_HEAD)
			parent_mark->title_region = key;
		else
			parent_mark->body_region = key;
		collector->active_owner = key;
	}
	/* A component is only a definition HEAD macro instance, never a
	 * style-looking body glyph or a list label without a declaration owner. */
	if (collector->active_owner != 0 &&
	    (region_kind = head_component_role(node)) != 0) {
		parent = collector->active_owner;
		parent_mark = collector->marks + parent - 1;
		if (parent_mark->kind == MANT_ANNOTATED_MARK_REGION &&
		    parent_mark->region_kind ==
		    MANT_ANNOTATED_REGION_OWNER_TERM &&
		    parent_mark->parent != 0 &&
		    (collector->marks[parent_mark->parent - 1].flags &
	    MANT_ANNOTATED_MARK_DEFINITION) != 0) {
			key = add_mark(collector, node, node,
			    MANT_ANNOTATED_MARK_HEAD_COMPONENT, parent, 0, NULL);
			if (key == 0)
				return 0;
			collector->marks[key - 1].flags |= region_kind;
			collector->active_head_component = key;
		}
	}

	if ((node->type == ROFFT_BLOCK || node->type == ROFFT_ELEM) &&
	    (node->tok == MAN_UR || node->tok == MAN_MT ||
	    node->tok == MAN_MR || node->tok == MDOC_Lk ||
	    node->tok == MDOC_Xr || node->tok == MDOC_Sx) &&
	    /* man_html.c::man_UR_pre has no anchor without a head operand;
	     * mdoc_html.c's Lk/Xr/Sx handlers emit no visible link without a
	     * child.  .MR differs: its generated () still forms an anchor. */
	    (node->tok == MAN_MR ||
	    ((node->tok == MAN_UR || node->tok == MAN_MT) ?
	    node->head != NULL && node->head->child != NULL :
	    node->child != NULL))) {
		key = add_mark(collector, node, node,
		    MANT_ANNOTATED_MARK_LINK, collector->active_link, 0, NULL);
		if (key == 0)
			return 0;
		collector->active_link = key;
		collector->active_link_node = node;
		collector->active_link_epoch = collector->phrase_epoch;
	} else if (node->type == ROFFT_TEXT && node->parent != NULL &&
	    node->parent->tok == MDOC_Mt) {
		/* mdoc_html.c::mdoc_mt_pre gives each direct operand its own
		 * mailto anchor.  The occurrence belongs to the exact operand;
		 * post_defaults() marks a generated ~ as NODE_NOSRC. */
		key = add_mark(collector, node->parent, node,
		    MANT_ANNOTATED_MARK_LINK, collector->active_link, 0, node);
		if (key == 0)
			return 0;
		collector->active_link = key;
		collector->active_link_node = node;
		collector->active_link_epoch = collector->phrase_epoch;
	}
	if ((node->flags & NODE_ID) != 0 && node->tag != NULL &&
	    node->tag[0] != '\0') {
		const struct roff_node *origin =
		    node->mant_manual_target_source == NULL ? node :
		    node->mant_manual_target_source;
		key = add_mark(collector, node, origin,
		    MANT_ANNOTATED_MARK_ANCHOR,
		    collector->active_owner, 0, NULL);
		if (key == 0)
			return 0;
		frame->anchor_mark = key;
	}
	return 1;
}

static int
text_cell(const struct tbl_cell *layout, const struct tbl_dat *data)
{
	if (layout->pos != TBL_CELL_LONG &&
	    layout->pos != TBL_CELL_CENTRE &&
	    layout->pos != TBL_CELL_LEFT &&
	    layout->pos != TBL_CELL_RIGHT &&
	    layout->pos != TBL_CELL_NUMBER)
		return 0;
	return data == NULL || data->pos == TBL_DATA_DATA ||
	    data->pos == TBL_DATA_NONE;
}

static int
prepare_table_span(struct mant_annotated_collector *collector,
    const struct roff_node *node)
{
	const struct tbl_span *span;
	const struct tbl_cell *layout;
	const struct tbl_dat *data, *current;
	struct annotated_cell *grown;
	struct mant_annotated_mark *mark;
	uint32_t columns, index, previous = UINT32_MAX, maximum, key;

	if (collector->table_node != node || collector->table_prepared ||
	    node->span == NULL || node->span->opts == NULL) {
		fail_relation(collector, collector->frame_count, 0);
		return 0;
	}
	collector->table_prepared = 1;
	span = node->span;
	collector->cell_count = 0;
	if (span->pos != TBL_SPAN_DATA)
		return 1;
	if (span->opts->cols < 0 || span->layout == NULL) {
		fail_relation(collector, span->opts->cols, 0);
		return 0;
	}
	columns = (uint32_t)span->opts->cols;
	if (columns == 0)
		return 1;
	maximum = collector->session->limits->max_table_cells > UINT32_MAX ?
	    UINT32_MAX :
	    (uint32_t)collector->session->limits->max_table_cells;
	if (columns > maximum) {
		mant_structured_set_failure(collector->session,
		    MANT_STRUCTURED_BUDGET, MANT_STRUCTURED_STAGE_RENDER,
		    19, columns, maximum);
		return 0;
	}
	if (!charge_work(collector, columns))
		return 0;
	grown = mant_structured_grow_array(collector->session,
	    collector->cells, columns - 1, &collector->cell_capacity,
	    maximum, sizeof(*grown),
	    collector->session->limits->max_builder_allocated_bytes, 9,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (grown == NULL)
		return 0;
	collector->cells = grown;
	memset(collector->cells, 0, columns * sizeof(*collector->cells));
	collector->cell_count = columns;
	layout = span->layout->first;
	data = span->first;
	for (index = 0; layout != NULL; layout = layout->next) {
		if (layout->col < 0 || (uint32_t)layout->col >= columns ||
		    (previous != UINT32_MAX &&
		    (uint32_t)layout->col <= previous)) {
			fail_relation(collector, layout->col, columns);
			return 0;
		}
		index = (uint32_t)layout->col;
		previous = index;
		current = data != NULL && data->layout == layout ? data : NULL;
		if (current != NULL)
			data = data->next;
		if (text_cell(layout, current)) {
			/* tbl_term.c::tbl_word() is the only authored text
			 * emitter.  Even when it is skipped for an empty cell,
			 * term_tbl() later reports that cell's native column. */
			key = add_mark(collector, node, NULL,
			    MANT_ANNOTATED_MARK_REGION,
			    collector->active_owner,
			    MANT_ANNOTATED_REGION_TABLE_CELL, NULL);
			if (key == 0)
				return 0;
			mark = collector->marks + key - 1;
			mark->source = source_key(node);
			mark->table_column = index;
			collector->cells[index].mark = key;
			collector->cells[index].data = current;
		}
	}
	return 1;
}

static void
finish_table_span(struct mant_annotated_collector *collector,
    const struct roff_node *node)
{
	uint32_t index, key;

	if (collector->table_node != node || !collector->table_prepared ||
	    collector->active_cell != NULL) {
		fail_relation(collector, collector->frame_count, 0);
		return;
	}
	for (index = 0; index < collector->cell_count; index++) {
		key = collector->cells[index].mark;
		if (key != 0 &&
		    collector->marks[key - 1].table_position_present == 0) {
			fail_relation(collector, key, 0);
			return;
		}
	}
	collector->table_node = NULL;
	collector->table_prepared = 0;
	collector->cell_count = 0;
}

static void
observe_table_cell(struct mant_annotated_collector *collector,
    const struct term_collector_event *event)
{
	const struct tbl_dat *data = event->cell;
	struct annotated_cell *cell;
	uint32_t column;

	if (collector->table_node == NULL || !collector->table_prepared ||
	    data == NULL || data->layout == NULL ||
	    data->layout->col < 0) {
		fail_relation(collector, collector->cell_count, 0);
		return;
	}
	column = (uint32_t)data->layout->col;
	if (column >= collector->cell_count) {
		fail_relation(collector, column, collector->cell_count);
		return;
	}
	cell = collector->cells + column;
	if (cell->mark == 0 || cell->data != data) {
		fail_relation(collector, column, cell->mark);
		return;
	}
	if (event->phase == TERM_COLLECT_ENTER) {
		if (collector->active_cell != NULL) {
			fail_relation(collector, column, 0);
			return;
		}
		collector->active_cell = data;
		collector->cell_saved_owner = collector->active_owner;
		collector->active_owner = cell->mark;
	} else if (event->phase == TERM_COLLECT_LEAVE) {
		if (collector->active_cell != data ||
		    collector->active_owner != cell->mark) {
			fail_relation(collector, column, 0);
			return;
		}
		collector->active_owner = collector->cell_saved_owner;
		collector->cell_saved_owner = 0;
		collector->active_cell = NULL;
	} else
		fail_relation(collector, event->phase, TERM_COLLECT_LEAVE);
}

static void
observe_table_cell_position(struct mant_annotated_collector *collector,
    const struct term_collector_event *event)
{
	struct mant_annotated_mark *mark;
	struct annotated_point_state *state;
	uint32_t key;

	if (collector->table_node == NULL || !collector->table_prepared ||
	    collector->active_cell != NULL || event->column >=
	    collector->cell_count) {
		fail_relation(collector, event->column, collector->cell_count);
		return;
	}
	key = collector->cells[event->column].mark;
	if (key == 0)
		return; /* Native rule or span column, not a text cell. */
	mark = collector->marks + key - 1;
	state = collector->points + key - 1;
	if (mark->table_position_present != 0 || state->state != 0) {
		fail_relation(collector, key, 0);
		return;
	}
	/* tbl_term.c::term_tbl reports every native text column on its first
	 * physical line, even when tbl_data() emitted no word.  The current
	 * display cursor is a real device boundary; the table offset remains
	 * a separate BU hint and is never reinterpreted as a display column. */
	if (!mant_annotated_display_checkpoint(collector->display,
	    collector->advance_count, &state->checkpoint)) {
		fail_relation(collector, key, 0);
		return;
	}
	state->state = 2;
	mark->table_position_present = 1;
	mark->table_offset = event->pos;
}

static void
pop_node(struct mant_annotated_collector *collector,
    const struct roff_node *node)
{
	struct annotated_frame *frame, *parent_frame;
	const struct mant_annotated_mark *owner;
	const struct roff_node **last_node;
	uint32_t *last_owner;

	if (collector->frame_count == 0) {
		fail_relation(collector, 0, 1);
		return;
	}
	frame = collector->frames + collector->frame_count - 1;
	if (frame->node != node) {
		fail_relation(collector, collector->frame_count, 0);
		return;
	}
	if (node->type == ROFFT_TBL) {
		finish_table_span(collector, node);
		if (collector->session->status != MANT_STRUCTURED_OK)
			return;
	}
	collector->active_owner = frame->saved_owner == 0 ?
	    collector->unsectioned_region : frame->saved_owner;
	collector->active_head_component = frame->saved_head_component;
	collector->active_link = frame->saved_link;
	collector->active_link_node = frame->saved_link_node;
	collector->active_link_epoch = frame->saved_link_epoch;
	collector->active_heading = frame->saved_heading;
	collector->frame_count--;
	parent_frame = collector->frame_count == 0 ? NULL :
	    collector->frames + collector->frame_count - 1;
	if (parent_frame != NULL && parent_frame->node != node->parent)
		return; /* A skipped AST wrapper cannot prove direct adjacency. */
	last_node = parent_frame == NULL ? &collector->last_root_ip_node :
	    &parent_frame->last_direct_ip_node;
	last_owner = parent_frame == NULL ? &collector->last_root_ip_owner :
	    &parent_frame->last_direct_ip_owner;
	owner = frame->owner_mark == 0 ? NULL :
	    collector->marks + frame->owner_mark - 1;
	if (node->type == ROFFT_BLOCK && node->tok == MAN_IP &&
	    owner != NULL &&
	    (owner->flags & MANT_ANNOTATED_MARK_DEFINITION) != 0) {
		*last_node = node;
		*last_owner = frame->owner_mark;
	} else if (!ip_sibling_gap(node)) {
		*last_node = NULL;
		*last_owner = 0;
	}
}

struct mant_annotated_collector *
mant_annotated_collector_new(struct structured_session *session,
    struct mant_annotated_display *display)
{
	struct mant_annotated_collector *collector;

	if (session == NULL || display == NULL || session->limits == NULL ||
	    session->status != MANT_STRUCTURED_OK)
		return NULL;
	collector = mant_structured_allocate(session, sizeof(*collector), 1,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (collector == NULL)
		return NULL;
	collector->session = session;
	collector->display = display;
	collector->allocated_display_bytes =
	    mant_annotated_display_allocated_bytes(display);
	collector->accounted_display_work = mant_annotated_display_work(display);
	return collector;
}

int
mant_annotated_collector_account_display(
    struct mant_annotated_collector *collector)
{
	struct structured_session *session;
	uint64_t allocated, work, added_allocated;

	if (collector == NULL)
		return 0;
	session = collector->session;
	allocated = mant_annotated_display_allocated_bytes(
	    collector->display);
	work = mant_annotated_display_work(collector->display);
	if (allocated < collector->allocated_display_bytes ||
	    work < collector->accounted_display_work) {
		fail_relation(collector, allocated, work);
		return 0;
	}
	added_allocated = allocated - collector->allocated_display_bytes;
	collector->allocated_display_bytes = allocated;
	collector->accounted_display_work = work;
	/* The display's work callback charged the shared counter *before* each
	 * normalization step.  This post-write account only covers allocations. */
	return mant_structured_charge(session,
	    &session->allocated_bytes, added_allocated,
	    session->limits->max_builder_allocated_bytes, 9,
	    MANT_STRUCTURED_STAGE_RENDER);
}

static int
report_display_failure(struct mant_annotated_collector *collector)
{
	struct structured_session *session = collector->session;
	enum mant_annotated_display_status status;
	uint64_t observed, allowed;
	uint32_t limit_kind;

	status = mant_annotated_display_status(collector->display);
	if (status == MANT_ANNOTATED_DISPLAY_BUDGET) {
		mant_annotated_display_failure(collector->display,
		    &limit_kind, &observed, &allowed);
		if (limit_kind != 0 && observed > allowed)
			mant_structured_set_failure(session,
			    MANT_STRUCTURED_BUDGET,
			    MANT_STRUCTURED_STAGE_RENDER, limit_kind,
			    observed, allowed);
		else
			fail_relation(collector, observed, allowed);
	} else if (status == MANT_ANNOTATED_DISPLAY_ALLOC)
		mant_structured_set_failure(session, MANT_STRUCTURED_BUILDER_ALLOC,
		    MANT_STRUCTURED_STAGE_RENDER, 0, 0, 0);
	else
		fail_relation(collector, status, MANT_ANNOTATED_DISPLAY_ALLOC);
	return 0;
}

static int
write_display(struct mant_annotated_collector *collector,
    const void *bytes, size_t length,
    struct mant_annotated_display_label label,
    struct mant_annotated_display_edge edge)
{
	int written;

	if (collector->session->status != MANT_STRUCTURED_OK)
		return 0;
	written = mant_annotated_display_write_join(collector->display, bytes,
	    length, label, edge);
	if (!mant_annotated_collector_account_display(collector))
		return 0;
	return written ? 1 : report_display_failure(collector);
}

static int
flush_advances(struct mant_annotated_collector *collector, int proven_gap)
{
	struct mant_annotated_display_label label = {0};
	struct mant_annotated_display_edge edge = {0};
	struct annotated_column *column = NULL;
	static const uint8_t space = ' ';
	uint16_t index, leading;
	int can_map;

	if (collector->advance_count == 0) {
		collector->skipped_cells = 0;
		return 1;
	}
	can_map = proven_gap && collector->advance_count < 256 &&
	    collector->skipped_cells <= collector->advance_count;
	if (collector->skipped_column < collector->column_capacity)
		column = collector->columns + collector->skipped_column;
	leading = can_map ? collector->advance_count -
	    collector->skipped_cells : collector->advance_count;
	for (index = 0; index < collector->advance_count; index++) {
		label.role = collector->advance_role;
		if (can_map && index >= leading)
			label = collector->skipped[index - leading];
		else if (can_map && label.role == MANT_ANNOTATED_BODY)
			label.role = MANT_ANNOTATED_LAYOUT;
		/* An emitted field blank with no glyph origin or authored
		 * attribution is formatter spacing.  It remains visible on the
		 * surface, but cannot interrupt a logical text join. */
		if (label.role == MANT_ANNOTATED_BODY &&
		    label.owner == 0 && label.link == 0 && label.source == 0 &&
		    label.head_component == 0 &&
		    label.glyph_origin == 0 && label.style == 0 &&
		    label.flags == 0)
			label.role = MANT_ANNOTATED_LAYOUT;
		if (column != NULL && label.glyph_origin != 0)
			edge = origin_edge(column, label.glyph_origin);
		else
			memset(&edge, 0, sizeof(edge));
		if ((label.role == MANT_ANNOTATED_BODY ||
		    label.role == MANT_ANNOTATED_DIRECT_DRAW) &&
		    label.glyph_origin == 0)
			collector->metrics.unverified_placements++;
		if (!write_display(collector, &space, 1, label, edge))
			return 0;
		if (column != NULL && label.glyph_origin != 0)
			commit_origin(column, label.glyph_origin, edge);
		else if (column != NULL && can_map && index >= leading)
			join_unknown(column);
		memset(&label, 0, sizeof(label));
	}
	if (!can_map && collector->skipped_cells != 0 && column != NULL)
		join_unknown(column);
	collector->advance_count = collector->skipped_cells = 0;
	collector->advance_role = 0;
	collector->skipped_column = 0;
	return 1;
}

/* term.c::term_field calls p->advance(vbl) before reporting FIELD_PLACE,
 * but the sink retains those blanks until the following LETTER.  Resolve
 * buffer gaps after they reach the device and before the glyph, collapsing
 * discarded skip cells onto the last visible column. */
static int
capture_field_points(struct mant_annotated_collector *collector,
    uint16_t emitted_advances)
{
	struct mant_annotated_display_checkpoint checkpoint, at;
	struct annotated_column *column;
	uint64_t total_cells, emitted_cells, prefix, cells;
	uint32_t index;

	if (!collector->letter_from_field ||
	    collector->letter_column >= collector->column_capacity)
		return 1;
	column = collector->columns + collector->letter_column;
	if (!mant_annotated_display_checkpoint(collector->display, 0,
	    &checkpoint)) {
		fail_relation(collector, collector->letter_pos, 0);
		return 0;
	}
	if (column->skipping) {
		if (column->skip_cell_width == 0 ||
		    column->skipped_visual % column->skip_cell_width != 0 ||
		    column->skip_start > collector->letter_pos) {
			fail_relation(collector, column->skipped_visual,
			    column->skip_cell_width);
			return 0;
		}
		/* FIELD_SKIP events paid for the first traversal; resolving their
		 * final device gaps is a second, separately charged traversal. */
		if (!charge_work(collector,
		    collector->letter_pos - column->skip_start))
			return 0;
		total_cells = column->skipped_visual /
		    column->skip_cell_width;
		emitted_cells = total_cells < emitted_advances ?
		    total_cells : emitted_advances;
		if (emitted_cells > checkpoint.column) {
			fail_relation(collector, emitted_cells, checkpoint.column);
			return 0;
		}
		prefix = 0;
		for (index = column->skip_start;
		    index < collector->letter_pos; index++) {
			if (index >= column->capacity) {
				fail_relation(collector, index, column->capacity);
				return 0;
			}
			at = checkpoint;
			at.column = checkpoint.column - (uint32_t)emitted_cells +
			    (uint32_t)(prefix < emitted_cells ? prefix : emitted_cells);
			if (!capture_point_slot(collector,
			    column->slots + index, at))
				return 0;
			cells = column->slots[index].skipped_visual /
			    column->skip_cell_width;
			prefix += cells;
			column->slots[index].skipped_visual = 0;
		}
		column->skipping = 0;
		column->skipped_visual = 0;
		column->skip_cell_width = 0;
	}
	if (!capture_point_slot(collector,
	    column->slots + collector->letter_pos, checkpoint))
		return 0;
	collector->letter_from_field = 0;
	return 1;
}

static void
record_field_skip(struct mant_annotated_collector *collector,
    struct termp *p, const struct term_collector_event *event)
{
	struct mant_annotated_display_label label = {0};
	struct annotated_column *column;
	struct annotated_slot *slot;
	size_t width, cells;
	uint16_t index;

	if (event->visual == 0) {
		if (event->value == '\n' &&
		    event->column < collector->column_capacity)
			join_hard(collector->columns + event->column);
		return;
	}
	collector->skipped_column = event->column;
	if (collector->skipped_cells > 256)
		return;
	width = (*p->getwidth)(p, ' ');
	if (width == 0 || event->visual % width != 0) {
		collector->skipped_cells = 257;
		return;
	}
	cells = event->visual / width;
	label.role = current_role(collector);
	if (event->column < collector->column_capacity) {
		column = collector->columns + event->column;
		if (event->pos < column->capacity) {
			slot = column->slots + event->pos;
			if (slot->occupied && slot->value == event->value) {
				label.owner = slot->owner;
				label.link = slot->link;
				label.source = slot->source;
				label.head_component = slot->head_component;
				label.glyph_origin = slot->origin;
				if (slot->layout_space && label.role ==
				    MANT_ANNOTATED_BODY)
					label.role = MANT_ANNOTATED_LAYOUT;
			}
		}
	}
	if (cells > (size_t)(256U - collector->skipped_cells)) {
		collector->skipped_cells = 257;
		return;
	}
	for (index = 0; index < cells; index++)
		collector->skipped[collector->skipped_cells++] = label;
}

void
mant_annotated_collector_observe(struct termp *p, void *argument,
    const struct term_collector_event *event)
{
	struct mant_annotated_collector *collector = argument;
	struct annotated_column *column;
	struct annotated_slot *slot;
	size_t index, end;

	(void)p;
	if (collector == NULL || event == NULL ||
	    collector->session->status != MANT_STRUCTURED_OK ||
	    mant_mandoc_output_active_failed())
		return;
	if (!charge_work(collector, 1))
		return;
	switch (event->op) {
	case TERM_COLLECT_NODE:
		if (event->phase == TERM_COLLECT_ENTER)
			(void)push_node(collector, event->node);
		else if (event->phase == TERM_COLLECT_CHILD &&
		    event->node != NULL &&
		    event->node->type == ROFFT_TBL)
			(void)prepare_table_span(collector, event->node);
		else if (event->phase == TERM_COLLECT_LEAVE)
			pop_node(collector, event->node);
		return;
	case TERM_COLLECT_TAG_POINT:
	case TERM_COLLECT_OWNER_POINT:
	case TERM_COLLECT_REGION_POINT:
		arm_point(collector, p, event);
		return;
	case TERM_COLLECT_TABLE_CELL:
		if (event->column < collector->column_capacity)
			join_hard(collector->columns + event->column);
		observe_table_cell(collector, event);
		return;
	case TERM_COLLECT_TABLE_CELL_POSITION:
		observe_table_cell_position(collector, event);
		return;
	case TERM_COLLECT_OUTPUT:
		if (event->reason == TERM_COLLECT_HEADER) {
			collector->in_header = event->phase == TERM_COLLECT_ENTER;
		} else if (event->reason == TERM_COLLECT_FOOTER) {
			collector->in_footer = event->phase == TERM_COLLECT_ENTER;
			collector->footer_drained = 0;
		} else if (event->reason == TERM_COLLECT_MARGIN) {
			collector->in_margin = event->phase == TERM_COLLECT_ENTER;
			collector->margin_owner = 0;
			collector->margin_mark = 0;
			if (collector->in_margin &&
			    current_role(collector) == MANT_ANNOTATED_BODY &&
			    !mant_annotated_display_trailing_owner(
			    collector->display, &collector->margin_owner)) {
				(void)mant_annotated_collector_account_display(collector);
				if (collector->session->status == MANT_STRUCTURED_OK)
					(void)report_display_failure(collector);
			} else if (collector->in_margin)
				(void)mant_annotated_collector_account_display(collector);
		}
		return;
	case TERM_COLLECT_VSPACE_DRAIN:
		/* term_vspace() flushes the prior body in term_newln() first. */
		if (collector->in_footer)
			collector->footer_drained = 1;
		if (event->column < collector->column_capacity)
			join_hard(collector->columns + event->column);
		return;
	case TERM_COLLECT_ENDLINE:
		if (event->reason != TERM_COLLECT_WRAP &&
		    event->column < collector->column_capacity)
			join_hard(collector->columns + event->column);
		return;
	case TERM_COLLECT_LOGICAL:
		/* term.c::endline() writes .mc independently of the field.  Even
		 * Unicode margin escapes must not become authored join evidence. */
		if (collector->in_margin) {
			collector->pending_origin = 0;
			collector->pending_owner = 0;
			collector->pending_link = 0;
			collector->pending_source = 0;
			collector->pending_head_component = 0;
			return;
		}
		if (collector->next_origin == UINT64_MAX) {
			mant_structured_set_failure(collector->session,
			    MANT_STRUCTURED_BUDGET, MANT_STRUCTURED_STAGE_RENDER,
			    29, UINT64_MAX, UINT64_MAX - 1);
			return;
		}
		collector->pending_origin = ++collector->next_origin;
		collector->pending_owner = collector->active_owner;
		/* term.c::term_word() writes AUTO_SPACE before each operand.
		 * The first is macro-leading padding; later spaces within one Cm/Ic
		 * are part of that macro's displayed phrase.  Fl's generated dash
		 * is a logical glyph and starts its own component. */
		collector->pending_head_component =
		    collector->active_head_component;
		if (collector->pending_head_component != 0) {
			struct annotated_point_state *component = collector->points +
			    collector->pending_head_component - 1;

			if (event->reason == TERM_COLLECT_AUTO_SPACE &&
			    !component->component_started)
				collector->pending_head_component = 0;
			else if (event->reason != TERM_COLLECT_AUTO_SPACE &&
			    event->value != ASCII_NBRZW &&
			    event->value != ASCII_BREAK &&
			    event->value != '\n')
				component->component_started = 1;
		}
		collector->pending_link = visible_link(collector,
		    event->node, event->reason);
		collector->pending_source = source_key(event->node);
		if (collector->active_cell != NULL &&
		    collector->pending_source == 0)
			collector->pending_source =
			    collector->marks[collector->active_owner - 1].source;
		if (event->reason == TERM_COLLECT_AUTO_SPACE)
			collector->pending_source = 0;
		if (collector->pending_source >
		    collector->session->result->source_count) {
			fail_relation(collector, collector->pending_source,
			    collector->session->result->source_count);
			return;
		}
		collector->metrics.glyph_origins++;
		return;
	case TERM_COLLECT_BUFFER_WRITE:
		if (event->end != event->pos + 1 ||
		    !charge_mutations(collector, 1)) {
			if (collector->session->status == MANT_STRUCTURED_OK)
				fail_relation(collector, event->end, event->pos + 1);
			return;
		}
		/* term_fill() can replace a glyph by its display hyphen. */
		if (event->reason == TERM_COLLECT_NORMALIZE) {
			/* A non-breaking space may have reused an existing blank
			 * buffer position without a BUFFER_WRITE and remains a
			 * FIELD_SKIP, so no glyph identity is required there. */
			if (event->column < collector->column_capacity) {
				column = collector->columns + event->column;
				if (event->pos < column->capacity) {
					slot = column->slots + event->pos;
					if (slot->occupied)
						slot->value = event->value;
				}
			}
			return;
		}
		column = column_at(collector, event->column);
		if (column == NULL ||
		    (slot = slot_at(collector, column, event->pos)) == NULL)
			return;
		if (!slot->occupied) {
			column->live++;
			collector->live_slots++;
			if (collector->metrics.peak_live_slots <
			    collector->live_slots)
				collector->metrics.peak_live_slots =
				    collector->live_slots;
		}
		slot->origin = event->reason == TERM_COLLECT_HORIZ ||
		    event->reason == TERM_COLLECT_FIELD ? 0 :
		    collector->pending_origin;
		slot->owner = event->reason == TERM_COLLECT_HORIZ ||
		    event->reason == TERM_COLLECT_FIELD ? 0 :
		    collector->pending_owner;
		slot->link = event->reason == TERM_COLLECT_HORIZ ||
		    event->reason == TERM_COLLECT_FIELD ? 0 :
		    collector->pending_link;
		slot->source = event->reason == TERM_COLLECT_HORIZ ||
		    event->reason == TERM_COLLECT_FIELD ? 0 :
		    collector->pending_source;
		slot->head_component = event->reason == TERM_COLLECT_HORIZ ||
		    event->reason == TERM_COLLECT_FIELD ? 0 :
		    collector->pending_head_component;
		slot->flags = event->reason == TERM_COLLECT_FONT ?
		    MANT_ANNOTATED_FONT_STROKE : 0;
		slot->value = event->value;
		slot->authored_space = event->reason == TERM_COLLECT_TEXT &&
		    event->value == ' ' && collector->pending_source != 0;
		slot->generated_space = event->reason ==
		    TERM_COLLECT_AUTO_SPACE && event->value == ' ';
		slot->layout_space = event->value == ' ' &&
		    (event->reason == TERM_COLLECT_HORIZ ||
		    event->reason == TERM_COLLECT_FIELD);
		slot->occupied = 1;
		return;
	case TERM_COLLECT_BUFFER_CURSOR:
		/* bufferc() may advance across an existing blank without writing.
		 * A later logical origin cannot inherit the blank's provenance. */
		if (event->end == event->pos + 1 &&
		    event->column < collector->column_capacity) {
			column = collector->columns + event->column;
			if (event->pos < column->capacity) {
				slot = column->slots + event->pos;
				if (slot->occupied && slot->value == ' ') {
					/* A matching cursor is the second half of
					 * bufferc()'s own write, not blank reuse. */
					if (slot->origin != collector->pending_origin ||
					    event->reason != TERM_COLLECT_TEXT)
						slot->authored_space = 0;
					if (slot->origin != collector->pending_origin ||
					    event->reason != TERM_COLLECT_AUTO_SPACE)
						slot->generated_space = 0;
					/* bufferc() can traverse a previously written
				 * HORIZ/FIELD blank without replacing it.  The
				 * cell remains formatter layout even after its
				 * logical origin is no longer current. */
				}
			}
		}
		return;
	case TERM_COLLECT_FIELD_PLACE:
		if (collector->letter_pending) {
			fail_relation(collector, event->pos, event->column);
			return;
		}
		if (event->column >= collector->column_capacity ||
		    event->pos >= collector->columns[event->column].capacity) {
			fail_relation(collector, event->pos, event->column);
			return;
		}
		column = collector->columns + event->column;
		if (column->skipping) {
			size_t cell_width = (*p->getwidth)(p, ' ');

			if (cell_width == 0 || cell_width > UINT32_MAX) {
				fail_relation(collector, cell_width, UINT32_MAX);
				return;
			}
			column->skip_cell_width = cell_width;
		}
		slot = column->slots + event->pos;
		if (!slot->occupied || slot->value != event->value) {
			fail_relation(collector, event->value, slot->value);
			return;
		}
		memset(&collector->letter_label, 0,
		    sizeof(collector->letter_label));
		collector->letter_label.role = current_role(collector);
		collector->letter_label.glyph_origin = slot->origin;
		collector->letter_label.owner = slot->owner;
		collector->letter_label.link = slot->link;
		collector->letter_label.source = slot->source;
		collector->letter_label.head_component = slot->head_component;
		collector->letter_label.flags = slot->flags;
		/* term.c::term_field() called p->advance(vbl) before FIELD_PLACE,
		 * but the sink still retains those blanks.  Bind the edge after
		 * LETTER flushes them and updates the origin. */
		collector->letter_edge = (struct mant_annotated_display_edge){0};
		collector->letter_column = event->column;
		collector->letter_pos = event->pos;
		collector->letter_pending = 1;
		collector->letter_from_field = 1;
		collector->metrics.field_placements++;
		return;
	case TERM_COLLECT_DIRECT:
		if (event->value == ASCII_BREAK ||
		    event->value == ASCII_NBRZW)
			return;
		if (collector->letter_pending) {
			fail_relation(collector, event->value, 0);
			return;
		}
		memset(&collector->letter_label, 0,
		    sizeof(collector->letter_label));
		collector->letter_label.role = current_role(collector);
		column = column_at(collector, event->column);
		if (column == NULL)
			return;
		collector->letter_label.glyph_origin =
		    event->reason == TERM_COLLECT_HORIZ ||
		    event->reason == TERM_COLLECT_FIELD ? 0 :
		    collector->pending_origin;
		if (event->reason != TERM_COLLECT_HORIZ &&
		    event->reason != TERM_COLLECT_FIELD) {
			collector->letter_label.owner =
			    collector->pending_owner;
			collector->letter_label.link =
			    collector->pending_link;
			collector->letter_label.source =
			    collector->pending_source;
			collector->letter_label.head_component =
			    collector->pending_head_component;
		}
		if (collector->in_margin) {
			collector->letter_label.glyph_origin = 0;
			collector->letter_label.owner = collector->margin_owner;
			collector->letter_label.link = 0;
			collector->letter_label.source = 0;
			collector->letter_label.head_component = 0;
		}
		collector->letter_edge = origin_edge(column,
		    collector->letter_label.glyph_origin);
		collector->letter_column = event->column;
		collector->letter_pending = 1;
		collector->letter_from_field = 0;
		return;
	case TERM_COLLECT_DRAW:
		if (collector->letter_pending) {
			fail_relation(collector, event->value, 0);
			return;
		}
		memset(&collector->letter_label, 0,
		    sizeof(collector->letter_label));
		collector->letter_label.role = current_role(collector) ==
		    MANT_ANNOTATED_BODY ? MANT_ANNOTATED_DIRECT_DRAW :
		    current_role(collector);
		collector->letter_label.owner = collector->active_owner;
		collector->letter_label.link = visible_link(collector,
		    event->node, event->reason);
		collector->letter_label.source = source_key(event->node);
		collector->letter_label.head_component =
		    collector->active_head_component;
		collector->letter_edge = (struct mant_annotated_display_edge){0};
		collector->letter_column = event->column;
		collector->letter_pending = 1;
		collector->letter_from_field = 0;
		collector->metrics.direct_draws++;
		return;
	case TERM_COLLECT_FIELD_SKIP:
		column = column_at(collector, event->column);
		if (column == NULL ||
		    (slot = slot_at(collector, column, event->pos)) == NULL)
			return;
		if (!column->skipping) {
			column->skip_start = event->pos;
			column->skipped_visual = 0;
			column->skipping = 1;
		}
		if (event->pos < column->skip_start ||
		    event->visual > UINT64_MAX - column->skipped_visual) {
			fail_relation(collector, event->pos,
			    column->skip_start);
			return;
		}
		slot->skipped_visual = event->visual;
		column->skipped_visual += event->visual;
		record_field_skip(collector, p, event);
		return;
	case TERM_COLLECT_BUFFER_CONSUME:
	case TERM_COLLECT_BUFFER_RESET:
	case TERM_COLLECT_BUFFER_TRUNCATE:
		if (!flush_advances(collector, 0))
			return;
		if (event->pos > event->end) {
			fail_relation(collector, event->pos, event->end);
			return;
		}
		if (event->column < collector->column_capacity) {
			column = collector->columns + event->column;
			index = event->pos < column->capacity ? event->pos :
			    column->capacity;
			end = event->end < column->capacity ? event->end :
			    column->capacity;
			if (!charge_mutations(collector, end - index) ||
			    !charge_work(collector, end - index))
				return;
			if (event->op == TERM_COLLECT_BUFFER_CONSUME &&
			    event->reason == TERM_COLLECT_WRAP) {
				for (size_t cell = index; cell < end; cell++) {
					slot = column->slots + cell;
					if (slot->occupied && slot->value == ' ' &&
					    slot->origin != 0) {
						if (!join_wrap_space(collector, column,
						    slot))
							return;
					} else
						join_unknown(column);
				}
			}
			if (event->op == TERM_COLLECT_BUFFER_TRUNCATE) {
				uint32_t head = 0, key, next;
				for (size_t cell = index; cell < end; cell++) {
					for (key = column->slots[cell].first_point;
					    key != 0; key = next) {
						next = collector->points[key - 1].next;
						collector->points[key - 1].next = head;
						head = key;
					}
					column->slots[cell].first_point = 0;
				}
				discard_range(collector, column, index, end);
				if (head != 0) {
					slot = slot_at(collector, column, event->pos);
					if (slot == NULL)
						return;
					slot->first_point = head;
				}
			} else {
				size_t point_end = event->op ==
				    TERM_COLLECT_BUFFER_RESET &&
				    end < column->capacity ? end + 1 : end;
				if (!capture_point_range(collector, column,
				    index, point_end))
					return;
				discard_range(collector, column, index, end);
				if (event->op == TERM_COLLECT_BUFFER_RESET &&
				    end < column->capacity)
					column->slots[end].first_point = 0;
			}
			column->skipping = 0;
			column->skipped_visual = 0;
		}
		if (event->op == TERM_COLLECT_BUFFER_RESET)
			collector->pending_origin = collector->pending_owner =
			    collector->pending_link = collector->pending_source =
			    collector->pending_head_component = 0;
		return;
	case TERM_COLLECT_COL_FREE:
		if (event->column >= collector->column_capacity)
			return;
		column = collector->columns + event->column;
		if (!capture_point_range(collector, column, 0,
		    column->capacity))
			return;
		collector->live_slots -= column->live;
		collector->slot_bytes -=
		    (uint64_t)column->capacity * sizeof(*column->slots);
		free(column->slots);
		memset(column, 0, sizeof(*column));
		return;
	default:
		return;
	}
}

int
mant_annotated_collector_sink(void *argument, const void *bytes,
    size_t length)
{
	struct mant_annotated_collector *collector = argument;
	struct mant_annotated_display_label label = {0};
	struct mant_annotated_display_edge edge = {0};
	struct annotated_column *column;
	enum mant_mandoc_output_operation operation;
	uint16_t emitted_advances;
	int from_field, written;

	if (collector == NULL ||
	    collector->session->status != MANT_STRUCTURED_OK)
		return 0;
	operation = mant_mandoc_output_current_operation();
	from_field = collector->letter_from_field;
	label.role = current_role(collector);
	/* With no FIELD_SKIP, term_field()'s advance is its own vbl
	 * indentation (or endline margin spacing), never buffer text. */
	if (operation == MANT_OUTPUT_ADVANCE &&
	    collector->skipped_cells == 0 &&
	    label.role == MANT_ANNOTATED_BODY)
		label.role = MANT_ANNOTATED_LAYOUT;
	if (operation == MANT_OUTPUT_ADVANCE &&
	    collector->skipped_cells != 0 &&
	    label.role == MANT_ANNOTATED_BODY) {
		/* term_ascii.c::ascii_advance() emits at most 256 spaces.
		 * Delay them until FIELD_PLACE establishes the exact emitted
		 * count; only then can input whitespace be distinguished from
		 * the field's leading layout indentation. */
		if (length != 1 || bytes == NULL ||
		    ((const uint8_t *)bytes)[0] != ' ' ||
		    collector->advance_count == 256) {
			fail_relation(collector, length, 1);
			return 0;
		}
		collector->advance_role = label.role;
		collector->advance_count++;
		return 1;
	}
	emitted_advances = collector->advance_count;
	if (operation != MANT_OUTPUT_ADVANCE) {
		if (!flush_advances(collector,
		    operation == MANT_OUTPUT_LETTER &&
		    collector->letter_from_field))
			return 0;
		if (operation == MANT_OUTPUT_LETTER &&
		    !capture_field_points(collector, emitted_advances))
			return 0;
	}
	if (operation == MANT_OUTPUT_LETTER) {
		if (!collector->letter_pending) {
			fail_relation(collector, operation, 0);
			return 0;
		}
		label = collector->letter_label;
		if (collector->in_margin && collector->margin_owner != 0 &&
		    label.role == MANT_ANNOTATED_BODY &&
		    !(length == 1 && bytes != NULL &&
		    ((const uint8_t *)bytes)[0] == '\b')) {
			if (collector->margin_mark == 0) {
				struct annotated_point_state *state;
				/* term.c::endline() emits .mc after the field.  Give
				 * its generated display bytes their own region so a
				 * heading title or definition head stays semantic-only. */
				collector->margin_mark = add_mark(collector, NULL,
				    NULL, MANT_ANNOTATED_MARK_REGION,
				    collector->margin_owner,
				    MANT_ANNOTATED_REGION_MARGIN, NULL);
				if (collector->margin_mark == 0)
					return 0;
				collector->marks[collector->margin_mark - 1].owner =
				    collector->margin_owner;
				state = collector->points +
				    collector->margin_mark - 1;
				if (!mant_annotated_display_checkpoint(
				    collector->display, 0, &state->checkpoint)) {
					fail_relation(collector,
					    collector->margin_mark, 0);
					return 0;
				}
				state->state = 2;
			}
			label.owner = collector->margin_mark;
		}
		if (from_field) {
			if (collector->letter_column >=
			    collector->column_capacity) {
				fail_relation(collector,
				    collector->letter_column,
				    collector->column_capacity);
				return 0;
			}
			column = collector->columns +
			    collector->letter_column;
			/* The pending field blanks were emitted and committed above.
			 * Join the current glyph to that final predecessor, not to
			 * the predecessor seen at FIELD_PLACE. */
			edge = origin_edge(column, label.glyph_origin);
		} else
			edge = collector->letter_edge;
		collector->letter_pending = 0;
		if ((label.role == MANT_ANNOTATED_BODY ||
		    label.role == MANT_ANNOTATED_DIRECT_DRAW) &&
		    label.glyph_origin == 0)
			collector->metrics.unverified_placements++;
	} else if (operation != MANT_OUTPUT_ADVANCE &&
	    operation != MANT_OUTPUT_ENDLINE) {
		fail_relation(collector, operation, MANT_OUTPUT_ENDLINE);
		return 0;
	}
	written = write_display(collector, bytes, length, label, edge);
	if (!written || operation != MANT_OUTPUT_LETTER ||
	    collector->letter_column >= collector->column_capacity ||
	    (label.role != MANT_ANNOTATED_BODY &&
	    label.role != MANT_ANNOTATED_DIRECT_DRAW) ||
	    (length == 1 && bytes != NULL &&
	    ((const uint8_t *)bytes)[0] == '\b'))
		return written;
	column = collector->columns + collector->letter_column;
	commit_origin(column, label.glyph_origin, edge);
	return written;
}

void
mant_annotated_collector_get_metrics(
    const struct mant_annotated_collector *collector,
    struct mant_annotated_collector_metrics *metrics)
{
	if (metrics == NULL)
		return;
	memset(metrics, 0, sizeof(*metrics));
	if (collector != NULL)
		*metrics = collector->metrics;
}

void
mant_annotated_collector_get_marks(
    const struct mant_annotated_collector *collector,
    const struct mant_annotated_mark **marks, uint32_t *count)
{
	if (marks != NULL)
		*marks = collector == NULL ? NULL : collector->marks;
	if (count != NULL)
		*count = collector == NULL ? 0 : collector->mark_count;
}

void
mant_annotated_collector_take_marks(
    struct mant_annotated_collector *collector,
    struct mant_annotated_mark **marks, uint32_t *count)
{
	if (marks != NULL)
		*marks = collector == NULL ? NULL : collector->marks;
	if (count != NULL)
		*count = collector == NULL ? 0 : collector->mark_count;
	if (collector != NULL && marks != NULL && count != NULL) {
		collector->marks = NULL;
		collector->mark_count = collector->mark_capacity = 0;
	}
}

int
mant_annotated_collector_finish_points(struct mant_annotated_collector *collector,
    const struct mant_annotated_display_view *display)
{
	struct mant_annotated_mark *mark;
	struct annotated_point_state *state;
	uint32_t index;

	if (collector == NULL || display == NULL ||
	    collector->pending_point_count != 0) {
		if (collector != NULL)
			fail_relation(collector, collector->pending_point_count, 0);
		return 0;
	}
	/* This pass visits every mark, including those without point state. */
	if (!charge_work(collector, collector->mark_count))
		return 0;
	for (index = 0; index < collector->mark_count; index++) {
		mark = collector->marks + index;
		state = collector->points + index;
		if (state->state == 0) {
			if (mark->kind == MANT_ANNOTATED_MARK_ANCHOR ||
			    mark->kind == MANT_ANNOTATED_MARK_REGION) {
				fail_relation(collector, mark->key, 0);
				return 0;
			}
			continue;
		}
		if (state->state != 2 || state->next != 0 ||
		    state->checkpoint.row_before > display->row_count) {
			fail_relation(collector, mark->key, display->row_count);
			return 0;
		}
		if (state->checkpoint.row_before == display->row_count) {
			mark->point_kind = MANT_ANNOTATED_POINT_DOCUMENT_END;
			mark->point_row = display->row_count;
		} else {
			const struct mant_annotated_display_row *row =
			    display->rows + state->checkpoint.row_before;
			mark->point_kind = MANT_ANNOTATED_POINT_ROW_COLUMN;
			mark->point_row = state->checkpoint.row_before + 1;
			/* A trailing native advance may be trimmed by finish_row().
			 * In that case the final zero-width boundary is row end. */
			mark->point_column = state->checkpoint.active ?
			    state->checkpoint.column : 0;
			if (mark->point_column > row->column_count)
				mark->point_column = row->column_count;
		}
		mark->flags &= ~MANT_ANNOTATED_MARK_FINAL_POINT_UNVERIFIED;
	}
	return 1;
}

void
mant_annotated_marks_free(struct mant_annotated_mark *marks, uint32_t count)
{
	uint32_t index;

	if (marks == NULL)
		return;
	for (index = 0; index < count; index++)
	{
		free((void *)marks[index].name);
		free((void *)marks[index].target_a.ptr);
		free((void *)marks[index].target_b.ptr);
	}
	free(marks);
}

void
mant_annotated_collector_free(struct mant_annotated_collector *collector)
{
	uint32_t index;

	if (collector == NULL)
		return;
	for (index = 0; index < collector->column_capacity; index++)
		free(collector->columns[index].slots);
	mant_annotated_marks_free(collector->marks, collector->mark_count);
	free(collector->points);
	free(collector->cells);
	free(collector->frames);
	free(collector->columns);
	free(collector);
}
