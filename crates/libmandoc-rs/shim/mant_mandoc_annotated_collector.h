/* Private, per-render terminal observation and post-device transfer. */
#ifndef MANT_MANDOC_ANNOTATED_COLLECTOR_H
#define MANT_MANDOC_ANNOTATED_COLLECTOR_H

#include "mant_mandoc_annotated_display.h"
#include "mant_mandoc_structured.h"

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
	MANT_ANNOTATED_REGION_TABLE_CELL = 9,
	MANT_ANNOTATED_REGION_UNSECTIONED = 10,
	/* Generated .mc glyphs belong to a physical row, never a semantic HEAD. */
	MANT_ANNOTATED_REGION_MARGIN = 11
};

/* No physical row/column adjacency is treated as logical text evidence. */
enum mant_annotated_text_join {
	MANT_ANNOTATED_JOIN_NONE = 0,
	MANT_ANNOTATED_JOIN_DIRECT_CONTACT = 1,
	MANT_ANNOTATED_JOIN_AUTHORED_SEPARATOR = 2,
	MANT_ANNOTATED_JOIN_HARD_BOUNDARY = 3,
	MANT_ANNOTATED_JOIN_UNKNOWN = 4
};

struct mant_annotated_selection_part {
	uint32_t run;
	uint32_t join_before;
	uint64_t start_byte;
	uint64_t end_byte;
	uint64_t join_text_start;
	uint64_t join_text_len;
};

#define MANT_ANNOTATED_MARK_AUTHORED (1U << 0)
#define MANT_ANNOTATED_MARK_FINAL_POINT_UNVERIFIED (1U << 1)
#define MANT_ANNOTATED_MARK_MANUAL_TARGET (1U << 2)
#define MANT_ANNOTATED_MARK_SUBSECTION (1U << 3)
#define MANT_ANNOTATED_MARK_DEFINITION (1U << 4)
#define MANT_ANNOTATED_MARK_HEAD_OPTION (1U << 5)
#define MANT_ANNOTATED_MARK_HEAD_ENVIRONMENT (1U << 6)
#define MANT_ANNOTATED_MARK_HEAD_LITERAL (1U << 7)
#define MANT_ANNOTATED_MARK_HEAD_LEXICAL (1U << 8)
#define MANT_ANNOTATED_MARK_HEAD_ROLE_MASK (\
    MANT_ANNOTATED_MARK_HEAD_OPTION | \
    MANT_ANNOTATED_MARK_HEAD_ENVIRONMENT | \
    MANT_ANNOTATED_MARK_HEAD_LITERAL | \
    MANT_ANNOTATED_MARK_HEAD_LEXICAL)

enum mant_annotated_point_kind {
	MANT_ANNOTATED_POINT_NONE = 0,
	MANT_ANNOTATED_POINT_ROW_COLUMN = 1,
	MANT_ANNOTATED_POINT_DOCUMENT_END = 2
};

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
	/* Owned UTF-8 target spelling for ANCHOR, optional deroff() authored
	 * phrase for HEADING, or conservative first native Fl/Ev operand for
	 * OWNER.  The latter is only a bound on a final display name. */
	const uint8_t *name;
	uint64_t name_length;
	/* Owned decoded destination for a link; authoring source remains above. */
	uint32_t target_kind;
	uint32_t target_b_present;
	struct mant_bytes_view target_a;
	struct mant_bytes_view target_b;
	/* Zero-based range in the result's shared selection_parts arena.
	 * Only direct owner and link labels are indexed; ancestors are not copied. */
	uint32_t selection_first;
	uint32_t selection_count;
	/* Final body-only zero-width point.  ROW_COLUMN uses a one-based row;
	 * DOCUMENT_END uses the final physical row count and column zero. */
	uint32_t point_kind;
	uint32_t point_row;
	uint32_t point_column;
	uint32_t point_reserved;
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
int mant_annotated_collector_finish_points(struct mant_annotated_collector *,
	const struct mant_annotated_display_view *);
void mant_annotated_marks_free(struct mant_annotated_mark *, uint32_t);
void mant_annotated_collector_free(struct mant_annotated_collector *);

#endif
