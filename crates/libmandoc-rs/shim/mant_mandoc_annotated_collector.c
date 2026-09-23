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
#include "mant_mandoc_structured_session.h"

struct annotated_slot {
	uint64_t origin;
	uint32_t owner;
	uint32_t link;
	uint32_t source;
	int value;
	uint8_t flags;
	uint8_t occupied;
};

struct annotated_column {
	struct annotated_slot *slots;
	uint32_t capacity;
	uint32_t live;
};

struct annotated_frame {
	const struct roff_node *node;
	uint32_t saved_owner;
	uint32_t saved_link;
	uint32_t saved_heading;
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
	struct mant_annotated_display_label skipped[256];
	uint16_t skipped_cells;
	uint16_t advance_count;
	uint32_t advance_role;
	uint8_t letter_pending;
	uint8_t in_header;
	uint8_t in_footer;
	uint8_t footer_drained;
};

static void
fail_relation(struct mant_annotated_collector *collector, uint64_t observed,
    uint64_t allowed)
{
	mant_structured_set_failure(collector->session, MANT_STRUCTURED_RELATION,
	    MANT_STRUCTURED_STAGE_RENDER, 0, observed, allowed);
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

static uint32_t
add_mark(struct mant_annotated_collector *collector,
    const struct roff_node *node, const struct roff_node *origin,
    uint32_t kind, uint32_t parent, uint32_t region_kind)
{
	struct mant_annotated_mark *marks, *mark;
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
	}
	collector->metrics.mark_count = collector->mark_count;
	return mark->key;
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
	frame->saved_heading = collector->active_heading;

	/* man_macro.c::blk_imp and mdoc_macro.c::blk_full produce a block
	 * with distinct HEAD/BODY scopes.  Their terminal traversal emits
	 * ENTER/LEAVE on every scope, without requiring a formatter flush. */
	if (node->type == ROFFT_BLOCK &&
	    (node->tok == MAN_SH || node->tok == MAN_SS ||
	    node->tok == MDOC_Sh || node->tok == MDOC_Ss)) {
		parent = node->tok == MAN_SS || node->tok == MDOC_Ss ?
		    collector->last_top_heading : 0;
		key = add_mark(collector, node, node,
		    MANT_ANNOTATED_MARK_HEADING, parent, 0);
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
		    MANT_ANNOTATED_MARK_OWNER, collector->active_owner, 0);
		if (key == 0)
			return 0;
		collector->active_owner = key;
	} else if (node->type == ROFFT_BLOCK &&
	    (node->tok == MDOC_Bl || node->tok == MDOC_Bd)) {
		region_kind = node->tok == MDOC_Bl ?
		    MANT_ANNOTATED_REGION_LIST : MANT_ANNOTATED_REGION_LITERAL;
		key = add_mark(collector, node, node,
		    MANT_ANNOTATED_MARK_REGION, collector->active_owner,
		    region_kind);
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
		    region_kind);
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
		    MANT_ANNOTATED_MARK_REGION, parent, region_kind);
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
	    node->tok == MDOC_Xr || node->tok == MDOC_Sx ||
	    node->tok == MDOC_Mt)) {
		key = add_mark(collector, node, node,
		    MANT_ANNOTATED_MARK_LINK, collector->active_link, 0);
		if (key == 0)
			return 0;
		collector->active_link = key;
	}
	if ((node->flags & NODE_ID) != 0 && node->tag != NULL &&
	    node->tag[0] != '\0') {
		const struct roff_node *origin =
		    node->mant_manual_target_source == NULL ? node :
		    node->mant_manual_target_source;
		if (add_mark(collector, node, origin,
		    MANT_ANNOTATED_MARK_ANCHOR,
		    collector->active_owner, 0) == 0)
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
			    MANT_ANNOTATED_REGION_TABLE_CELL);
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
    struct mant_annotated_display_label label)
{
	struct structured_session *session = collector->session;
	enum mant_annotated_display_status status;
	uint64_t observed, allowed;
	uint32_t limit_kind;
	int written;

	if (session->status != MANT_STRUCTURED_OK)
		return 0;
	written = mant_annotated_display_write(collector->display, bytes,
	    length, label);
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
	static const uint8_t space = ' ';
	uint16_t index, leading;
	int can_map;

	if (collector->advance_count == 0) {
		collector->skipped_cells = 0;
		return 1;
	}
	can_map = proven_gap && collector->advance_count < 256 &&
	    collector->skipped_cells <= collector->advance_count;
	leading = can_map ? collector->advance_count -
	    collector->skipped_cells : collector->advance_count;
	for (index = 0; index < collector->advance_count; index++) {
		label.role = collector->advance_role;
		if (can_map && index >= leading)
			label = collector->skipped[index - leading];
		if ((label.role == MANT_ANNOTATED_BODY ||
		    label.role == MANT_ANNOTATED_DIRECT_DRAW) &&
		    label.glyph_origin == 0)
			collector->metrics.unverified_placements++;
		if (!write_display(collector, &space, 1, label))
			return 0;
		memset(&label, 0, sizeof(label));
	}
	collector->advance_count = collector->skipped_cells = 0;
	collector->advance_role = 0;
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

	if (event->visual == 0)
		return;
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
		collector->pending_link = collector->active_link;
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
		slot->occupied = 1;
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
		collector->letter_label.link = collector->active_link;
		collector->letter_label.source = source_key(event->node);
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
	enum mant_mandoc_output_operation operation;

	if (collector == NULL ||
	    collector->session->status != MANT_STRUCTURED_OK)
		return 0;
	operation = mant_mandoc_output_current_operation();
	label.role = current_role(collector);
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
	return write_display(collector, bytes, length, label);
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
		free((void *)marks[index].name);
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
