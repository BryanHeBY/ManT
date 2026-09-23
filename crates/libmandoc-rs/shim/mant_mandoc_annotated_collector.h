/* Private, per-render terminal observation and post-device transfer. */
#ifndef MANT_MANDOC_ANNOTATED_COLLECTOR_H
#define MANT_MANDOC_ANNOTATED_COLLECTOR_H

#include "mant_mandoc_annotated_display.h"

struct structured_session;
struct termp;
struct term_collector_event;
struct mant_annotated_collector;

/* Keys are one-based and shared across these kinds.  A mark's source is a
 * SourceKey, not a generated source span or a proven final display point. */
enum mant_annotated_mark_kind {
	MANT_ANNOTATED_MARK_HEADING = 1,
	MANT_ANNOTATED_MARK_OWNER = 2,
	MANT_ANNOTATED_MARK_LINK = 3,
	MANT_ANNOTATED_MARK_ANCHOR = 4,
	MANT_ANNOTATED_MARK_REGION = 5
};

enum mant_annotated_region_kind {
	MANT_ANNOTATED_REGION_NONE = 0,
	MANT_ANNOTATED_REGION_HEADING_TITLE = 1,
	MANT_ANNOTATED_REGION_HEADING_BODY = 2,
	MANT_ANNOTATED_REGION_OWNER_TERM = 3,
	MANT_ANNOTATED_REGION_OWNER_BODY = 4,
	MANT_ANNOTATED_REGION_LIST = 5,
	MANT_ANNOTATED_REGION_LITERAL = 6,
	MANT_ANNOTATED_REGION_TABLE_SPAN = 7,
	MANT_ANNOTATED_REGION_EQUATION = 8,
	MANT_ANNOTATED_REGION_TABLE_CELL = 9
};

#define MANT_ANNOTATED_MARK_AUTHORED (1U << 0)
#define MANT_ANNOTATED_MARK_FINAL_POINT_UNVERIFIED (1U << 1)

struct mant_annotated_mark {
	uint32_t key;
	uint32_t kind;
	uint32_t parent;
	uint32_t owner;
	uint32_t source;
	uint32_t line;
	uint32_t column;
	uint32_t token;
	uint32_t region_kind;
	uint32_t title_region;
	uint32_t body_region;
	uint32_t flags;
	uint32_t reserved;
	/* Native tbl column and offset hint, not a validated final DisplayPoint.
	 * Present only for a table-cell region after TABLE_CELL_POSITION. */
	uint32_t table_column;
	uint32_t table_position_present;
	uint64_t table_offset;
	/* Owned UTF-8 only for ANCHOR.  Empty for other mark kinds. */
	const uint8_t *name;
	uint64_t name_length;
};

struct mant_annotated_collector_metrics {
	uint64_t glyph_origins;
	uint64_t field_placements;
	uint64_t direct_draws;
	uint64_t unverified_placements;
	uint64_t mark_count;
	uint64_t peak_live_slots;
	uint64_t peak_slot_bytes;
};

struct mant_annotated_collector *mant_annotated_collector_new(
	struct structured_session *, struct mant_annotated_display *);
void mant_annotated_collector_observe(struct termp *, void *,
	const struct term_collector_event *);
int mant_annotated_collector_sink(void *, const void *, size_t);
int mant_annotated_collector_account_display(
	struct mant_annotated_collector *);
void mant_annotated_collector_get_metrics(
	const struct mant_annotated_collector *,
	struct mant_annotated_collector_metrics *);
void mant_annotated_collector_get_marks(
	const struct mant_annotated_collector *,
	const struct mant_annotated_mark **, uint32_t *);
void mant_annotated_collector_take_marks(
	struct mant_annotated_collector *,
	struct mant_annotated_mark **, uint32_t *);
void mant_annotated_marks_free(struct mant_annotated_mark *, uint32_t);
void mant_annotated_collector_free(struct mant_annotated_collector *);

#endif
