static unsigned int
snapshot_node_flags(const struct roff_node *source)
{
	unsigned int flags;
	const struct tbl_span *previous;

	flags = 0;
	if (source->flags & NODE_NOSRC)
		flags |= MANT_MANDOC_NODE_GENERATED;
	if (source->flags & NODE_EOS)
		flags |= MANT_MANDOC_NODE_SENTENCE_END;
	if (source->flags & NODE_NOPRT)
		flags |= MANT_MANDOC_NODE_NO_PRINT;
	if (source->flags & NODE_NOFILL)
		flags |= MANT_MANDOC_NODE_NO_FILL;
	if (source->flags & NODE_BROKEN)
		flags |= MANT_MANDOC_NODE_BROKEN;
	if (source->flags & NODE_ID)
		flags |= MANT_MANDOC_NODE_DEEP_LINK_TARGET;
	if (source->flags & NODE_HREF)
		flags |= MANT_MANDOC_NODE_PERMALINK;
	if (source->flags & NODE_LINE)
		flags |= MANT_MANDOC_NODE_LINE_START;
	if (source->flags & NODE_DELIMO)
		flags |= MANT_MANDOC_NODE_DELIMITER_OPEN;
	if (source->flags & NODE_DELIMC)
		flags |= MANT_MANDOC_NODE_DELIMITER_CLOSE;
	if (source->flags & NODE_SYNPRETTY)
		flags |= MANT_MANDOC_NODE_SYNOPSIS_PRETTY;
	if (source->type == ROFFT_EQN && source->flags & NODE_EQN_INLINE)
		flags |= MANT_MANDOC_NODE_EQUATION_INLINE;
	if (source->type == ROFFT_TBL && source->span != NULL) {
		/* tbl_data.c links every span owned by one tbl_node, including
		 * leading whole-row rules and empty data rows.  A T& layout restart
		 * keeps the same chain.  Mark its actual first span so consumers do
		 * not attach a leading rule to the preceding native table. */
		previous = source->span->prev;
		if (previous == NULL)
			flags |= MANT_MANDOC_NODE_TABLE_START;
	}
	return flags;
}

static void
snapshot_normalized_data(struct mant_mandoc_node_view *view,
    const struct roff_node *source)
{
	if (source->norm == NULL)
		return;
	if (source->tok == MDOC_Bl) {
		view->compact = source->norm->Bl.comp;
		view->offset = source->norm->Bl.offs;
		view->width = source->norm->Bl.width;
		if (source->norm->Bl.type == LIST_column &&
		    source->type == ROFFT_BLOCK) {
			view->cols = source->norm->Bl.cols;
			view->ncols = source->norm->Bl.ncols;
		}
		switch (source->norm->Bl.type) {
		case LIST_bullet:
			view->list_kind = MANT_MANDOC_LIST_BULLET;
			break;
		case LIST_dash:
		case LIST_hyphen:
			view->list_kind = MANT_MANDOC_LIST_DASH;
			break;
		case LIST_enum:
			view->list_kind = MANT_MANDOC_LIST_ORDERED;
			break;
		case LIST_diag:
			view->definition_list_style =
			    MANT_MANDOC_DEFINITION_LIST_DIAG;
			view->list_kind = MANT_MANDOC_LIST_DEFINITION;
			break;
		case LIST_hang:
			view->definition_list_style =
			    MANT_MANDOC_DEFINITION_LIST_HANG;
			view->list_kind = MANT_MANDOC_LIST_DEFINITION;
			break;
		case LIST_inset:
			view->definition_list_style =
			    MANT_MANDOC_DEFINITION_LIST_INSET;
			view->list_kind = MANT_MANDOC_LIST_DEFINITION;
			break;
		case LIST_ohang:
			view->definition_list_style =
			    MANT_MANDOC_DEFINITION_LIST_OHANG;
			view->list_kind = MANT_MANDOC_LIST_DEFINITION;
			break;
		case LIST_tag:
			view->definition_list_style =
			    MANT_MANDOC_DEFINITION_LIST_TAG;
			view->list_kind = MANT_MANDOC_LIST_DEFINITION;
			break;
		case LIST_column:
			view->list_kind = MANT_MANDOC_LIST_COLUMN;
			break;
		case LIST_item:
			view->list_kind = MANT_MANDOC_LIST_PLAIN;
			break;
		case LIST__NONE:
		case LIST_MAX:
			break;
		}
	} else if (source->tok == MDOC_Bd) {
		view->compact = source->norm->Bd.comp;
		view->offset = source->norm->Bd.offs;
		switch (source->norm->Bd.type) {
		case DISP_unfilled:
			view->display_kind = MANT_MANDOC_DISPLAY_UNFILLED;
			break;
		case DISP_literal:
			view->display_kind = MANT_MANDOC_DISPLAY_LITERAL;
			break;
		case DISP_centered:
		case DISP_ragged:
		case DISP_filled:
			view->display_kind = MANT_MANDOC_DISPLAY_FILLED;
			break;
		case DISP__NONE:
			break;
		}
	} else if (source->tok == MDOC_Bf) {
		switch (source->norm->Bf.font) {
		case FONT_Em:
			view->font_kind = MANT_MANDOC_FONT_EMPHASIS;
			break;
		case FONT_Li:
			view->font_kind = MANT_MANDOC_FONT_LITERAL;
			break;
		case FONT_Sy:
			view->font_kind = MANT_MANDOC_FONT_SYMBOLIC;
			break;
		case FONT__NONE:
			break;
		}
	} else if (source->tok == MDOC_An) {
		switch (source->norm->An.auth) {
		case AUTH_split:
			view->author_mode = MANT_MANDOC_AUTHOR_SPLIT;
			break;
		case AUTH_nosplit:
			view->author_mode = MANT_MANDOC_AUTHOR_NOSPLIT;
			break;
		case AUTH__NONE:
			break;
		}
	} else if (source->tok == MDOC_En && source->norm->Es != NULL &&
	    source->norm->Es->child != NULL) {
		view->enclosure_open = source->norm->Es->child->string;
		if (source->norm->Es->child->next != NULL)
			view->enclosure_close =
			    source->norm->Es->child->next->string;
	}
}

