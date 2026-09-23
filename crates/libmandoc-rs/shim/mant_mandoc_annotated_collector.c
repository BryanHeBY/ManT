/* Active terminal labels; no historical formatter event stream is retained. */
#include "config.h"

#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "mandoc.h"
#include "roff.h"
#include "tbl.h"
#include "out.h"
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
	int value;
	uint8_t flags;
	uint8_t occupied;
	uint8_t authored_space;
	uint8_t layout_space;
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
};

struct annotated_frame {
	const struct roff_node *node;
	const struct roff_node *saved_link_node;
	uint32_t saved_owner;
	uint32_t saved_link;
	uint32_t saved_heading;
	uint64_t saved_link_epoch;
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
	struct annotated_cell *cells;
	uint32_t cell_capacity;
	uint32_t cell_count;
	const struct roff_node *table_node;
	const struct tbl_dat *active_cell;
	uint32_t cell_saved_owner;
	uint8_t table_prepared;
	uint32_t active_owner;
	uint32_t active_link;
	const struct roff_node *active_link_node;
	uint64_t active_link_epoch;
	uint64_t phrase_epoch;
	uint32_t active_heading;
	uint32_t last_top_heading;
	uint64_t next_origin;
	uint64_t pending_origin;
	uint32_t pending_owner;
	uint32_t pending_link;
	uint32_t pending_source;
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
	uint8_t letter_pending;
	uint8_t in_header;
	uint8_t in_footer;
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
	if (column->pending_join == MANT_DISPLAY_JOIN_HARD ||
	    column->pending_join == MANT_DISPLAY_JOIN_UNKNOWN)
		return 1;
	/* term_word() inserts AUTO_SPACE before the next word.  Even when
	 * bufferc() reuses a blank slot, only a surviving TEXT write can prove
	 * that a consumed WRAP space was authored. */
	if (!slot->authored_space) {
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
	column->pending_join = MANT_DISPLAY_JOIN_SEPARATOR;
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
	mark = marks + collector->mark_count;
	memset(mark, 0, sizeof(*mark));
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
	if (kind == MANT_ANNOTATED_MARK_ANCHOR) {
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

static int
push_node(struct mant_annotated_collector *collector,
    const struct roff_node *node)
{
	struct annotated_frame *frames, *frame;
	struct mant_annotated_mark *parent_mark;
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
	frame->saved_link = collector->active_link;
	frame->saved_link_node = collector->active_link_node;
	frame->saved_heading = collector->active_heading;
	frame->saved_link_epoch = collector->active_link_epoch;

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
		collector->active_heading = key;
		collector->active_owner = key;
		if (node->tok == MAN_SH || node->tok == MDOC_Sh)
			collector->last_top_heading = key;
	} else if (node->type == ROFFT_BLOCK &&
	    (node->tok == MAN_TP || node->tok == MAN_IP ||
	    node->tok == MAN_TQ ||
	    node->tok == MDOC_It)) {
		key = add_mark(collector, node, node,
		    MANT_ANNOTATED_MARK_OWNER, collector->active_owner, 0, NULL);
		if (key == 0)
			return 0;
		collector->active_owner = key;
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
		/* add_mark() may reallocate the array, so reselect parent. */
		parent_mark = collector->marks + parent - 1;
		if (node->type == ROFFT_HEAD)
			parent_mark->title_region = key;
		else
			parent_mark->body_region = key;
		collector->active_owner = key;
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
		if (add_mark(collector, node, origin,
		    MANT_ANNOTATED_MARK_ANCHOR,
		    collector->active_owner, 0, NULL) == 0)
			return 0;
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
	if (mark->table_position_present != 0) {
		fail_relation(collector, key, 0);
		return;
	}
	mark->table_position_present = 1;
	mark->table_offset = event->pos;
}

static void
pop_node(struct mant_annotated_collector *collector,
    const struct roff_node *node)
{
	struct annotated_frame *frame;

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
	collector->active_owner = frame->saved_owner;
	collector->active_link = frame->saved_link;
	collector->active_link_node = frame->saved_link_node;
	collector->active_link_epoch = frame->saved_link_epoch;
	collector->active_heading = frame->saved_heading;
	collector->frame_count--;
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
write_display(struct mant_annotated_collector *collector,
    const void *bytes, size_t length,
    struct mant_annotated_display_label label,
    struct mant_annotated_display_edge edge)
{
	struct structured_session *session = collector->session;
	enum mant_annotated_display_status status;
	uint64_t observed, allowed;
	uint32_t limit_kind;
	int written;

	if (session->status != MANT_STRUCTURED_OK)
		return 0;
	written = mant_annotated_display_write_join(collector->display, bytes,
	    length, label, edge);
	if (!mant_annotated_collector_account_display(collector))
		return 0;
	if (written)
		return 1;
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
	    collector->session->status != MANT_STRUCTURED_OK)
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
		if (collector->next_origin == UINT64_MAX) {
			mant_structured_set_failure(collector->session,
			    MANT_STRUCTURED_BUDGET, MANT_STRUCTURED_STAGE_RENDER,
			    29, UINT64_MAX, UINT64_MAX - 1);
			return;
		}
		collector->pending_origin = ++collector->next_origin;
		collector->pending_owner = collector->active_owner;
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
		slot->flags = event->reason == TERM_COLLECT_FONT ?
		    MANT_ANNOTATED_FONT_STROKE : 0;
		slot->value = event->value;
		slot->authored_space = event->reason == TERM_COLLECT_TEXT &&
		    event->value == ' ' && collector->pending_source != 0;
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
				if (slot->occupied && slot->value == ' ' &&
				    (slot->origin != collector->pending_origin ||
				    event->reason != TERM_COLLECT_TEXT)) {
					slot->authored_space = 0;
					slot->layout_space = 0;
				}
			}
		}
		return;
	case TERM_COLLECT_FIELD_PLACE:
		if (!flush_advances(collector, 1))
			return;
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
		collector->letter_label.flags = slot->flags;
		collector->letter_edge = origin_edge(column, slot->origin);
		collector->letter_column = event->column;
		collector->letter_pending = 1;
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
		}
		collector->letter_edge = origin_edge(column,
		    collector->letter_label.glyph_origin);
		collector->letter_column = event->column;
		collector->letter_pending = 1;
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
		collector->letter_edge = (struct mant_annotated_display_edge){0};
		collector->letter_column = event->column;
		collector->letter_pending = 1;
		collector->metrics.direct_draws++;
		return;
	case TERM_COLLECT_FIELD_SKIP:
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
			discard_range(collector, column, index, end);
		}
		if (event->op == TERM_COLLECT_BUFFER_RESET)
			collector->pending_origin = collector->pending_owner =
			    collector->pending_link = collector->pending_source = 0;
		return;
	case TERM_COLLECT_COL_FREE:
		if (event->column >= collector->column_capacity)
			return;
		column = collector->columns + event->column;
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
	int written;

	if (collector == NULL ||
	    collector->session->status != MANT_STRUCTURED_OK)
		return 0;
	operation = mant_mandoc_output_current_operation();
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
	if (operation != MANT_OUTPUT_ADVANCE &&
	    !flush_advances(collector, 0))
		return 0;
	if (operation == MANT_OUTPUT_LETTER) {
		if (!collector->letter_pending) {
			fail_relation(collector, operation, 0);
			return 0;
		}
		label = collector->letter_label;
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
	free(collector->cells);
	free(collector->frames);
	free(collector->columns);
	free(collector);
}
