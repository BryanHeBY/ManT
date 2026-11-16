#ifndef MANT_MANDOC_DIAGNOSTIC_CAPTURE_H
#define MANT_MANDOC_DIAGNOSTIC_CAPTURE_H

#include <stdarg.h>

/* Include after mandoc.h. Capture before spelling any display prefix. */
int mant_mandoc_capture_diagnostic(enum mandocerr, enum mandoclevel,
    const char *, int, int, const char *, const char *, va_list *);

#endif
