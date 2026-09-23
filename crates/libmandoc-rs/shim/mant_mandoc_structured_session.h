/* Private session state shared by structured-rendering implementation units. */
#ifndef MANT_MANDOC_STRUCTURED_SESSION_H
#define MANT_MANDOC_STRUCTURED_SESSION_H

#include "mant_mandoc_structured_internal.h"

struct roff_node;
struct structured_token;
struct structured_column;
struct structured_anchor_state;
struct structured_link_identity {
	const struct roff_node *node;
	uint32_t key;
};

enum structured_content_part {
	STRUCTURED_PART_FLOW,
	STRUCTURED_PART_TERM,
	STRUCTURED_PART_BODY
};

struct structured_node_context {
	uint32_t owner;
	uint32_t list;
	uint32_t item;
	uint32_t container_block;
	uint32_t term_root;
	const struct roff_node *saved_man_marker_node;
	uint32_t saved_man_item;
	uint32_t saved_man_list;
	uint32_t saved_man_parent_block;
	uint32_t saved_man_marker_list;
	uint32_t saved_man_marker_kind;
	uint32_t saved_man_marker_style;
	uint32_t saved_man_marker_ordinal;
	uint8_t part;
	uint8_t restore_man_state;
};

struct structured_anchor_queue {
	uint32_t head;
	uint32_t tail;
	uint32_t last_root;
};

struct structured_root_atoms {
	/*
	 * Formatter head execution emits each term root as one atom interval.
	 * Finalization validates that interval after the formatter has drained;
	 * body roots keep the same bookkeeping but do not otherwise depend on
	 * contiguity.  A closed term can legitimately contain no visible atoms.
	 */
	uint32_t first;
	uint32_t count;
	uint32_t item;
	uint32_t point_count;
	uint64_t scalar_count;
	struct structured_anchor_queue pending_anchors;
	uint8_t closed;
	uint8_t finalized;
};

struct structured_list_state {
	/* Active item ordinals are local to one list scope. */
	uint32_t item_count;
	struct structured_anchor_queue pending_anchors;
};

struct structured_session {
	const struct mant_structured_input_view *input;
	const struct mant_input_source_view *inputs;
	const struct mant_structured_limits *limits;
	struct mant_structured_probe_metrics *probe;
	struct mant_structured_result *result;
	uint32_t *source_keys;
	struct structured_source_map *source_maps;
	uint32_t current_input;
	uint32_t status;
	uint32_t stage;
	uint32_t limit_kind;
	uint64_t observed;
	uint64_t allowed;
	uint64_t source_path_bytes;
	uint64_t decoded_bytes;
	uint64_t source_map_entries;
	uint64_t source_map_bytes;
	uint64_t builder_operations;
	uint64_t allocated_bytes;
	uint64_t content_bytes;
	uint64_t include_depth;
	uint64_t connection_atoms;
	uint64_t annotation_runs;
	uint64_t annotation_mutations;
	uint64_t relation_edges;
	const struct roff_node **node_stack;
	uint32_t node_depth;
	uint32_t node_capacity;
	struct structured_node_context *node_contexts;
	uint32_t node_context_capacity;
	uint32_t *owner_root_counts;
	uint32_t owner_root_capacity;
	struct structured_root_atoms *root_atoms;
	uint32_t root_atom_capacity;
	uint32_t *block_child_counts;
	uint32_t block_child_capacity;
	struct structured_list_state *list_states;
	uint32_t list_state_capacity;
	uint32_t last_man_item;
	uint32_t last_man_list;
	uint32_t last_man_parent_block;
	uint8_t man_continuation_pending;
	const struct roff_node *last_man_marker_node;
	uint32_t last_man_marker_list;
	uint32_t last_man_marker_kind;
	uint32_t last_man_marker_style;
	uint32_t next_man_marker_ordinal;
	uint32_t output_depth;
	uint32_t current_root;
	uint32_t current_owner;
	uint32_t section_owner;
	uint32_t section_heading_block;
	uint32_t top_level_block_count;
	uint32_t pending_break_provenance;
	uint32_t pending_break_root;
	uint32_t pending_break_link;
	uint64_t pending_break_sequence;
	struct structured_anchor_state *anchor_states;
	uint32_t anchor_state_capacity;
	struct structured_anchor_queue unowned_anchors;
	struct structured_anchor_queue *owner_anchor_queues;
	uint32_t owner_anchor_queue_capacity;
	uint8_t force_atom_split;
	struct structured_token *tokens;
	uint32_t token_slot_count;
	uint32_t token_capacity;
	uint32_t free_token;
	uint32_t pending_token;
	uint64_t token_total;
	uint64_t projection_live_bytes;
	uint64_t projection_peak_bytes;
	struct structured_column *columns;
	uint32_t column_count;
	uint32_t column_capacity;
	const struct roff_node *last_span_node;
	uint32_t last_span;
	const struct roff_node *last_provenance_node;
	uint32_t last_provenance;
	uint8_t last_provenance_authored;
	uint64_t current_atom_capacity;
	uint64_t current_display_capacity;
	struct structured_link_identity *link_identities;
	uint32_t link_identity_capacity;
	uint32_t link_identity_count;
};

int mant_structured_injected_allocation_failure(void);
void mant_structured_set_failure(struct structured_session *, uint32_t,
    uint32_t, uint32_t, uint64_t, uint64_t);
int mant_structured_charge(struct structured_session *, uint64_t *, uint64_t,
    uint64_t, uint32_t, uint32_t);
void *mant_structured_allocate(struct structured_session *, uint64_t, int,
    uint32_t);
void *mant_structured_grow_array(struct structured_session *, void *,
    uint32_t, uint32_t *, uint32_t, size_t, uint64_t, uint32_t, uint32_t);
uint8_t *mant_structured_copy_bytes(struct structured_session *,
    const uint8_t *, uint64_t, int, uint32_t);
struct mant_bytes_view mant_structured_copy_cstring(
    struct structured_session *, const char *);

#endif
