/* Physical table geometry observed at the pinned terminal execution boundary. */
#include "config.h"

#include <stdio.h>
#include <string.h>

#include "mandoc.h"
#include "roff.h"
#include "tbl.h"
#include "out.h"
#include "term.h"

#include "mant_mandoc_structured_buffer.h"
#include "mant_mandoc_structured_builder.h"
#include "mant_mandoc_structured_fixed.h"

#define MANT_FIXED_MAX_LINE_COLUMNS 1048576U
#define MANT_FIXED_MAX_TOTAL_COLUMNS (32U * 1024U * 1024U)

static size_t
encode_scalar(int value, uint8_t bytes[4])
{
	uint32_t scalar = (uint32_t)value;

	if (scalar <= 0x7f) {
		bytes[0] = (uint8_t)scalar;
		return 1;
	}
	if (scalar <= 0x7ff) {
		bytes[0] = 0xc0 | (uint8_t)(scalar >> 6);
		bytes[1] = 0x80 | (uint8_t)(scalar & 0x3f);
		return 2;
	}
	if (scalar <= 0xffff && !(scalar >= 0xd800 && scalar <= 0xdfff)) {
		bytes[0] = 0xe0 | (uint8_t)(scalar >> 12);
		bytes[1] = 0x80 | (uint8_t)((scalar >> 6) & 0x3f);
		bytes[2] = 0x80 | (uint8_t)(scalar & 0x3f);
		return 3;
	}
	if (scalar <= 0x10ffff) {
		bytes[0] = 0xf0 | (uint8_t)(scalar >> 18);
		bytes[1] = 0x80 | (uint8_t)((scalar >> 12) & 0x3f);
		bytes[2] = 0x80 | (uint8_t)((scalar >> 6) & 0x3f);
		bytes[3] = 0x80 | (uint8_t)(scalar & 0x3f);
		return 4;
	}
	return 0;
}

int
mant_structured_fixed_table_required(struct structured_session *session,
    const struct tbl_span *span)
{
	const struct tbl_cell *cell;
	const struct tbl_dat *data;
	int fixed = 0;

	/* tbl_term.c::term_tbl performs native width, alignment, frame, and
	 * repeated-line layout.  Plain left-aligned data rows alone can use the
	 * generic table layout without a second physical projection. */
	for (; span != NULL; span = span->next) {
		if (!mant_structured_charge(session, &session->builder_operations, 1,
		    session->limits->max_builder_operations, 8,
		    MANT_STRUCTURED_STAGE_RENDER))
			return -1;
		if (span->pos != TBL_SPAN_DATA || span->opts->lvert != 0 ||
		    span->opts->rvert != 0 ||
		    (span->opts->opts & (TBL_OPT_ALLBOX | TBL_OPT_BOX |
		    TBL_OPT_CENTRE | TBL_OPT_DBOX | TBL_OPT_EXPAND)) != 0 ||
		    span->layout->vert != 0)
			fixed = 1;
		for (cell = span->layout->first; cell != NULL; cell = cell->next) {
			if (!mant_structured_charge(session,
			    &session->builder_operations, 1,
			    session->limits->max_builder_operations, 8,
			    MANT_STRUCTURED_STAGE_RENDER))
				return -1;
			if (cell->pos != TBL_CELL_LEFT || cell->vert != 0 ||
			    cell->flags != 0)
				fixed = 1;
		}
		for (data = span->first; data != NULL; data = data->next) {
			if (!mant_structured_charge(session,
			    &session->builder_operations, 1,
			    session->limits->max_builder_operations, 8,
			    MANT_STRUCTURED_STAGE_RENDER))
				return -1;
			if (data->pos != TBL_DATA_DATA &&
			    data->pos != TBL_DATA_NONE)
				fixed = 1;
			if (data->hspans != 0 || data->vspans != 0 ||
			    data->block != 0)
				fixed = 1;
		}
	}
	return fixed;
}

