/* Private native list/item ownership adapter. */
#ifndef MANT_MANDOC_STRUCTURED_STRUCTURE_H
#define MANT_MANDOC_STRUCTURED_STRUCTURE_H

#include "mant_mandoc_structured_session.h"

int mant_structured_enter_node(struct structured_session *,
    const struct roff_node *);
void mant_structured_leave_node(struct structured_session *,
    const struct roff_node *);
struct structured_node_context *mant_structured_current_context(
    struct structured_session *);

#endif
