/* Private, bounded post-device display surface for annotated rendering. */
#ifndef MANT_MANDOC_ANNOTATED_DISPLAY_H
#define MANT_MANDOC_ANNOTATED_DISPLAY_H

#include <stddef.h>
#include <stdint.h>

struct mant_annotated_display;

enum mant_annotated_display_role {
	MANT_ANNOTATED_BODY = 1,
	MANT_ANNOTATED_HEADER = 2,
	MANT_ANNOTATED_FOOTER = 3,
	MANT_ANNOTATED_DIRECT_DRAW = 4,
	/* Device advance proven to be layout, not an authored glyph. */
	MANT_ANNOTATED_LAYOUT = 5
};

enum mant_annotated_display_status {
	MANT_ANNOTATED_DISPLAY_OK = 0,
	MANT_ANNOTATED_DISPLAY_INVALID = 1,
	MANT_ANNOTATED_DISPLAY_BUDGET = 2,
	MANT_ANNOTATED_DISPLAY_ALLOC = 3,
	MANT_ANNOTATED_DISPLAY_UNSAFE_CONTROL = 4
};

/* Keep the style bits compatible with the private structured ABI. */
#define MANT_ANNOTATED_STYLE_BOLD (1U << 0)
#define MANT_ANNOTATED_STYLE_UNDERLINE (1U << 3)
#define MANT_ANNOTATED_FONT_STROKE (1U << 0)

struct mant_annotated_display_label {
	uint32_t owner;
	uint32_t link;
	uint32_t source;
	uint32_t style;
	uint32_t role;
	/* Input-only execution identity; zero means that folding is unproven. */
	uint64_t glyph_origin;
	uint32_t flags;
	uint32_t reserved;
};

/* Private execution evidence, never inferred from final columns.  A zero
 * predecessor means the relationship to an earlier survivor is unproved. */
enum mant_annotated_display_join {
	MANT_DISPLAY_JOIN_UNKNOWN = 0,
	MANT_DISPLAY_JOIN_DIRECT = 1,
	MANT_DISPLAY_JOIN_SEPARATOR = 2,
	MANT_DISPLAY_JOIN_HARD = 3
};

struct mant_annotated_display_edge {
	uint64_t predecessor_origin;
	uint64_t separator_spaces;
	uint32_t join;
	uint32_t separator_owner;
	uint32_t separator_link;
	uint32_t same_origin_continuation;
	uint32_t reserved;
};

/* One private endpoint per final run; borrowed until display_free(). */
struct mant_annotated_run_endpoint {
	uint64_t first_origin;
	uint64_t last_origin;
	struct mant_annotated_display_edge first_edge;
};

struct mant_annotated_display_limits {
	uint64_t max_input_bytes;
	uint64_t max_result_bytes;
	uint64_t max_allocated_bytes;
	uint64_t max_work;
	uint32_t max_rows;
	uint32_t max_runs;
	uint32_t max_row_columns;
};

/* Match the selected native device's term_ascii.c::utf8_getwidth rule. */
typedef size_t (*mant_annotated_display_width)(void *, uint32_t);
/* Called before each unit of normalization work; no rollback on failure. */
typedef int (*mant_annotated_display_work_charge)(void *, uint64_t);

struct mant_annotated_display_row {
	uint32_t key;
	uint32_t first_run;
	uint32_t run_count;
	uint32_t column_count;
	uint32_t break_after;
};

struct mant_annotated_display_run {
	uint32_t key;
	uint32_t column;
	uint32_t width;
	uint32_t reserved;
	uint64_t byte_start;
	uint64_t byte_count;
	struct mant_annotated_display_label label;
};

/* All pointers are borrowed and remain valid only until display_free(). */
struct mant_annotated_display_view {
	const uint8_t *bytes;
	uint64_t byte_count;
	const struct mant_annotated_display_row *rows;
	uint32_t row_count;
	const struct mant_annotated_display_run *runs;
	uint32_t run_count;
	uint64_t input_bytes;
	uint64_t work;
	uint64_t peak_allocated_bytes;
};

/* An unresolved location in the native device, before final row sealing.
 * `row_before` counts already sealed body rows; `active` selects their
 * in-progress successor.  It is not a public DisplayPoint. */
struct mant_annotated_display_checkpoint {
	uint32_t row_before;
	uint32_t column;
	uint32_t active;
};

struct mant_annotated_display *mant_annotated_display_new(
	const struct mant_annotated_display_limits *,
	mant_annotated_display_width, void *);
int mant_annotated_display_set_work_charge(struct mant_annotated_display *,
	mant_annotated_display_work_charge, void *);
int mant_annotated_display_write(struct mant_annotated_display *,
	const void *, size_t, struct mant_annotated_display_label);
int mant_annotated_display_write_join(struct mant_annotated_display *,
	const void *, size_t, struct mant_annotated_display_label,
	struct mant_annotated_display_edge);
int mant_annotated_display_checkpoint(const struct mant_annotated_display *,
	uint32_t, struct mant_annotated_display_checkpoint *);
/* Rightmost surviving non-layout glyph on the unsealed physical row.  A
 * mixed-owner overprint is ambiguous and returns owner zero.  Work is charged
 * through the display's normal cumulative budget. */
int mant_annotated_display_trailing_owner(struct mant_annotated_display *,
	uint32_t *);
const struct mant_annotated_run_endpoint *mant_annotated_display_endpoints(
	const struct mant_annotated_display *, uint32_t *);
int mant_annotated_display_finish(struct mant_annotated_display *,
	struct mant_annotated_display_view *);
enum mant_annotated_display_status mant_annotated_display_status(
	const struct mant_annotated_display *);
void mant_annotated_display_failure(const struct mant_annotated_display *,
	uint32_t *, uint64_t *, uint64_t *);
uint64_t mant_annotated_display_allocated_bytes(
	const struct mant_annotated_display *);
uint64_t mant_annotated_display_work(
	const struct mant_annotated_display *);
void mant_annotated_display_free(struct mant_annotated_display *);

#endif