uint32_t
mant_structured_fixed_open_table(struct structured_session *session,
    uint32_t table_key, uint32_t block_key, uint32_t owner,
    uint32_t provenance)
{
	struct mant_structured_fixed_view *views, *view;

	views = mant_structured_grow_array(session, session->result->fixed_views,
	    session->result->fixed_view_count,
	    &session->result->fixed_view_capacity,
	    mant_structured_limit_u32(session->limits->max_fixed_views),
	    sizeof(*views), session->limits->max_builder_allocated_bytes, 20,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (views == NULL)
		return 0;
	session->result->fixed_views = views;
	if (!mant_structured_charge(session, &session->builder_operations, 1,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_RENDER) ||
	    !mant_structured_charge(session, &session->relation_edges, 4,
	    session->limits->max_relation_edges, 30,
	    MANT_STRUCTURED_STAGE_RENDER))
		return 0;
	view = views + session->result->fixed_view_count;
	memset(view, 0, sizeof(*view));
	view->key = ++session->result->fixed_view_count;
	view->owner = owner;
	view->block = block_key;
	view->table = table_key;
	view->provenance = provenance;
	session->result->tables[table_key - 1].fixed_view = view->key;
	session->result->blocks[block_key - 1].fixed_view = view->key;
	session->active_fixed_view = view->key;
	return view->key;
}

static uint32_t
open_line(struct structured_session *session)
{
	struct mant_structured_fixed_line_view *lines, *line;
	uint32_t prior;

	if (session->active_fixed_line != 0)
		return session->active_fixed_line;
	if (session->active_fixed_view == 0)
		return 0;
	prior = session->result->fixed_line_count;
	lines = mant_structured_grow_array(session, session->result->fixed_lines,
	    prior, &session->result->fixed_line_capacity,
	    mant_structured_limit_u32(session->limits->max_fixed_lines),
	    sizeof(*lines), session->limits->max_builder_allocated_bytes, 21,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (lines == NULL)
		return 0;
	session->result->fixed_lines = lines;
	if (!mant_structured_charge(session, &session->builder_operations, 1,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_RENDER) ||
	    !mant_structured_charge(session, &session->relation_edges, 1,
	    session->limits->max_relation_edges, 30,
	    MANT_STRUCTURED_STAGE_RENDER))
		return 0;
	line = lines + prior;
	memset(line, 0, sizeof(*line));
	line->key = ++session->result->fixed_line_count;
	line->view = session->active_fixed_view;
	line->ordinal = prior == 0 || lines[prior - 1].view != line->view ?
	    0 : lines[prior - 1].ordinal + 1;
	session->active_fixed_line = line->key;
	return line->key;
}

static int
columns(struct structured_session *session, struct termp *p,
    size_t position, size_t width, uint32_t *start, uint32_t *end)
{
	size_t unit;

	unit = term_len(p, 1);
	if (unit == 0 || position % unit != 0 || width % unit != 0 ||
	    position / unit > MANT_FIXED_MAX_LINE_COLUMNS ||
	    width / unit > MANT_FIXED_MAX_LINE_COLUMNS ||
	    position / unit + width / unit > MANT_FIXED_MAX_LINE_COLUMNS) {
		mant_structured_set_failure(session, MANT_STRUCTURED_UNSUPPORTED,
		    MANT_STRUCTURED_STAGE_RENDER, 0, position, width);
		return 0;
	}
	*start = (uint32_t)(position / unit);
	*end = *start + (uint32_t)(width / unit);
	return 1;
}

static void
append_decoration(struct structured_session *session, uint32_t line_key,
    uint32_t start, uint32_t end, int value, uint32_t kind)
{
	struct mant_structured_decoration_view *decorations, *decoration;
	const struct roff_node *node;
	uint8_t bytes[4];
	size_t length;
	uint32_t provenance, count;

	if (start == end || value == ' ')
		return;
	length = encode_scalar(value, bytes);
	if (length == 0) {
		mant_structured_set_failure(session, MANT_STRUCTURED_UNSUPPORTED,
		    MANT_STRUCTURED_STAGE_RENDER, 0, value, 0);
		return;
	}
	node = session->node_depth == 0 ? NULL :
	    session->node_stack[session->node_depth - 1];
	provenance = mant_structured_append_provenance(session, node, 0);
	if (provenance == 0)
		return;
	count = session->result->decoration_count;
	decorations = mant_structured_grow_array(session,
	    session->result->decorations, count,
	    &session->result->decoration_capacity,
	    mant_structured_limit_u32(session->limits->max_decorations),
	    sizeof(*decorations), session->limits->max_builder_allocated_bytes,
	    23, MANT_STRUCTURED_STAGE_RENDER);
	if (decorations == NULL)
		return;
	session->result->decorations = decorations;
	if (!mant_structured_charge(session, &session->builder_operations, 1,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_RENDER) ||
	    !mant_structured_charge(session, &session->relation_edges, 2,
	    session->limits->max_relation_edges, 30,
	    MANT_STRUCTURED_STAGE_RENDER))
		return;
	decoration = decorations + count;
	memset(decoration, 0, sizeof(*decoration));
	decoration->text.ptr = mant_structured_copy_bytes(session, bytes,
	    length, 1, MANT_STRUCTURED_STAGE_RENDER);
	if (decoration->text.ptr == NULL)
		return;
	decoration->text.len = length;
	decoration->key = ++session->result->decoration_count;
	decoration->line = line_key;
	decoration->ordinal = count == 0 ||
	    decorations[count - 1].line != line_key ? 0 :
	    decorations[count - 1].ordinal + 1;
	decoration->kind = kind;
	decoration->column_start = start;
	decoration->column_end = end;
	decoration->provenance = provenance;
}

static void
record_use(struct structured_session *session, struct structured_token *token,
    uint32_t line, uint32_t start, uint32_t end)
{
	struct structured_fixed_use *uses, *use;
	uint32_t count;

	count = token->fixed_use_count;
	if (count != 0 && token->fixed_uses[count - 1].line == line) {
		use = token->fixed_uses + count - 1;
		if (use->start > start)
			use->start = start;
		if (use->end < end)
			use->end = end;
		return;
	}
	uses = mant_structured_grow_array(session, token->fixed_uses, count,
	    &token->fixed_use_capacity,
	    mant_structured_limit_u32(session->limits->max_placements),
	    sizeof(*uses), session->limits->max_builder_allocated_bytes, 22,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (uses == NULL)
		return;
	token->fixed_uses = uses;
	use = uses + count;
	memset(use, 0, sizeof(*use));
	use->line = line;
	use->start = start;
	use->end = end;
	token->fixed_use_count++;
}

void
mant_structured_fixed_field(struct structured_session *session, struct termp *p,
    const struct term_collector_event *event, struct structured_token *token,
    uint32_t token_key)
{
	struct mant_structured_fixed_line_view *line;
	uint32_t key, start, end, kind;

	if (session->active_fixed_view == 0)
		return;
	if (event->value == '\b') {
		if (token != NULL && token->fixed_use_count != 0)
			token->fixed_uses[token->fixed_use_count - 1].overlay = 1;
		return;
	}
	if (!columns(session, p, p->viscol, event->visual, &start, &end) ||
	    (key = open_line(session)) == 0)
		return;
	line = session->result->fixed_lines + key - 1;
	if (line->total_columns < end)
		line->total_columns = end;
	if (token_key != 0 && token != NULL) {
		record_use(session, token, key, start, end);
		return;
	}
	kind = session->active_table_row != 0 &&
	    session->result->table_rows[session->active_table_row - 1].kind !=
	    MANT_TABLE_ROW_DATA ? MANT_DECORATION_RULE :
	    MANT_DECORATION_BORDER;
	append_decoration(session, key, start, end, event->value, kind);
}

void
mant_structured_fixed_draw(struct structured_session *session, struct termp *p,
    const struct term_collector_event *event)
{
	struct mant_structured_fixed_line_view *line;
	uint32_t key, start, end, kind;

	if (session->active_fixed_view == 0)
		return;
	if (!columns(session, p, event->pos, event->visual,
	    &start, &end) || (key = open_line(session)) == 0)
		return;
	line = session->result->fixed_lines + key - 1;
	if (line->total_columns < end)
		line->total_columns = end;
	kind = session->active_table_row != 0 &&
	    session->result->table_rows[session->active_table_row - 1].kind !=
	    MANT_TABLE_ROW_DATA ? MANT_DECORATION_RULE :
	    MANT_DECORATION_BORDER;
	append_decoration(session, key, start, end, event->value, kind);
}

void
mant_structured_fixed_endline(struct structured_session *session, struct termp *p,
    const struct term_collector_event *event)
{
	struct mant_structured_fixed_line_view *line;
	uint32_t key, start, end;

	if (session->active_fixed_view == 0 ||
	    event->reason != TERM_COLLECT_TABLE_LINE)
		return;
	if (!columns(session, p, event->visual, 0, &start, &end) ||
	    (key = open_line(session)) == 0)
		return;
	line = session->result->fixed_lines + key - 1;
	if (line->total_columns < start)
		line->total_columns = start;
	if (session->fixed_column_total > MANT_FIXED_MAX_TOTAL_COLUMNS -
	    line->total_columns) {
		mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET,
		    MANT_STRUCTURED_STAGE_RENDER, 21,
		    session->fixed_column_total + line->total_columns,
		    MANT_FIXED_MAX_TOTAL_COLUMNS);
		return;
	}
	session->fixed_column_total += line->total_columns;
	session->active_fixed_line = 0;
}

void
mant_structured_fixed_commit(struct structured_session *session,
    const struct structured_token *token, uint32_t atom, uint32_t byte_start,
    uint32_t byte_end, uint32_t scalar_start, uint32_t scalar_end)
{
	struct mant_structured_placement_view *placements, *placement;
	const struct structured_fixed_use *use;
	uint32_t index, count, width;

	for (index = 0; index < token->fixed_use_count; index++) {
		use = token->fixed_uses + index;
		if (use->start == use->end)
			continue;
		width = use->end - use->start;
		if (scalar_end != scalar_start + 1 || width > UINT8_MAX) {
			mant_structured_set_failure(session, MANT_STRUCTURED_UNSUPPORTED,
			    MANT_STRUCTURED_STAGE_RENDER, 0, width,
			    scalar_end - scalar_start);
			return;
		}
		count = session->result->placement_count;
		placements = mant_structured_grow_array(session,
		    session->result->placements, count,
		    &session->result->placement_capacity,
		    mant_structured_limit_u32(session->limits->max_placements),
		    sizeof(*placements), session->limits->max_builder_allocated_bytes,
		    22, MANT_STRUCTURED_STAGE_RENDER);
		if (placements == NULL)
			return;
		session->result->placements = placements;
		if (!mant_structured_charge(session,
		    &session->builder_operations, 1,
		    session->limits->max_builder_operations, 8,
		    MANT_STRUCTURED_STAGE_RENDER) ||
		    !mant_structured_charge(session, &session->relation_edges, 2,
		    session->limits->max_relation_edges, 30,
		    MANT_STRUCTURED_STAGE_RENDER))
			return;
		placement = placements + count;
		memset(placement, 0, sizeof(*placement));
		placement->key = ++session->result->placement_count;
		placement->line = use->line;
		placement->ordinal = count == 0 ||
		    placements[count - 1].line != use->line ? 0 :
		    placements[count - 1].ordinal + 1;
		placement->target_kind = MANT_PLACEMENT_CONTENT;
		placement->atom = atom;
		placement->byte_start = byte_start;
		placement->byte_end = byte_end;
		placement->scalar_start = scalar_start;
		placement->scalar_end = scalar_end;
		placement->column_start = use->start;
		placement->column_end = use->end;
		placement->cell_map_kind = use->overlay ?
		    MANT_CELL_MAP_OVERLAY : MANT_CELL_MAP_AFFINE;
		placement->cell_map_value = use->overlay ? 0 : width;
	}
}
