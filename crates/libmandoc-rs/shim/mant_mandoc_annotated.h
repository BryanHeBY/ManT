/* Private, versioned ABI for one checked native display result. */
#ifndef MANT_MANDOC_ANNOTATED_H
#define MANT_MANDOC_ANNOTATED_H

#include "mant_mandoc_annotated_display.h"
#include "mant_mandoc_annotated_collector.h"
#include "mant_mandoc_structured.h"

struct mant_annotated_result;

struct mant_annotated_result_view {
	uint32_t root_source;
	uint32_t profile;
	uint32_t width;
	uint32_t reserved;
	struct mant_structured_metadata_view metadata;
	struct mant_slice_view sources;
	struct mant_slice_view spans;
	struct mant_slice_view provenances;
	struct mant_slice_view diagnostics;
	struct mant_slice_view marks;
	struct mant_annotated_display_view display;
	uint64_t coverage_checked;
	uint64_t coverage_unverified;
};

uint32_t mant_annotated_abi_version(void);
uint32_t mant_annotated_render(const struct mant_structured_input_view *,
    const struct mant_structured_limits *, struct mant_annotated_result **,
    struct mant_structured_failure_view *);
uint32_t mant_annotated_result_check(const struct mant_annotated_result *,
    struct mant_structured_failure_view *);
uint32_t mant_annotated_result_view(const struct mant_annotated_result *,
    struct mant_annotated_result_view *);
void mant_annotated_result_free(struct mant_annotated_result *);

/* ABI tests use these instead of mirroring C's padding assumptions. */
size_t mant_annotated_sizeof_result_view(void);
size_t mant_annotated_alignof_result_view(void);
size_t mant_annotated_offsetof_result_view_display(void);
size_t mant_annotated_sizeof_display_row(void);
size_t mant_annotated_alignof_display_row(void);
size_t mant_annotated_offsetof_display_row_break_after(void);
size_t mant_annotated_sizeof_display_run(void);
size_t mant_annotated_alignof_display_run(void);
size_t mant_annotated_offsetof_display_run_label(void);
size_t mant_annotated_sizeof_display_label(void);
size_t mant_annotated_alignof_display_label(void);
size_t mant_annotated_offsetof_display_label_glyph_origin(void);
size_t mant_annotated_sizeof_mark(void);
size_t mant_annotated_alignof_mark(void);
size_t mant_annotated_offsetof_mark_name(void);

#endif
