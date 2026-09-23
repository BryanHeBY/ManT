/* Private tbl row/cell ownership at the native formatter boundary. */
#ifndef MANT_MANDOC_STRUCTURED_TABLE_H
#define MANT_MANDOC_STRUCTURED_TABLE_H

#include "mant_mandoc_structured_session.h"

struct roff_node;
struct tbl_dat;

int mant_structured_table_enter(struct structured_session *,
    const struct roff_node *);
void mant_structured_table_leave(struct structured_session *,
    const struct roff_node *);
void mant_structured_table_cell(struct structured_session *,
    const struct tbl_dat *, int);

#endif
