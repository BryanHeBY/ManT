int
mant_mandoc_node_snapshot(struct mant_mandoc_document *document,
    const struct mant_mandoc_node *node,
    struct mant_mandoc_node_view *view)
{
	const struct roff_node *source;

	if (document == NULL || document->parser == NULL || node == NULL ||
	    view == NULL)
		return 0;
	source = (const struct roff_node *)node;
	memset(view, 0, sizeof(*view));
	view->kind = (int)source->type;
	if (source->sec < SEC_NONE || source->sec >= SEC__MAX ||
	    (source->end != ENDBODY_NOT && source->end != ENDBODY_SPACE))
		return 0;
	view->section = (int)source->sec;
	view->end_kind = (int)source->end;
	if (source->end != ENDBODY_NOT)
		view->end_body = (const struct mant_mandoc_node *)source->body;
	if (source->tok == MDOC__T && source->parent != NULL &&
	    source->parent->tok == MDOC_Rs && source->parent->norm != NULL)
		view->reference_quotes_title =
		    source->parent->norm->Rs.quote_T != 0;
	if (source->type != ROFFT_ROOT && source->tok != TOKEN_NONE)
		view->macro_name = roff_name[source->tok];
	if (source->type == ROFFT_TEXT || source->type == ROFFT_COMMENT)
		view->text = source->string;
	view->tag = source->tag;
	view->line = source->line;
	view->column = source->pos + 1;
	view->flow_epoch = source->flow_epoch;
	view->table_escape = source->type == ROFFT_TBL ? source->tbl_escape : -1;
	if (source->type == ROFFT_TBL && source->span != NULL) {
		const struct tbl_dat *dat;

		/*
		 * tbl_term.c positions the whole table from the shared final
		 * options, including a T& layout restart or a leading rule.
		 * These facts belong to the table, not an individual cell.
		 */
		view->table_alignment =
		    (source->span->opts->opts & TBL_OPT_CENTRE) != 0;
		view->table_column_count = source->span->opts->cols;
		/* tbl_opts.c aliases frame/box and doubleframe/doublebox. */
		view->table_frame = (source->span->opts->opts & TBL_OPT_DBOX) ?
		    2 : (source->span->opts->opts & TBL_OPT_BOX) ? 1 : 0;
		view->table_allbox =
		    (source->span->opts->opts & TBL_OPT_ALLBOX) != 0;
		/* tbl_term.c consumes row.vert and each slot's trailing vert. */
		if (source->span->layout != NULL) {
			view->table_left_border = source->span->layout->vert;
			view->table_layout_cells =
			    (const struct mant_mandoc_table_layout_cell *)
			    source->span->layout->first;
		}

		switch (source->span->pos) {
		case TBL_SPAN_DATA: {
			const struct tbl_cell *layout_cell;
			int layout_rule;

			/*
			 * tbl_data.c represents an all-rule layout row as a
			 * data span.  tbl_term.c renders each layout cell,
			 * including mixed _/= rows, and ignores any tbl_dat
			 * operands that getdata() happened to retain.
			 */
			layout_cell = source->span->layout == NULL ? NULL :
			    source->span->layout->first;
			layout_rule = layout_cell != NULL;
			for (; layout_cell != NULL; layout_cell = layout_cell->next)
				if (layout_cell->pos != TBL_CELL_HORIZ &&
				    layout_cell->pos != TBL_CELL_DHORIZ) {
					layout_rule = 0;
					break;
				}
			if (layout_rule) {
				view->table_row_kind =
				    MANT_MANDOC_TABLE_ROW_LAYOUT_RULE;
				view->table_rule_cells =
				    (const struct mant_mandoc_table_rule_cell *)
				    source->span->layout->first;
			} else
				view->table_row_kind = MANT_MANDOC_TABLE_ROW_DATA;
			break;
		}
		case TBL_SPAN_HORIZ:
			view->table_row_kind = MANT_MANDOC_TABLE_ROW_HORIZ;
			break;
		case TBL_SPAN_DHORIZ:
			view->table_row_kind = MANT_MANDOC_TABLE_ROW_DHORIZ;
			break;
		}

		if (source->span->pos == TBL_SPAN_DATA) {
			view->table_source_recovery_safe = 1;
			for (dat = source->span->first; dat != NULL;
			    dat = dat->next)
				if (dat->source_safe == 0) {
					view->table_source_recovery_safe = 0;
					break;
				}
		}
	}
	view->flags = snapshot_node_flags(source);
	snapshot_normalized_data(view, source);
	if (source->type == ROFFT_TBL && source->span != NULL &&
	    view->table_row_kind == MANT_MANDOC_TABLE_ROW_DATA)
		view->table_cells =
		    (const struct mant_mandoc_table_cell *)source->span->first;
	else if (source->type == ROFFT_EQN)
		view->equation = (const struct mant_mandoc_eqn_box *)source->eqn;
	view->child = (const struct mant_mandoc_node *)source->child;
	view->next = (const struct mant_mandoc_node *)source->next;
	return 1;
}

