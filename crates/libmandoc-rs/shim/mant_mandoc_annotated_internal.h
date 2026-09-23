/* Internal result ownership; session alone transfers its common shell. */
#ifndef MANT_MANDOC_ANNOTATED_INTERNAL_H
#define MANT_MANDOC_ANNOTATED_INTERNAL_H

#include "mant_mandoc_annotated.h"
#include "mant_mandoc_structured_internal.h"

#define MANT_ANNOTATED_MAGIC 0x4d414e31U

struct mant_annotated_result {
	uint32_t magic;
	uint32_t checked;
	struct mant_structured_result *common;
	struct mant_annotated_display *display;
	struct mant_annotated_mark *marks;
	uint32_t mark_count;
	uint64_t coverage_checked;
	uint64_t coverage_unverified;
};

int mant_annotated_result_is_valid(const struct mant_annotated_result *);

#endif
