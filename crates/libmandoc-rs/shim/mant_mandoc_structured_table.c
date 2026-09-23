/* Native tbl structure follows tbl_term.c::term_tbl and tbl_word. */
#include "config.h"

#include <stdio.h>
#include <string.h>

#include "mandoc.h"
#include "roff.h"
#include "tbl.h"

#include "mant_mandoc_structured_address.h"
#include "mant_mandoc_structured_builder.h"
#include "mant_mandoc_structured_structure.h"
#include "mant_mandoc_structured_table.h"

static uint32_t
row_kind(const struct tbl_span *span)
{
	const struct tbl_cell *layout;
	int has_rule;

	switch (span->pos) {
	case TBL_SPAN_DATA:
		has_rule = 0;
		for (layout = span->layout->first; layout != NULL;
		    layout = layout->next) {
			if (layout->pos == TBL_CELL_SPAN)
				continue;
			if (layout->pos != TBL_CELL_HORIZ &&
			    layout->pos != TBL_CELL_DHORIZ)
				return MANT_TABLE_ROW_DATA;
			has_rule = 1;
		}
		if (has_rule)
			return MANT_TABLE_ROW_LAYOUT_RULE;
		return MANT_TABLE_ROW_DATA;
	case TBL_SPAN_HORIZ:
		return MANT_TABLE_ROW_HORIZONTAL_RULE;
	case TBL_SPAN_DHORIZ:
		return MANT_TABLE_ROW_DOUBLE_HORIZONTAL_RULE;
	}
	return 0;
}

static uint32_t
cell_kind(const struct tbl_dat *data, const struct tbl_cell *layout)
{
	/* tbl_term.c::tbl_data gives a layout rule precedence over the row's
	 * data cell, so the logical kind follows the executed rule path. */
	if (layout->pos == TBL_CELL_HORIZ)
		return MANT_TABLE_CELL_HORIZONTAL_RULE;
	if (layout->pos == TBL_CELL_DHORIZ)
		return MANT_TABLE_CELL_DOUBLE_HORIZONTAL_RULE;
	if (data == NULL)
		return MANT_TABLE_CELL_TEXT;
	switch (data->pos) {
	case TBL_DATA_DATA:
		return MANT_TABLE_CELL_TEXT;
	case TBL_DATA_HORIZ:
		return MANT_TABLE_CELL_HORIZONTAL_RULE;
	case TBL_DATA_DHORIZ:
		return MANT_TABLE_CELL_DOUBLE_HORIZONTAL_RULE;
	case TBL_DATA_NHORIZ:
		return MANT_TABLE_CELL_ISOLATED_HORIZONTAL_RULE;
	case TBL_DATA_NDHORIZ:
		return MANT_TABLE_CELL_ISOLATED_DOUBLE_HORIZONTAL_RULE;
	case TBL_DATA_NONE:
		return MANT_TABLE_CELL_TEXT;
	}
	return 0;
}

static uint32_t
cell_alignment(const struct tbl_cell *layout)
{
	switch (layout->pos) {
	case TBL_CELL_CENTRE:
		return MANT_TABLE_ALIGN_CENTER;
	case TBL_CELL_RIGHT:
	case TBL_CELL_NUMBER:
		return MANT_TABLE_ALIGN_RIGHT;
	default:
		return MANT_TABLE_ALIGN_LEFT;
	}
}

