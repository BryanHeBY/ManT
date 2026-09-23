/* Bounded per-call sink used by the optional embedded renderers. */
#ifndef MANT_MANDOC_OUTPUT_H
#define MANT_MANDOC_OUTPUT_H

#include <stddef.h>

struct mant_mandoc_output;

/* A sink consumes each native write before the next write begins. Returning
 * zero records a hard output failure; it must not retain the borrowed bytes
 * or call begin/write/end/free recursively on this thread. The implementation
 * detects recursive write/end/free and makes the active call fail closed. */
typedef int (*mant_mandoc_output_sink)(void *, const void *, size_t);

/* The device operation is evidence for annotation, not a request to lay out
 * content. The terminal formatter alone decides the bytes and their order. */
enum mant_mandoc_output_operation {
	MANT_OUTPUT_GENERIC = 0,
	MANT_OUTPUT_LETTER = 1,
	MANT_OUTPUT_ADVANCE = 2,
	MANT_OUTPUT_ENDLINE = 3
};

struct mant_mandoc_output *mant_mandoc_output_alloc(size_t);
struct mant_mandoc_output *mant_mandoc_output_alloc_sink(size_t,
    mant_mandoc_output_sink, void *);
int mant_mandoc_output_begin(struct mant_mandoc_output *);
void mant_mandoc_output_write(const void *, size_t);
void mant_mandoc_output_write_op(const void *, size_t,
    enum mant_mandoc_output_operation);
void mant_mandoc_output_utf8(int);
void mant_mandoc_output_utf8_op(int, enum mant_mandoc_output_operation);
enum mant_mandoc_output_operation mant_mandoc_output_current_operation(void);
size_t mant_mandoc_utf8_width(int);
const char *mant_mandoc_ctype_locale(void);
void mant_mandoc_output_end(void);
const unsigned char *mant_mandoc_output_data(
    const struct mant_mandoc_output *);
size_t mant_mandoc_output_length(const struct mant_mandoc_output *);
size_t mant_mandoc_output_attempted_length(const struct mant_mandoc_output *);
int mant_mandoc_output_status(const struct mant_mandoc_output *);
void mant_mandoc_output_free(struct mant_mandoc_output *);

#endif
