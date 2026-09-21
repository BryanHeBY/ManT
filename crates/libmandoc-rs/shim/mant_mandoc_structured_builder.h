/* Private final-result builder boundary. */
#ifndef MANT_MANDOC_STRUCTURED_BUILDER_H
#define MANT_MANDOC_STRUCTURED_BUILDER_H

#include "mant_mandoc_structured_session.h"

struct roff_meta;

int mant_structured_copy_metadata(struct structured_session *,
    const struct roff_meta *);

#endif