static uint32_t
append_table(struct structured_session *session, const struct roff_node *node,
    uint32_t provenance)
{
	struct structured_node_context *context;
	struct mant_structured_table_view *tables, *table;
	uint32_t owner, parent, block;

	context = mant_structured_current_context(session);
	if (context != NULL && context->owner != 0)
		owner = context->owner;
	else {
		if (session->section_owner == 0)
			session->section_owner = mant_structured_append_owner(session,
			    MANT_OWNER_DOCUMENT, provenance);
		owner = session->section_owner;
	}
	parent = context != NULL && context->container_block != 0 ?
	    context->container_block : session->section_heading_block;
	if (owner == 0)
		return 0;
	block = mant_structured_append_block(session, owner, MANT_BLOCK_TABLE,
	    parent, provenance, 0);
	if (block == 0)
		return 0;
	tables = mant_structured_grow_array(session, session->result->tables,
	    session->result->table_count, &session->result->table_capacity,
	    mant_structured_limit_u32(session->limits->max_blocks),
	    sizeof(*tables), session->limits->max_builder_allocated_bytes, 12,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (tables == NULL)
		return 0;
	session->result->tables = tables;
	if (!mant_structured_charge(session, &session->builder_operations, 1,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_RENDER) ||
	    !mant_structured_charge(session, &session->relation_edges, 3,
	    session->limits->max_relation_edges, 30,
	    MANT_STRUCTURED_STAGE_RENDER))
		return 0;
	table = tables + session->result->table_count;
	memset(table, 0, sizeof(*table));
	table->key = ++session->result->table_count;
	table->block = block;
	table->provenance = provenance;
	session->result->blocks[block - 1].table = table->key;
	(void)node;
	return table->key;
}

int
mant_structured_table_enter(struct structured_session *session,
    const struct roff_node *node)
{
	const struct tbl_span *span = node->span;
	const struct tbl_dat *data, *current;
	const struct tbl_cell *layout;
	struct mant_structured_table_row_view *rows, *row;
	struct mant_structured_table_cell_view *cells, *cell;
	uint32_t provenance, owner, root, point, row_key, column_span;
	uint32_t covered_until;

	if (span == NULL) {
		mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
		    MANT_STRUCTURED_STAGE_RENDER, 0, 0, 0);
		return 0;
	}
	provenance = mant_structured_append_provenance(session, node, 1);
	if (provenance == 0)
		return 0;
	if (span->prev == NULL)
		session->active_table = append_table(session, node, provenance);
	if (session->active_table == 0)
		return 0;
	rows = mant_structured_grow_array(session, session->result->table_rows,
	    session->result->table_row_count, &session->result->table_row_capacity,
	    mant_structured_limit_u32(session->limits->max_blocks), sizeof(*rows),
	    session->limits->max_builder_allocated_bytes, 12,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (rows == NULL)
		return 0;
	session->result->table_rows = rows;
	if (!mant_structured_charge(session, &session->builder_operations, 1,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_RENDER) ||
	    !mant_structured_charge(session, &session->relation_edges, 2,
	    session->limits->max_relation_edges, 30,
	    MANT_STRUCTURED_STAGE_RENDER))
		return 0;
	row = rows + session->result->table_row_count;
	memset(row, 0, sizeof(*row));
	row->key = ++session->result->table_row_count;
	row->table = session->active_table;
	row->kind = row_kind(span);
	if (row->kind == 0) {
		mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
		    MANT_STRUCTURED_STAGE_RENDER, 0, 0, 0);
		return 0;
	}
	row->provenance = provenance;
	row->ordinal = row->key == 1 ||
	    rows[row->key - 2].table != row->table ? 0 :
	    rows[row->key - 2].ordinal + 1;
	row_key = row->key;
	session->active_table_row = row_key;
	session->table_cell_start = session->result->table_cell_count + 1;
	session->current_root = 0;
	session->current_owner = 0;
	if (span->pos != TBL_SPAN_DATA)
		return session->status == MANT_STRUCTURED_OK;
	data = span->first;
	covered_until = 0;
	for (layout = span->layout->first; layout != NULL;
	    layout = layout->next) {
		if (layout->col < 0) {
			mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
			    MANT_STRUCTURED_STAGE_RENDER, 0, 0, 0);
			return 0;
		}
		if ((uint32_t)layout->col < covered_until ||
		    layout->pos == TBL_CELL_SPAN)
			continue;
		current = data != NULL && data->layout == layout ? data : NULL;
		if (current != NULL)
			data = data->next;
		column_span = current == NULL ? 1 : (uint32_t)current->hspans + 1;
		if (UINT32_MAX - (uint32_t)layout->col < column_span) {
			mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
			    MANT_STRUCTURED_STAGE_RENDER, 0, 0, 0);
			return 0;
		}
		covered_until = (uint32_t)layout->col + column_span;
		owner = mant_structured_append_owner(session, MANT_OWNER_TABLE_CELL,
		    provenance);
		root = mant_structured_append_root(session, owner, MANT_ROOT_CELL,
		    provenance);
		point = mant_structured_append_point(session, root, 0);
		if (owner == 0 || root == 0 || point == 0)
			return 0;
		cells = mant_structured_grow_array(session,
		    session->result->table_cells,
		    session->result->table_cell_count,
		    &session->result->table_cell_capacity,
		    mant_structured_limit_u32(session->limits->max_owners),
		    sizeof(*cells), session->limits->max_builder_allocated_bytes,
		    11, MANT_STRUCTURED_STAGE_RENDER);
		if (cells == NULL)
			return 0;
		session->result->table_cells = cells;
		if (!mant_structured_charge(session,
		    &session->builder_operations, 1,
		    session->limits->max_builder_operations, 8,
		    MANT_STRUCTURED_STAGE_RENDER) ||
		    !mant_structured_charge(session, &session->relation_edges, 4,
		    session->limits->max_relation_edges, 30,
		    MANT_STRUCTURED_STAGE_RENDER))
			return 0;
		cell = cells + session->result->table_cell_count;
		memset(cell, 0, sizeof(*cell));
		cell->key = ++session->result->table_cell_count;
		cell->row = row_key;
		cell->column = (uint32_t)layout->col;
		cell->owner = owner;
		cell->kind = cell_kind(current, layout);
		cell->alignment = cell_alignment(layout);
		cell->row_span = current == NULL ? 1 : (uint32_t)current->vspans + 1;
		cell->column_span = column_span;
		cell->point = point;
		cell->provenance = provenance;
	}
	if (data != NULL) {
		mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
		    MANT_STRUCTURED_STAGE_RENDER, 0, 0, 0);
		return 0;
	}
	return session->status == MANT_STRUCTURED_OK;
}

