/* Private structured link construction boundary. */
#ifndef MANT_MANDOC_STRUCTURED_LINK_H
#define MANT_MANDOC_STRUCTURED_LINK_H

#include "mant_mandoc_structured_session.h"

struct roff_node;
struct mant_bytes_view;

const struct roff_node *mant_structured_link_node(const struct roff_node *);
uint32_t mant_structured_existing_link(const struct structured_session *,
    const struct roff_node *);
uint32_t mant_structured_ensure_link(struct structured_session *,
    const struct roff_node *, uint32_t);
int mant_structured_finalize_links(struct structured_session *);
/* Shared, parser-alive target decoding; follows pinned html.c::print_encode. */
int mant_structured_copy_link_target(struct structured_session *,
    struct mant_bytes_view *, const struct roff_node *);
int mant_structured_copy_link_target_allow_empty(struct structured_session *,
    struct mant_bytes_view *, const struct roff_node *);
int mant_structured_copy_deroff_target(struct structured_session *,
    struct mant_bytes_view *, const struct roff_node *);

#endif
