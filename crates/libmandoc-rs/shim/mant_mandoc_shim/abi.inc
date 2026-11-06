size_t
mant_mandoc_node_view_size(void)
{
	return sizeof(struct mant_mandoc_node_view);
}

size_t
mant_mandoc_node_view_align(void)
{
	return MANT_VIEW_ALIGNOF(struct mant_mandoc_node_view);
}

const size_t *
mant_mandoc_node_view_offsets(size_t *count)
{
	static const size_t offsets[] = {
		offsetof(struct mant_mandoc_node_view, kind),
		offsetof(struct mant_mandoc_node_view, section),
		offsetof(struct mant_mandoc_node_view, end_kind),
		offsetof(struct mant_mandoc_node_view, end_body),
		offsetof(struct mant_mandoc_node_view, reference_quotes_title),
		offsetof(struct mant_mandoc_node_view, macro_name),
		offsetof(struct mant_mandoc_node_view, text),
		offsetof(struct mant_mandoc_node_view, tag),
		offsetof(struct mant_mandoc_node_view, line),
		offsetof(struct mant_mandoc_node_view, column),
		offsetof(struct mant_mandoc_node_view, flow_epoch),
		offsetof(struct mant_mandoc_node_view, table_escape),
		offsetof(struct mant_mandoc_node_view, table_source_recovery_safe),
		offsetof(struct mant_mandoc_node_view, table_row_kind),
		offsetof(struct mant_mandoc_node_view, table_alignment),
		offsetof(struct mant_mandoc_node_view, table_column_count),
		offsetof(struct mant_mandoc_node_view, table_decimal_point),
		offsetof(struct mant_mandoc_node_view, table_width_edges),
		offsetof(struct mant_mandoc_node_view, table_frame),
		offsetof(struct mant_mandoc_node_view, table_allbox),
		offsetof(struct mant_mandoc_node_view, table_left_border),
		offsetof(struct mant_mandoc_node_view, table_layout_cells),
		offsetof(struct mant_mandoc_node_view, flags),
		offsetof(struct mant_mandoc_node_view, list_kind),
		offsetof(struct mant_mandoc_node_view, definition_list_style),
		offsetof(struct mant_mandoc_node_view, display_kind),
		offsetof(struct mant_mandoc_node_view, font_kind),
		offsetof(struct mant_mandoc_node_view, author_mode),
		offsetof(struct mant_mandoc_node_view, compact),
		offsetof(struct mant_mandoc_node_view, offset),
		offsetof(struct mant_mandoc_node_view, width),
		offsetof(struct mant_mandoc_node_view, cols),
		offsetof(struct mant_mandoc_node_view, ncols),
		offsetof(struct mant_mandoc_node_view, enclosure_open),
		offsetof(struct mant_mandoc_node_view, enclosure_close),
		offsetof(struct mant_mandoc_node_view, equation),
		offsetof(struct mant_mandoc_node_view, table_cells),
		offsetof(struct mant_mandoc_node_view, table_rule_data),
		offsetof(struct mant_mandoc_node_view, table_rule_cells),
		offsetof(struct mant_mandoc_node_view, child),
		offsetof(struct mant_mandoc_node_view, next),
	};
	if (count != NULL)
		*count = sizeof(offsets) / sizeof(offsets[0]);
	return offsets;
}

size_t
mant_mandoc_table_cell_view_size(void)
{
	return sizeof(struct mant_mandoc_table_cell_view);
}

size_t
mant_mandoc_table_cell_view_align(void)
{
	return MANT_VIEW_ALIGNOF(struct mant_mandoc_table_cell_view);
}

