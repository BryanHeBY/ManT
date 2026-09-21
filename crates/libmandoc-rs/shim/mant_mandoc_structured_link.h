/* Private structured link construction boundary. */
#ifndef MANT_MANDOC_STRUCTURED_LINK_H
#define MANT_MANDOC_STRUCTURED_LINK_H

#include "mant_mandoc_structured_session.h"

struct roff_node;

const struct roff_node *mant_structured_link_node(const struct roff_node *);
uint32_t mant_structured_ensure_link(struct structured_session *,
    const struct roff_node *, uint32_t, uint32_t);
void mant_structured_record_link_ref(struct structured_session *, uint32_t);

#endif
