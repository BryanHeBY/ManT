/* Private source, include, provenance, and diagnostic boundary. */
#ifndef MANT_MANDOC_STRUCTURED_SOURCE_H
#define MANT_MANDOC_STRUCTURED_SOURCE_H

#include <stdarg.h>

#include "mant_mandoc_structured_session.h"

struct mparse;

struct structured_session *mant_structured_active_session(void);
int mant_structured_validate_input(struct structured_session *);
int mant_structured_read_input(struct structured_session *, struct mparse *,
    uint32_t);
int mant_structured_check_source_positions(struct structured_session *);
void mant_structured_observe_source_line(void *, uint32_t, int, size_t);
void mant_structured_observe_diagnostic(void *, enum mandocerr,
    enum mandoclevel, uint32_t, int, int, const char *, const char *,
    va_list *);

#endif
