/* Private final-result builder boundary. */
#ifndef MANT_MANDOC_STRUCTURED_BUILDER_H
#define MANT_MANDOC_STRUCTURED_BUILDER_H

#include "mant_mandoc_structured_session.h"

struct roff_meta;
struct roff_node;

int mant_structured_copy_metadata(struct structured_session *,
    const struct roff_meta *);
uint32_t mant_structured_append_provenance(struct structured_session *,
    const struct roff_node *, int);
uint32_t mant_structured_limit_u32(uint64_t);
uint32_t mant_structured_append_owner(struct structured_session *, uint32_t,
    uint32_t);
uint32_t mant_structured_append_block(struct structured_session *, uint32_t,
    uint32_t, uint32_t, uint32_t, uint32_t);
uint32_t mant_structured_append_root(struct structured_session *, uint32_t,
    uint32_t, uint32_t);
int mant_structured_open_content_root(struct structured_session *, int,
    uint32_t);
int mant_structured_close_term_root(struct structured_session *, uint32_t,
    uint32_t);
int mant_structured_finish_term_roots(struct structured_session *);
int mant_structured_append_atom(struct structured_session *, uint32_t,
    uint32_t, uint32_t, uint32_t, uint32_t, uint32_t, const uint8_t *,
    size_t, const uint8_t *, size_t, int);

#endif
