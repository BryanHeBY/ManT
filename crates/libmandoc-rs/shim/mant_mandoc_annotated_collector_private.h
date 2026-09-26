/* Private collector state: marks observe AST identity; buffer owns active
 * columns, slots, pending glyphs and point chains.  Session owns the call. */
#ifndef MANT_MANDOC_ANNOTATED_COLLECTOR_PRIVATE_H
#define MANT_MANDOC_ANNOTATED_COLLECTOR_PRIVATE_H

#include "config.h"

#include <ctype.h>
#include <limits.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "mandoc.h"
#include "roff.h"
#include "mdoc.h"
#include "libmdoc.h"
#include "tbl.h"
#include "out.h"
#include "tag.h"
#include "term.h"

#include "mant_mandoc_annotated_collector.h"
#include "mant_mandoc_output.h"
#include "mant_mandoc_structured_link.h"
#include "mant_mandoc_structured_session.h"

struct annotated_slot {
	uint64_t origin;
	uint32_t owner;
	uint32_t link;
	uint32_t source;
	uint32_t head_component;
	int value;
	uint8_t flags;
	uint8_t font;
	uint8_t occupied;
	uint8_t authored_space;
	uint8_t generated_space;
	uint8_t layout_space;
	uint32_t first_point;
	uint64_t skipped_visual;
};

struct annotated_column {
	struct annotated_slot *slots;
	uint32_t capacity;
	uint32_t live;
	uint64_t last_output_origin;
	uint64_t pending_spaces;
	uint32_t separator_owner;
	uint32_t separator_link;
	struct mant_annotated_display_edge last_origin_edge;
	uint32_t pending_join;
	uint32_t skip_start;
	uint64_t skipped_visual;
	uint32_t skip_cell_width;
	uint8_t skipping;
};

struct annotated_frame {
	const struct roff_node *node;
	const struct roff_node *saved_link_node;
	const struct roff_node *last_direct_man_node;
	const struct roff_node *alternate_child;
	const struct roff_node *alternate_next_child;
	uint32_t saved_owner;
	uint32_t saved_head_component;
	uint32_t saved_link;
	uint32_t saved_heading;
	uint64_t saved_link_epoch;
	uint32_t owner_mark;
	uint32_t anchor_mark;
	uint32_t region_mark;
	uint32_t last_direct_man_owner;
	uint32_t alternate_child_index;
};

struct annotated_point_state {
	struct mant_annotated_display_checkpoint checkpoint;
	uint32_t next;
	uint8_t state; /* 0 absent, 1 active buffer gap, 2 captured. */
	uint8_t component_started; /* Collector-only logical macro state. */
};

struct annotated_cell {
	const struct tbl_dat *data;
	uint32_t mark;
};