void
mant_structured_table_cell(struct structured_session *session,
    const struct tbl_dat *data, int entering)
{
	const struct mant_structured_table_cell_view *cell;
	const struct roff_node *node;
	uint32_t key, low, high, middle;

	if (!entering) {
		session->active_table_cell = 0;
		session->current_root = 0;
		session->current_owner = 0;
		return;
	}
	node = session->node_depth == 0 ? NULL :
	    session->node_stack[session->node_depth - 1];
	if (data == NULL || data->layout == NULL || node == NULL ||
	    node->type != ROFFT_TBL || session->active_table_row == 0) {
		mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
		    MANT_STRUCTURED_STAGE_RENDER, 0, 0, 0);
		return;
	}
	low = session->table_cell_start;
	high = session->result->table_cell_count + 1;
	key = 0;
	while (low < high) {
		middle = low + (high - low) / 2;
		cell = session->result->table_cells + middle - 1;
		if (cell->column == (uint32_t)data->layout->col) {
			key = middle;
			break;
		}
		if (cell->column < (uint32_t)data->layout->col)
			low = middle + 1;
		else
			high = middle;
	}
	if (key == 0) {
		mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
		    MANT_STRUCTURED_STAGE_RENDER, 0, key,
		    session->result->table_cell_count);
		return;
	}
	cell = session->result->table_cells + key - 1;
	session->active_table_cell = key;
	session->current_owner = cell->owner;
	session->current_root = session->result->content_points[
	    cell->point - 1].root;
}

void
mant_structured_table_leave(struct structured_session *session,
    const struct roff_node *node)
{
	session->active_table_cell = 0;
	session->active_table_row = 0;
	session->current_root = 0;
	session->current_owner = 0;
	if (node->span->next == NULL)
		session->active_table = 0;
}
