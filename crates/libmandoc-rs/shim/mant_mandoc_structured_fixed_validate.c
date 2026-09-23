/* Native fixed geometry cannot outlive or duplicate its logical content. */
#include "mant_mandoc_structured_fixed_validate.h"

#include <stddef.h>

static uint64_t
scalar_count(const uint8_t *bytes, uint32_t length)
{
	uint32_t index;
	uint64_t count = 0;

	for (index = 0; index < length; index++)
		if ((bytes[index] & 0xc0) != 0x80)
			count++;
	return count;
}

static int
utf8_boundary(const uint8_t *bytes, uint64_t length, uint32_t offset)
{
	return offset <= length && (offset == length ||
	    (bytes[offset] & 0xc0) != 0x80);
}

static int
table_has_cell_owner(const struct mant_structured_result *result,
    uint32_t table, uint32_t owner)
{
	uint32_t low = 0, high = result->table_cell_count, middle;

	while (low < high) {
		middle = low + (high - low) / 2;
		if (result->table_cells[middle].owner < owner)
			low = middle + 1;
		else
			high = middle;
	}
	return low < result->table_cell_count &&
	    result->table_cells[low].owner == owner &&
	    result->table_rows[result->table_cells[low].row - 1].table == table;
}

static uint32_t
next_fixed_cell(const struct mant_structured_result *result, uint32_t previous)
{
	uint32_t index, table;

	for (index = previous; index < result->table_cell_count; index++) {
		table = result->table_rows[result->table_cells[index].row - 1].table;
		if (result->tables[table - 1].fixed_view != 0)
			return index + 1;
	}
	return 0;
}

static uint32_t
table_cell_for_point(const struct mant_structured_result *result,
    uint32_t table, uint32_t point)
{
	uint32_t owner, low = 0, high = result->table_cell_count, middle;

	owner = result->content_points[point - 1].owner;
	while (low < high) {
		middle = low + (high - low) / 2;
		if (result->table_cells[middle].owner < owner)
			low = middle + 1;
		else
			high = middle;
	}
	if (low < result->table_cell_count &&
	    result->table_cells[low].owner == owner &&
	    result->table_rows[result->table_cells[low].row - 1].table == table &&
	    result->table_cells[low].point == point)
		return low + 1;
	return 0;
}

