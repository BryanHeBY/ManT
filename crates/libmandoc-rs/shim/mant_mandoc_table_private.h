#ifndef MANT_MANDOC_TABLE_PRIVATE_H
#define MANT_MANDOC_TABLE_PRIVATE_H

/*
 * Upstream: tbl_data.c revision 1.67, tbl_data(), and tbl_html.c
 * revision 1.40, print_tbl().
 * Role: shared private interpretation of the two native spellings for a
 * vertical continuation cell.  Both the owned AST snapshot and the execution
 * report must expose the same parser fact.  Include this private helper only
 * after the upstream tbl.h and string.h definitions.
 */
static inline int
mant_mandoc_tbl_cell_is_vertical_continuation(const struct tbl_dat *cell)
{
	return cell != NULL &&
	    ((cell->layout != NULL && cell->layout->pos == TBL_CELL_DOWN) ||
	    (cell->string != NULL && strcmp(cell->string, "\\^") == 0));
}

#endif