int
mant_mandoc_table_cell_snapshot(const struct mant_mandoc_document *document,
    const struct mant_mandoc_table_cell *cell,
    struct mant_mandoc_table_cell_view *view)
{
	const struct tbl_dat *source;

	if (document == NULL || document->parser == NULL || cell == NULL ||
	    view == NULL)
		return 0;
	source = (const struct tbl_dat *)cell;
	memset(view, 0, sizeof(*view));
	if (source->layout != NULL)
		view->column = (unsigned int)source->layout->col;
	view->text = source->string;
	/* Layout rules override data, including otherwise printable strings. */
	if (source->layout != NULL && source->layout->pos == TBL_CELL_HORIZ)
		view->kind = 2;
	else if (source->layout != NULL && source->layout->pos == TBL_CELL_DHORIZ)
		view->kind = 3;
	else {
		switch (source->pos) {
		case TBL_DATA_NONE: view->kind = 1; break;
		case TBL_DATA_HORIZ: view->kind = 2; break;
		case TBL_DATA_DHORIZ: view->kind = 3; break;
		case TBL_DATA_NHORIZ: view->kind = 4; break;
		case TBL_DATA_NDHORIZ: view->kind = 5; break;
		default: break;
		}
	}
	view->text_block = source->block;
	view->source_recovery_safe = source->source_safe;
	view->vertical_continuation =
	    (source->layout != NULL && source->layout->pos == TBL_CELL_DOWN) ||
	    (source->string != NULL && !strcmp(source->string, "\\^"));
	view->column_span = source->hspans < 0 ? 1U :
	    (unsigned int)source->hspans + 1U;
	view->row_span = source->vspans < 0 ? 1U :
	    (unsigned int)source->vspans + 1U;
	if (source->layout != NULL &&
	    source->layout->pos == TBL_CELL_CENTRE)
		view->alignment = 1;
	else if (source->layout != NULL &&
	    (source->layout->pos == TBL_CELL_RIGHT ||
	     source->layout->pos == TBL_CELL_NUMBER))
		view->alignment = 2;
	if (source->layout != NULL)
		switch (source->layout->font) {
		case ESCAPE_FONTROMAN: view->font = 1; break;
		case ESCAPE_FONTBOLD: view->font = 2; break;
		case ESCAPE_FONTITALIC: view->font = 3; break;
		case ESCAPE_FONTBI: view->font = 4; break;
		case ESCAPE_FONTCR: view->font = 5; break;
		case ESCAPE_FONTCB: view->font = 6; break;
		case ESCAPE_FONTCI: view->font = 7; break;
		default: return 0;
		}
	view->next = (const struct mant_mandoc_table_cell *)source->next;
	return 1;
}

