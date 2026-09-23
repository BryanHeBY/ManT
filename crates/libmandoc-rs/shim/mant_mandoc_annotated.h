/* Private, versioned ABI for one checked native display result. */
#ifndef MANT_MANDOC_ANNOTATED_H
#define MANT_MANDOC_ANNOTATED_H

#include "mant_mandoc_annotated_display.h"
#include "mant_mandoc_annotated_collector.h"
#include "mant_mandoc_structured.h"

struct mant_annotated_result;

enum mant_annotated_coverage_producer {
	MANT_ANNOTATED_COVERAGE_NATIVE = 1,
	MANT_ANNOTATED_COVERAGE_CODEC = 2,
	MANT_ANNOTATED_COVERAGE_VALIDATOR = 3
};

enum mant_annotated_coverage_dimension {
	MANT_ANNOTATED_COVERAGE_SECTION = 1,
	MANT_ANNOTATED_COVERAGE_OWNER_BOUNDARY = 2,
	MANT_ANNOTATED_COVERAGE_DECLARATION = 3,
	MANT_ANNOTATED_COVERAGE_LINK = 4,
	MANT_ANNOTATED_COVERAGE_ANCHOR = 5,
	MANT_ANNOTATED_COVERAGE_RELATION = 6,
	MANT_ANNOTATED_COVERAGE_SOURCE = 7,
	MANT_ANNOTATED_COVERAGE_JOIN = 8
};

enum mant_annotated_coverage_state {
	MANT_ANNOTATED_COVERAGE_CHECKED = 1,
	MANT_ANNOTATED_COVERAGE_NOT_APPLICABLE = 2,
	MANT_ANNOTATED_COVERAGE_UNVERIFIED = 3,
	MANT_ANNOTATED_COVERAGE_PENDING = 4
};

enum mant_annotated_coverage_reason {
	MANT_ANNOTATED_COVERAGE_NOT_OBSERVED = 1,
	MANT_ANNOTATED_COVERAGE_REASON_UNVERIFIED = 2,
	MANT_ANNOTATED_COVERAGE_REJECTED = 3,
	MANT_ANNOTATED_COVERAGE_AMBIGUOUS_SURVIVAL = 4
};

enum mant_annotated_coverage_scope {
	MANT_ANNOTATED_COVERAGE_DOCUMENT = 1,
	MANT_ANNOTATED_COVERAGE_SECTION_SCOPE = 2,
	MANT_ANNOTATED_COVERAGE_OWNER_SCOPE = 3,
	MANT_ANNOTATED_COVERAGE_REGION_SCOPE = 4,
	MANT_ANNOTATED_COVERAGE_SOURCE_SCOPE = 5
};

struct mant_annotated_coverage_check {
	uint32_t producer;
	uint32_t dimension;
	uint32_t state;
	uint32_t reserved;
};

struct mant_annotated_coverage_issue {
	uint32_t producer;
	uint32_t dimension;
	uint32_t reason;
	uint32_t scope;
	uint32_t scope_key;
	uint32_t source;
	uint32_t line;
	uint32_t column;
};

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
	struct mant_slice_view coverage_checks;
	struct mant_slice_view coverage_issues;
	struct mant_annotated_display_view display;
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
size_t mant_annotated_offsetof_mark_table_offset(void);
size_t mant_annotated_sizeof_coverage_check(void);
size_t mant_annotated_alignof_coverage_check(void);
size_t mant_annotated_sizeof_coverage_issue(void);
size_t mant_annotated_alignof_coverage_issue(void);
size_t mant_annotated_offsetof_result_view_coverage_checks(void);
size_t mant_annotated_offsetof_result_view_coverage_issues(void);

#endif
