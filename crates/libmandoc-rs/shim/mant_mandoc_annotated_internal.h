/* Internal result ownership; session alone transfers its common shell. */
#ifndef MANT_MANDOC_ANNOTATED_INTERNAL_H
#define MANT_MANDOC_ANNOTATED_INTERNAL_H

#include "mant_mandoc_annotated.h"
#include "mant_mandoc_structured_internal.h"

#define MANT_ANNOTATED_MAGIC 0x4d414e31U

struct mant_annotated_result {
	uint32_t magic;
	uint32_t checked;
	/* A checked body survived, but native mark relations were discarded. */
	uint32_t annotation_degraded;
	struct mant_structured_result *common;
	struct mant_annotated_display *display;
	struct mant_annotated_mark *marks;
	uint32_t mark_count;
	struct mant_annotated_selection_part *selection_parts;
	uint32_t selection_part_count;
	uint8_t *join_text;
	uint64_t join_text_count;
	/* Producer × dimension table, including explicit downstream pending/N/A. */
	struct mant_annotated_coverage_check coverage_checks[24];
	struct mant_annotated_coverage_issue *coverage_issues;
	uint32_t coverage_issue_count;
	uint32_t coverage_issue_capacity;
};

int mant_annotated_result_is_valid(const struct mant_annotated_result *);
int mant_annotated_result_is_surface_valid(
	const struct mant_annotated_result *);
int mant_annotated_result_strip_annotations(
	struct mant_annotated_result *);
/* One native join rule is shared by selection construction and validation. */
uint32_t mant_annotated_native_join(
	const struct mant_annotated_run_endpoint *, uint32_t, uint32_t,
	uint32_t, uint32_t, int, uint64_t *);
int mant_annotated_build_selection_parts(struct structured_session *,
	struct mant_annotated_result *,
	const struct mant_annotated_display_view *);
int mant_annotated_coverage_build(struct structured_session *,
    struct mant_annotated_result *);
int mant_annotated_coverage_is_valid(const struct mant_annotated_result *);

#endif