const size_t *
mant_mandoc_table_cell_view_offsets(size_t *count)
{
	static const size_t offsets[] = {
		offsetof(struct mant_mandoc_table_cell_view, column),
		offsetof(struct mant_mandoc_table_cell_view, text),
		offsetof(struct mant_mandoc_table_cell_view, kind),
		offsetof(struct mant_mandoc_table_cell_view, text_block),
		offsetof(struct mant_mandoc_table_cell_view, source_recovery_safe),
		offsetof(struct mant_mandoc_table_cell_view, vertical_continuation),
		offsetof(struct mant_mandoc_table_cell_view, column_span),
		offsetof(struct mant_mandoc_table_cell_view, row_span),
		offsetof(struct mant_mandoc_table_cell_view, alignment),
		offsetof(struct mant_mandoc_table_cell_view, font),
		offsetof(struct mant_mandoc_table_cell_view, next),
	};
	if (count != NULL)
		*count = sizeof(offsets) / sizeof(offsets[0]);
	return offsets;
}

size_t
mant_mandoc_table_layout_cell_view_size(void)
{
	return sizeof(struct mant_mandoc_table_layout_cell_view);
}

size_t
mant_mandoc_table_layout_cell_view_align(void)
{
	return MANT_VIEW_ALIGNOF(struct mant_mandoc_table_layout_cell_view);
}

const size_t *
mant_mandoc_table_layout_cell_view_offsets(size_t *count)
{
	static const size_t offsets[] = {
		offsetof(struct mant_mandoc_table_layout_cell_view, column),
		offsetof(struct mant_mandoc_table_layout_cell_view, kind),
		offsetof(struct mant_mandoc_table_layout_cell_view, minimum_width),
		offsetof(struct mant_mandoc_table_layout_cell_view, spacing),
		offsetof(struct mant_mandoc_table_layout_cell_view, width_flags),
		offsetof(struct mant_mandoc_table_layout_cell_view, right_border),
		offsetof(struct mant_mandoc_table_layout_cell_view, next),
	};
	if (count != NULL)
		*count = sizeof(offsets) / sizeof(offsets[0]);
	return offsets;
}

size_t
mant_mandoc_table_rule_cell_view_size(void)
{
	return sizeof(struct mant_mandoc_table_rule_cell_view);
}

size_t
mant_mandoc_table_rule_cell_view_align(void)
{
	return MANT_VIEW_ALIGNOF(struct mant_mandoc_table_rule_cell_view);
}

const size_t *
mant_mandoc_table_rule_cell_view_offsets(size_t *count)
{
	static const size_t offsets[] = {
		offsetof(struct mant_mandoc_table_rule_cell_view, kind),
		offsetof(struct mant_mandoc_table_rule_cell_view, next),
	};
	if (count != NULL)
		*count = sizeof(offsets) / sizeof(offsets[0]);
	return offsets;
}

size_t
mant_mandoc_eqn_box_view_size(void)
{
	return sizeof(struct mant_mandoc_eqn_box_view);
}

size_t
mant_mandoc_eqn_box_view_align(void)
{
	return MANT_VIEW_ALIGNOF(struct mant_mandoc_eqn_box_view);
}

const size_t *
mant_mandoc_eqn_box_view_offsets(size_t *count)
{
	static const size_t offsets[] = {
		offsetof(struct mant_mandoc_eqn_box_view, kind),
		offsetof(struct mant_mandoc_eqn_box_view, font),
		offsetof(struct mant_mandoc_eqn_box_view, position),
		offsetof(struct mant_mandoc_eqn_box_view, size),
		offsetof(struct mant_mandoc_eqn_box_view, expected_args),
		offsetof(struct mant_mandoc_eqn_box_view, actual_args),
		offsetof(struct mant_mandoc_eqn_box_view, text),
		offsetof(struct mant_mandoc_eqn_box_view, gnu_ldots),
		offsetof(struct mant_mandoc_eqn_box_view, left),
		offsetof(struct mant_mandoc_eqn_box_view, right),
		offsetof(struct mant_mandoc_eqn_box_view, top),
		offsetof(struct mant_mandoc_eqn_box_view, bottom),
		offsetof(struct mant_mandoc_eqn_box_view, first),
		offsetof(struct mant_mandoc_eqn_box_view, next),
	};
	if (count != NULL)
		*count = sizeof(offsets) / sizeof(offsets[0]);
	return offsets;
}