int
mant_mandoc_table_layout_cell_snapshot(
    const struct mant_mandoc_document *document,
    const struct mant_mandoc_table_layout_cell *cell,
    struct mant_mandoc_table_layout_cell_view *view)
{
	const struct tbl_cell *source;

	if (document == NULL || document->parser == NULL || cell == NULL ||
	    view == NULL)
		return 0;
	source = (const struct tbl_cell *)cell;
	if (source->col < 0)
		return 0;
	memset(view, 0, sizeof(*view));
	view->column = (unsigned int)source->col;
	view->minimum_width = source->width;
	view->right_border = source->vert;
	view->next = (const struct mant_mandoc_table_layout_cell *)source->next;
	return 1;
}

int
mant_mandoc_eqn_box_snapshot(const struct mant_mandoc_document *document,
    const struct mant_mandoc_eqn_box *box,
    struct mant_mandoc_eqn_box_view *view)
{
	const struct eqn_box *source;

	if (document == NULL || document->parser == NULL || box == NULL ||
	    view == NULL)
		return 0;
	source = (const struct eqn_box *)box;
	memset(view, 0, sizeof(*view));
	/* These values are private shim discriminants, checked again by Rust. */
	switch (source->type) {
	case EQN_TEXT: view->kind = 0; break;
	case EQN_SUBEXPR: view->kind = 1; break;
	case EQN_LIST: view->kind = 2; break;
	case EQN_PILE: view->kind = 3; break;
	case EQN_MATRIX: view->kind = 4; break;
	default: return 0;
	}
	switch (source->font) {
	case EQNFONT_NONE: view->font = 0; break;
	case EQNFONT_ROMAN: view->font = 1; break;
	case EQNFONT_BOLD: view->font = 2; break;
	case EQNFONT_FAT: view->font = 3; break;
	case EQNFONT_ITALIC: view->font = 4; break;
	default: return 0;
	}
	switch (source->pos) {
	case EQNPOS_NONE: view->position = 0; break;
	case EQNPOS_SUP: view->position = 1; break;
	case EQNPOS_SUBSUP: view->position = 2; break;
	case EQNPOS_SUB: view->position = 3; break;
	case EQNPOS_TO: view->position = 4; break;
	case EQNPOS_FROM: view->position = 5; break;
	case EQNPOS_FROMTO: view->position = 6; break;
	case EQNPOS_OVER: view->position = 7; break;
	case EQNPOS_SQRT: view->position = 8; break;
	default: return 0;
	}
	view->size = source->size;
	view->expected_args = source->expectargs;
	view->actual_args = source->args;
	view->text = source->text;
#ifdef MANT_MANDOC_COMPAT_GNU_EQN
	view->gnu_ldots = source->gnu_ldots;
#else
	/* eqn.c retains token evidence; disabled compatibility cannot apply it. */
	view->gnu_ldots = 0;
#endif
	view->left = source->left;
	view->right = source->right;
	view->top = source->top;
	view->bottom = source->bottom;
	view->first = (const struct mant_mandoc_eqn_box *)source->first;
	view->next = (const struct mant_mandoc_eqn_box *)source->next;
	return 1;
}

int
mant_mandoc_table_rule_cell_snapshot(
    const struct mant_mandoc_document *document,
    const struct mant_mandoc_table_rule_cell *cell,
    struct mant_mandoc_table_rule_cell_view *view)
{
	const struct tbl_cell *source;

	if (document == NULL || document->parser == NULL || cell == NULL ||
	    view == NULL)
		return 0;
	source = (const struct tbl_cell *)cell;
	memset(view, 0, sizeof(*view));
	if (source->pos == TBL_CELL_HORIZ)
		view->kind = 1;
	else if (source->pos == TBL_CELL_DHORIZ)
		view->kind = 2;
	else
		return 0;
	view->next = (const struct mant_mandoc_table_rule_cell *)source->next;
	return 1;
}