int
mant_structured_fixed_result_valid(const struct mant_structured_result *result)
{
	const struct mant_structured_fixed_view *view;
	const struct mant_structured_fixed_line_view *line;
	const struct mant_structured_placement_view *placement;
	const struct mant_structured_decoration_view *decoration;
	const struct mant_structured_content_atom_view *atom;
	const struct mant_structured_content_point_view *point;
	uint32_t i, previous_view, ordinal, previous_line, last_fixed_cell;
	uint32_t placement_ordinal, decoration_ordinal;
	uint64_t total_columns, scalar_work, start, end;

	if ((result->fixed_view_count != 0) != (result->fixed_views != NULL) ||
	    (result->fixed_line_count != 0) != (result->fixed_lines != NULL) ||
	    (result->placement_count != 0) != (result->placements != NULL) ||
	    (result->decoration_count != 0) != (result->decorations != NULL))
		return 0;
	for (i = 0; i < result->fixed_view_count; i++) {
		view = result->fixed_views + i;
		if (view->key != i + 1 || view->owner == 0 ||
		    view->owner > result->owner_count || view->block == 0 ||
		    view->block > result->block_count ||
		    view->provenance == 0 ||
		    view->provenance > result->provenance_count ||
		    view->reserved != 0 ||
		    result->blocks[view->block - 1].owner != view->owner ||
		    result->blocks[view->block - 1].fixed_view != view->key)
			return 0;
		if (view->table != 0) {
			if (view->table > result->table_count ||
			    result->tables[view->table - 1].block != view->block ||
			    result->tables[view->table - 1].fixed_view != view->key)
				return 0;
		} else if (result->blocks[view->block - 1].kind !=
		    MANT_BLOCK_FIXED_DISPLAY)
			return 0;
	}
	for (i = 0; i < result->table_count; i++) {
		uint32_t key = result->tables[i].fixed_view;

		if (key != 0 && (key > result->fixed_view_count ||
		    result->fixed_views[key - 1].table != i + 1))
			return 0;
	}
	for (i = 0; i < result->block_count; i++) {
		uint32_t key = result->blocks[i].fixed_view;

		if (key != 0 && (key > result->fixed_view_count ||
		    result->fixed_views[key - 1].block != i + 1))
			return 0;
	}
	previous_view = ordinal = 0;
	total_columns = 0;
	for (i = 0; i < result->fixed_line_count; i++) {
		line = result->fixed_lines + i;
		if (line->view != previous_view) {
			if (line->view != previous_view + 1)
				return 0;
			previous_view = line->view;
			ordinal = 0;
		}
		if (line->key != i + 1 || line->view == 0 ||
		    line->view > result->fixed_view_count ||
		    line->ordinal != ordinal++ ||
		    line->total_columns > 1048576U ||
		    line->reserved != 0)
			return 0;
		total_columns += line->total_columns;
		if (total_columns > 32U * 1024U * 1024U)
			return 0;
	}
	if (previous_view != result->fixed_view_count)
		return 0;
	previous_line = placement_ordinal = 0;
	last_fixed_cell = 0;
	scalar_work = 0;
	for (i = 0; i < result->placement_count; i++) {
		placement = result->placements + i;
		if (placement->line != previous_line) {
			if (placement->line < previous_line)
				return 0;
			previous_line = placement->line;
			placement_ordinal = 0;
		}
		if (placement->key != i + 1 || placement->line == 0 ||
		    placement->line > result->fixed_line_count ||
		    placement->ordinal != placement_ordinal++ ||
		    placement->column_start > placement->column_end ||
		    placement->column_end > result->fixed_lines[
		    placement->line - 1].total_columns ||
		    placement->scalar_start > placement->scalar_end)
			return 0;
		view = result->fixed_views + result->fixed_lines[
		    placement->line - 1].view - 1;
		if (placement->target_kind == MANT_PLACEMENT_CONTENT) {
			if (placement->atom == 0 ||
			    placement->cell != 0 ||
			    placement->atom > result->content_atom_count ||
			    placement->point != 0 ||
			    placement->byte_start >= placement->byte_end ||
			    (placement->column_start == placement->column_end &&
			    placement->cell_map_kind != MANT_CELL_MAP_GRAPHEME_CLUSTER) ||
			    placement->cell_map_kind < MANT_CELL_MAP_AFFINE ||
			    placement->cell_map_kind > MANT_CELL_MAP_OVERLAY)
				return 0;
			atom = result->content_atoms + placement->atom - 1;
			if ((atom->kind != MANT_ATOM_TEXT &&
			    atom->kind != MANT_ATOM_WHITESPACE) ||
			    !utf8_boundary(atom->text.ptr, atom->text.len,
			    placement->byte_start) ||
			    !utf8_boundary(atom->text.ptr, atom->text.len,
			    placement->byte_end) ||
			    (view->table != 0 && !table_has_cell_owner(result,
			    view->table, atom->owner)) ||
			    (view->table == 0 && atom->owner != view->owner))
				return 0;
			/* Prefix recounts for repeated slices still consume work. */
			scalar_work += (uint64_t)placement->byte_end +
			    placement->byte_end - placement->byte_start;
			if (scalar_work > 32U * 1024U * 1024U)
				return 0;
			start = result->validation_atom_scalar_offsets[
			    placement->atom - 1] + scalar_count(atom->text.ptr,
			    placement->byte_start);
			end = start + scalar_count(atom->text.ptr +
			    placement->byte_start, placement->byte_end -
			    placement->byte_start);
			if (end > UINT32_MAX ||
			    placement->scalar_start != start ||
			    placement->scalar_end != end)
				return 0;
			if (placement->cell_map_kind == MANT_CELL_MAP_AFFINE ?
			    placement->cell_map_value == 0 ||
			    (end - start) * placement->cell_map_value !=
			    placement->column_end - placement->column_start :
			    placement->cell_map_value != 0 || end - start != 1)
				return 0;
		} else if (placement->target_kind == MANT_PLACEMENT_POINT) {
			if (placement->point == 0 ||
			    placement->point > result->content_point_count ||
			    placement->atom != 0 || placement->byte_start != 0 ||
			    placement->byte_end != 0 ||
			    placement->column_start != placement->column_end ||
			    placement->cell_map_kind != MANT_CELL_MAP_AFFINE ||
			    placement->cell_map_value != 0)
				return 0;
			point = result->content_points + placement->point - 1;
			if (placement->scalar_start != point->scalar_boundary ||
			    placement->scalar_end != point->scalar_boundary ||
			    (view->table != 0 && placement->cell == 0 &&
			    !table_has_cell_owner(result,
			    view->table, point->owner)) ||
			    (view->table == 0 && point->owner != view->owner))
				return 0;
			if (placement->cell != 0) {
				if (placement->cell != next_fixed_cell(result,
				    last_fixed_cell) ||
				    result->table_cells[placement->cell - 1].point !=
				    placement->point ||
				    result->table_rows[result->table_cells[
				    placement->cell - 1].row - 1].table != view->table)
					return 0;
				last_fixed_cell = placement->cell;
			} else if (view->table != 0 &&
			    table_cell_for_point(result, view->table,
			    placement->point) != 0)
				return 0;
		} else
			return 0;
	}
	if (next_fixed_cell(result, last_fixed_cell) != 0)
		return 0;
	previous_line = decoration_ordinal = 0;
	for (i = 0; i < result->decoration_count; i++) {
		decoration = result->decorations + i;
		if (decoration->line != previous_line) {
			if (decoration->line < previous_line)
				return 0;
			previous_line = decoration->line;
			decoration_ordinal = 0;
		}
		if (decoration->key != i + 1 || decoration->line == 0 ||
		    decoration->line > result->fixed_line_count ||
		    decoration->ordinal != decoration_ordinal++ ||
		    decoration->kind < MANT_DECORATION_BORDER ||
		    decoration->kind > MANT_DECORATION_PADDING ||
		    decoration->column_start >= decoration->column_end ||
		    decoration->column_end > result->fixed_lines[
		    decoration->line - 1].total_columns ||
		    decoration->provenance == 0 ||
		    decoration->provenance > result->provenance_count ||
		    decoration->reserved != 0 ||
		    decoration->text.len == 0 ||
		    !mant_structured_valid_bytes(decoration->text) ||
		    !mant_structured_valid_utf8(decoration->text.ptr,
		    decoration->text.len))
			return 0;
	}
	return 1;
}
