/* Private structured link construction boundary. */
#ifndef MANT_MANDOC_STRUCTURED_LINK_H
#define MANT_MANDOC_STRUCTURED_LINK_H

#include "mant_mandoc_structured_session.h"

struct roff_node;

const struct roff_node *mant_structured_link_node(const struct roff_node *);
uint32_t mant_structured_existing_link(const struct structured_session *,
    const struct roff_node *);
uint32_t mant_structured_ensure_link(struct structured_session *,
    const struct roff_node *, uint32_t);
int mant_structured_finalize_links(struct structured_session *);

#endif
