/* Active terminal buffer, point chain, field placement and output sink. */
#include "mant_mandoc_annotated_collector_private.h"

/* term.c::buffer_write() retains the effective font at insertion, but
 * term_field() reports TERM_COLLECT_FIELD_PLACE with TERMFONT_NONE.  A final
 * literal '_' otherwise makes I and B indistinguishable as two byte-identical
 * overstrike glyphs.  This evidence is used only for that final glyph. */
static uint32_t
underscore_style(uint8_t font)
{
	switch ((enum termfont)font) {
	case TERMFONT_BOLD:
		return MANT_ANNOTATED_STYLE_BOLD;
	case TERMFONT_UNDER:
		return MANT_ANNOTATED_STYLE_UNDERLINE;
	case TERMFONT_BI:
		return MANT_ANNOTATED_STYLE_BOLD |
		    MANT_ANNOTATED_STYLE_UNDERLINE;
	default:
		return 0;
	}
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

/* Reserve the point with the same one-based key as the not-yet-committed
 * mark.  Marks commit their key only after both allocations succeed. */
int
mant_annotated_buffer_reserve_point(struct mant_annotated_collector *collector,
    uint32_t index, uint32_t maximum)
{
	struct annotated_point_state *points;

	points = mant_structured_grow_array(collector->session,
	    collector->points, index, &collector->point_capacity, maximum,
	    sizeof(*points),
	    collector->session->limits->max_builder_allocated_bytes, 9,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (points == NULL)
		return 0;
	collector->points = points;
	memset(points + index, 0, sizeof(*points));
	return 1;
}

int
mant_annotated_buffer_point_is_unused(
    const struct mant_annotated_collector *collector, uint32_t key)
{
	return key != 0 && key <= collector->mark_count &&
	    collector->points[key - 1].state == 0;
}

/* Native structural checkpoints and the terminal slot chain share this one
 * point state; neither the mark writer nor a reader fabricates display rows. */
int
mant_annotated_buffer_point_now(struct mant_annotated_collector *collector,
    uint32_t key, uint16_t pending_advances)
{
	struct annotated_point_state *state;

	if (!mant_annotated_buffer_point_is_unused(collector, key)) {
		mant_annotated_fail_relation(collector, key, collector->mark_count);
		return 0;
	}
	state = collector->points + key - 1;
	if (!mant_annotated_display_checkpoint(collector->display,
	    pending_advances, &state->checkpoint)) {
		mant_annotated_fail_relation(collector, key, 0);
		return 0;
	}
	state->state = 2;
	return 1;
}

static struct annotated_column *
column_at(struct mant_annotated_collector *collector, size_t index)
{
	struct annotated_column *grown;
	uint32_t old_capacity;

	if (index >= UINT32_MAX) {
		mant_annotated_fail_relation(collector, index, UINT32_MAX - 1);
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
		mant_annotated_fail_relation(collector, index, UINT32_MAX - 1);
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
			mant_annotated_fail_relation(collector, key, collector->mark_count);
			return 0;
		}
		state = collector->points + key - 1;
		if (state->state != 1) {
			mant_annotated_fail_relation(collector, state->state, 1);
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
	if (!mant_annotated_charge_work(collector, end - first))
		return 0;
	if (!mant_annotated_display_checkpoint(collector->display, 0,
	    &checkpoint)) {
		mant_annotated_fail_relation(collector, first, end);
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
		mant_annotated_fail_relation(collector, collector->frame_count, 1);
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
		mant_annotated_fail_relation(collector, key, collector->mark_count);
		return;
	}
	state = collector->points + key - 1;
	if (state->state != 0) {
		mant_annotated_fail_relation(collector, state->state, 0);
		return;
	}
	if ((p->flags & TERMP_NOBUF) != 0 ||
	    (event->pos == 0 && p->tcol->lastcol == 0) ||
	    event->pos < event->end) {
		if (!mant_annotated_display_checkpoint(collector->display,
		    collector->advance_count, &state->checkpoint)) {
			mant_annotated_fail_relation(collector, event->pos, event->end);
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
			mant_annotated_fail_relation(collector, observed, allowed);
	} else if (status == MANT_ANNOTATED_DISPLAY_ALLOC)
		mant_structured_set_failure(session, MANT_STRUCTURED_BUILDER_ALLOC,
		    MANT_STRUCTURED_STAGE_RENDER, 0, 0, 0);
	else
		mant_annotated_fail_relation(collector, status, MANT_ANNOTATED_DISPLAY_ALLOC);
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
		mant_annotated_fail_relation(collector, collector->letter_pos, 0);
		return 0;
	}
	if (column->skipping) {
		if (column->skip_cell_width == 0 ||
		    column->skipped_visual % column->skip_cell_width != 0 ||
		    column->skip_start > collector->letter_pos) {
			mant_annotated_fail_relation(collector, column->skipped_visual,
			    column->skip_cell_width);
			return 0;
		}
		/* FIELD_SKIP events paid for the first traversal; resolving their
		 * final device gaps is a second, separately charged traversal. */
		if (!mant_annotated_charge_work(collector,
		    collector->letter_pos - column->skip_start))
			return 0;
		total_cells = column->skipped_visual /
		    column->skip_cell_width;
		emitted_cells = total_cells < emitted_advances ?
		    total_cells : emitted_advances;
		if (emitted_cells > checkpoint.column) {
			mant_annotated_fail_relation(collector, emitted_cells, checkpoint.column);
			return 0;
		}
		prefix = 0;
		for (index = column->skip_start;
		    index < collector->letter_pos; index++) {
			if (index >= column->capacity) {
				mant_annotated_fail_relation(collector, index, column->capacity);
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

/* Word provenance is optional annotation evidence, not display safety.
 * An inconsistent observer interval cannot be used for compatible links,
 * but must not revoke already checked native body bytes. */
static void
reject_word_evidence(struct mant_annotated_collector *collector)
{
	collector->link_annotation_rejected = 1;
	collector->active_word = NULL;
	collector->active_word_length = 0;
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
	if (collector == NULL || event == NULL)
		return;
	/* A failure can stop ordinary observation mid-word.  The formatter
	 * still closes term_word(); do not retain its borrowed input pointer
	 * through the collector's later cleanup path. */
	if (event->op == TERM_COLLECT_WORD &&
	    event->phase == TERM_COLLECT_LEAVE &&
	    (collector->session->status != MANT_STRUCTURED_OK ||
	    mant_mandoc_output_active_failed())) {
		collector->active_word = NULL;
		collector->active_word_length = 0;
		return;
	}
	if (collector->session->status != MANT_STRUCTURED_OK ||
	    mant_mandoc_output_active_failed())
		return;
	if (event->op == TERM_COLLECT_WORD) {
		/* term.c::term_word() owns the borrowed input for precisely this
		 * synchronous interval.  Older pos/end fields remain buffer units;
		 * no pointer or source byte range is retained after LEAVE. */
		if (event->phase == TERM_COLLECT_ENTER &&
		    collector->active_word == NULL && event->word != NULL &&
		    event->word_start == 0) {
			collector->active_word = event->word;
			collector->active_word_length = event->word_end;
		} else if (event->phase == TERM_COLLECT_LEAVE &&
		    collector->active_word == event->word &&
		    event->word_start <= event->word_end &&
		    event->word_end <= collector->active_word_length) {
			collector->active_word = NULL;
			collector->active_word_length = 0;
		} else
			reject_word_evidence(collector);
		return;
	}
	if (!mant_annotated_charge_work(collector, 1))
		return;
	switch (event->op) {
	case TERM_COLLECT_NODE:
		mant_annotated_marks_observe(collector, event);
		return;
	case TERM_COLLECT_TAG_POINT:
	case TERM_COLLECT_OWNER_POINT:
	case TERM_COLLECT_REGION_POINT:
		arm_point(collector, p, event);
		return;
	case TERM_COLLECT_TABLE_CELL:
		if (event->column < collector->column_capacity)
			join_hard(collector->columns + event->column);
		mant_annotated_marks_observe(collector, event);
		return;
	case TERM_COLLECT_TABLE_CELL_POSITION:
		mant_annotated_marks_observe(collector, event);
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
		if (event->word != collector->active_word ||
		    (event->word != NULL &&
		    (event->word_start > event->word_end ||
		    event->word_end > collector->active_word_length))) {
			reject_word_evidence(collector);
		} else {
			/* In pinned term.c, TEXT reaches logical_emit() one input byte
			 * at a time through encode().  Escapes and generated spaces
			 * have different reasons and are not guessed here. */
			if (event->reason == TERM_COLLECT_TEXT &&
			    event->word != NULL &&
			    (event->word_end != event->word_start + 1 ||
			    (unsigned char)event->value != (unsigned char)
			    event->word[event->word_start]))
				reject_word_evidence(collector);
		}
		if (!mant_annotated_marks_select_component(collector, event->node))
			return;
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
		collector->pending_link = mant_annotated_marks_visible_link(collector,
		    event->node, event->reason);
		collector->pending_source = mant_annotated_marks_source_key(event->node);
		if (collector->active_cell != NULL &&
		    collector->pending_source == 0)
			collector->pending_source =
			    collector->marks[collector->active_owner - 1].source;
		if (event->reason == TERM_COLLECT_AUTO_SPACE)
			collector->pending_source = 0;
		if (collector->pending_source >
		    collector->session->result->source_count) {
			mant_annotated_fail_relation(collector, collector->pending_source,
			    collector->session->result->source_count);
			return;
		}
		collector->metrics.glyph_origins++;
		return;
	case TERM_COLLECT_BUFFER_WRITE:
		if (event->end != event->pos + 1 ||
		    !mant_annotated_charge_mutations(collector, 1)) {
			if (collector->session->status == MANT_STRUCTURED_OK)
				mant_annotated_fail_relation(collector, event->end, event->pos + 1);
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
		slot->font = (uint8_t)event->font;
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
			mant_annotated_fail_relation(collector, event->pos, event->column);
			return;
		}
		if (event->column >= collector->column_capacity ||
		    event->pos >= collector->columns[event->column].capacity) {
			mant_annotated_fail_relation(collector, event->pos, event->column);
			return;
		}
		column = collector->columns + event->column;
		if (column->skipping) {
			size_t cell_width = (*p->getwidth)(p, ' ');

			if (cell_width == 0 || cell_width > UINT32_MAX) {
				mant_annotated_fail_relation(collector, cell_width, UINT32_MAX);
				return;
			}
			column->skip_cell_width = cell_width;
		}
		slot = column->slots + event->pos;
		if (!slot->occupied || slot->value != event->value) {
			mant_annotated_fail_relation(collector, event->value, slot->value);
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
		if (slot->value == '_' &&
		    (slot->flags & MANT_ANNOTATED_FONT_STROKE) == 0)
			collector->letter_label.style =
			    underscore_style(slot->font);
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
			mant_annotated_fail_relation(collector, event->value, 0);
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
			mant_annotated_fail_relation(collector, event->value, 0);
			return;
		}
		memset(&collector->letter_label, 0,
		    sizeof(collector->letter_label));
		collector->letter_label.role = current_role(collector) ==
		    MANT_ANNOTATED_BODY ? MANT_ANNOTATED_DIRECT_DRAW :
		    current_role(collector);
		collector->letter_label.owner = collector->active_owner;
		collector->letter_label.link = mant_annotated_marks_visible_link(collector,
		    event->node, event->reason);
		collector->letter_label.source = mant_annotated_marks_source_key(event->node);
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
			mant_annotated_fail_relation(collector, event->pos,
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
			mant_annotated_fail_relation(collector, event->pos, event->end);
			return;
		}
		if (event->column < collector->column_capacity) {
			column = collector->columns + event->column;
			index = event->pos < column->capacity ? event->pos :
			    column->capacity;
			end = event->end < column->capacity ? event->end :
			    column->capacity;
			if (!mant_annotated_charge_mutations(collector, end - index) ||
			    !mant_annotated_charge_work(collector, end - index))
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
			mant_annotated_fail_relation(collector, length, 1);
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
			mant_annotated_fail_relation(collector, operation, 0);
			return 0;
		}
		label = collector->letter_label;
		if (collector->in_margin && collector->margin_owner != 0 &&
		    label.role == MANT_ANNOTATED_BODY &&
		    !(length == 1 && bytes != NULL &&
		    ((const uint8_t *)bytes)[0] == '\b')) {
			if (collector->margin_mark == 0) {
				/* term.c::endline() emits .mc after the field.  Give
				 * its generated display bytes their own region so a
				 * heading title or definition head stays semantic-only. */
				collector->margin_mark =
				    mant_annotated_marks_margin(collector);
				if (collector->margin_mark == 0)
					return 0;
			}
			label.owner = collector->margin_mark;
		}
		if (from_field) {
			if (collector->letter_column >=
			    collector->column_capacity) {
				mant_annotated_fail_relation(collector,
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
		mant_annotated_fail_relation(collector, operation, MANT_OUTPUT_ENDLINE);
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
			mant_annotated_fail_relation(collector, collector->pending_point_count, 0);
		return 0;
	}
	/* This pass visits every mark, including those without point state. */
	if (!mant_annotated_charge_work(collector, collector->mark_count))
		return 0;
	for (index = 0; index < collector->mark_count; index++) {
		mark = collector->marks + index;
		state = collector->points + index;
		if (state->state == 0) {
			if (mark->kind == MANT_ANNOTATED_MARK_ANCHOR ||
			    mark->kind == MANT_ANNOTATED_MARK_REGION) {
				mant_annotated_fail_relation(collector, mark->key, 0);
				return 0;
			}
			continue;
		}
		if (state->state != 2 || state->next != 0 ||
		    state->checkpoint.row_before > display->row_count) {
			mant_annotated_fail_relation(collector, mark->key, display->row_count);
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
mant_annotated_buffer_release(struct mant_annotated_collector *collector)
{
	uint32_t index;

	for (index = 0; index < collector->column_capacity; index++)
		free(collector->columns[index].slots);
	free(collector->columns);
	free(collector->points);
}