struct mant_annotated_collector {
	struct structured_session *session;
	struct mant_annotated_display *display;
	struct annotated_column *columns;
	uint32_t column_capacity;
	struct annotated_frame *frames;
	uint32_t frame_count;
	uint32_t frame_capacity;
	struct mant_annotated_mark *marks;
	uint32_t mark_count;
	uint32_t mark_capacity;
	struct annotated_point_state *points;
	uint32_t point_capacity;
	uint32_t pending_point_count;
	struct annotated_cell *cells;
	uint32_t cell_capacity;
	uint32_t cell_count;
	const struct roff_node *table_node;
	const struct tbl_dat *active_cell;
	uint32_t cell_saved_owner;
	uint8_t table_prepared;
	uint32_t active_owner;
	uint32_t active_head_component;
	uint32_t active_link;
	const struct roff_node *active_link_node;
	uint64_t active_link_epoch;
	uint64_t phrase_epoch;
	uint32_t active_heading;
	uint32_t last_top_heading;
	uint32_t unsectioned_region;
	/* At most one direct PP/P/LP or first-paragraph -> RS handoff. */
	const struct roff_node *pending_hanging_rs;
	const struct roff_node *pending_hanging_scope;
	uint32_t pending_hanging_owner;
	uint32_t pending_hanging_head_region;
	size_t pending_hanging_epoch;
	enum roff_tok pending_hanging_par_tok;
	uint8_t pending_hanging_mode; /* 1 elided PP/P/LP, 2 implicit first paragraph. */
	/* ROOT has no terminal frame: retain its last completed direct sibling. */
	const struct roff_node *last_root_man_node;
	uint32_t last_root_man_owner;
	uint64_t next_origin;
	uint64_t pending_origin;
	uint32_t pending_owner;
	uint32_t pending_head_component;
	uint32_t pending_link;
	uint32_t pending_source;
	uint32_t margin_owner;
	uint32_t margin_mark;
	uint64_t allocated_display_bytes;
	uint64_t accounted_display_work;
	uint64_t live_slots;
	uint64_t slot_bytes;
	struct mant_annotated_collector_metrics metrics;
	struct mant_annotated_display_label letter_label;
	struct mant_annotated_display_edge letter_edge;
	struct mant_annotated_display_label skipped[256];
	uint16_t skipped_cells;
	uint16_t advance_count;
	uint32_t advance_role;
	uint32_t skipped_column;
	uint32_t letter_column;
	uint32_t letter_pos;
	uint8_t letter_pending;
	uint8_t letter_from_field;
	uint8_t in_header;
	uint8_t in_footer;
	uint8_t in_margin;
	uint8_t footer_drained;
	uint8_t html_nofill;
	uint8_t link_annotation_rejected;
};

/* Shared cumulative budgets and relation failure report through the one
 * session.  No module owns a parallel status or TLS state. */
void mant_annotated_fail_relation(struct mant_annotated_collector *,
    uint64_t, uint64_t);
int mant_annotated_charge_work(struct mant_annotated_collector *, uint64_t);
int mant_annotated_charge_mutations(struct mant_annotated_collector *,
    uint64_t);

/* AST/mark observation supplies identities, never formatter geometry. */
void mant_annotated_marks_observe(struct mant_annotated_collector *,
    const struct term_collector_event *);
int mant_annotated_marks_select_component(struct mant_annotated_collector *,
    const struct roff_node *);
uint32_t mant_annotated_marks_visible_link(
    const struct mant_annotated_collector *, const struct roff_node *,
    enum term_collector_reason);
uint32_t mant_annotated_marks_source_key(const struct roff_node *);
uint32_t mant_annotated_marks_margin(struct mant_annotated_collector *);

/* Parsed-head and reading-neighbor facts are candidates, not entry names. */
int mant_annotated_decl_reading_sibling_gap(const struct roff_node *);
int mant_annotated_decl_reading_family(int);
int mant_annotated_decl_paragraph_token(enum roff_tok);
const struct roff_node *mant_annotated_decl_hanging_successor(
    struct mant_annotated_collector *, const struct roff_node *, int *);
int mant_annotated_decl_direct_predecessor(
    struct mant_annotated_collector *, const struct roff_node *,
    const struct roff_node **);
uint32_t mant_annotated_decl_owner_head_role(const struct roff_node *,
    const struct roff_node **);
int mant_annotated_decl_copy_tp_lexical_head(
    struct mant_annotated_collector *, const struct roff_node *, int *, int *);
int mant_annotated_decl_ip_bold_candidate(
    struct mant_annotated_collector *, const struct roff_node *, int *);
int mant_annotated_decl_ip_head_candidate(
    struct mant_annotated_collector *, const struct roff_node *, int *);

/* The buffer module alone owns point states and active slot lifetimes.
 * Mark insertion reserves its matching point before committing a key. */
int mant_annotated_buffer_reserve_point(struct mant_annotated_collector *,
    uint32_t, uint32_t);
int mant_annotated_buffer_point_now(struct mant_annotated_collector *,
    uint32_t, uint16_t);
int mant_annotated_buffer_point_is_unused(
    const struct mant_annotated_collector *, uint32_t);
void mant_annotated_buffer_release(struct mant_annotated_collector *);

#endif
