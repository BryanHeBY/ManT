/* Bounded per-call sink used by the optional embedded renderers. */
#ifndef MANT_MANDOC_OUTPUT_H
#define MANT_MANDOC_OUTPUT_H

#include <stddef.h>

struct mant_mandoc_output;

struct mant_mandoc_output *mant_mandoc_output_alloc(size_t);
int mant_mandoc_output_begin(struct mant_mandoc_output *);
void mant_mandoc_output_write(const void *, size_t);
void mant_mandoc_output_end(void);
const unsigned char *mant_mandoc_output_data(
    const struct mant_mandoc_output *);
size_t mant_mandoc_output_length(const struct mant_mandoc_output *);
int mant_mandoc_output_status(const struct mant_mandoc_output *);
void mant_mandoc_output_free(struct mant_mandoc_output *);

#endif
