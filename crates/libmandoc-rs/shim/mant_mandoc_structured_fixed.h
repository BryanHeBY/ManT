/* Native physical geometry over the one logical content store. */
#ifndef MANT_MANDOC_STRUCTURED_FIXED_H
#define MANT_MANDOC_STRUCTURED_FIXED_H

#include "mant_mandoc_structured_session.h"

struct termp;
struct term_collector_event;
struct tbl_span;
struct structured_token;

int mant_structured_fixed_table_required(struct structured_session *,
    const struct tbl_span *);
uint32_t mant_structured_fixed_open_table(struct structured_session *,
    uint32_t, uint32_t, uint32_t, uint32_t);
void mant_structured_fixed_field(struct structured_session *, struct termp *,
    const struct term_collector_event *, struct structured_token *, uint32_t);
void mant_structured_fixed_draw(struct structured_session *, struct termp *,
    const struct term_collector_event *);
void mant_structured_fixed_endline(struct structured_session *, struct termp *,
    const struct term_collector_event *);
void mant_structured_fixed_commit(struct structured_session *,
    const struct structured_token *, uint32_t, uint32_t, uint32_t,
    uint32_t, uint32_t);

#endif
