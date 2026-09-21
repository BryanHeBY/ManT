/* Private zero-width address and heading-evidence boundary. */
#ifndef MANT_MANDOC_STRUCTURED_ADDRESS_H
#define MANT_MANDOC_STRUCTURED_ADDRESS_H

#include "mant_mandoc_structured_session.h"

struct roff_node;

void mant_structured_address_enter_node(struct structured_session *,
    const struct roff_node *);
void mant_structured_address_bind_item(struct structured_session *, uint32_t,
    uint32_t);
void mant_structured_address_root_opened(struct structured_session *,
    const struct roff_node *, int);
void mant_structured_address_before_atom(struct structured_session *, uint32_t,
    uint64_t);
int mant_structured_address_owner_needs_root(const struct structured_session *,
    uint32_t);
void mant_structured_address_finish_owner(struct structured_session *, uint32_t);
void mant_structured_address_finish(struct structured_session *);
void mant_structured_address_release(struct structured_session *);

#endif
