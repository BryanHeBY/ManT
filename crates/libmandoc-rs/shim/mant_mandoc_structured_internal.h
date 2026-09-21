/* Private ownership boundary shared by structured-rendering C modules. */
#ifndef MANT_MANDOC_STRUCTURED_INTERNAL_H
#define MANT_MANDOC_STRUCTURED_INTERNAL_H

#include "mant_mandoc_structured.h"

#define MANT_STRUCTURED_MAGIC 0x4d535231U

struct structured_source_line {
	uint64_t length;
	uint8_t present;
};

struct structured_source_map {
	struct structured_source_line *lines;
	uint32_t line_count;
	uint32_t line_capacity;
};

/* The result owns every allocation reachable through the public view. */
struct mant_structured_result {
	uint32_t magic;
	uint32_t checked;
	uint32_t root_source;
	uint32_t profile;
	uint32_t width;
	struct mant_structured_metadata_view metadata;
	struct mant_structured_source_view *sources;
	uint32_t source_count;
	uint32_t source_capacity;
	struct mant_structured_span_view *spans;
	uint32_t span_count;
	uint32_t span_capacity;
	struct mant_structured_provenance_view *provenances;
	uint32_t provenance_count;
	uint32_t provenance_capacity;
	struct mant_structured_owner_view *owners;
	uint32_t owner_count;
	uint32_t owner_capacity;
	struct mant_structured_content_root_view *content_roots;
	uint32_t content_root_count;
	uint32_t content_root_capacity;
	struct mant_structured_content_atom_view *content_atoms;
	uint32_t content_atom_count;
	uint32_t content_atom_capacity;
	struct mant_structured_content_ref_view *content_refs;
	uint32_t content_ref_count;
	uint32_t content_ref_capacity;
	struct mant_structured_content_point_view *content_points;
	uint32_t content_point_count;
	uint32_t content_point_capacity;
	struct mant_structured_link_view *links;
	uint32_t link_count;
	uint32_t link_capacity;
	struct mant_structured_link_label_part_view *link_label_parts;
	uint32_t link_label_part_count;
	uint32_t link_label_part_capacity;
	struct mant_structured_anchor_view *anchors;
	uint32_t anchor_count;
	uint32_t anchor_capacity;
	struct mant_structured_heading_evidence_view *heading_evidence;
	uint32_t heading_evidence_count;
	uint32_t heading_evidence_capacity;
	struct mant_structured_block_view *blocks;
	uint32_t block_count;
	uint32_t block_capacity;
	struct mant_structured_list_view *lists;
	uint32_t list_count;
	uint32_t list_capacity;
	struct mant_structured_item_view *items;
	uint32_t item_count;
	uint32_t item_capacity;
	struct mant_structured_form_view *forms;
	uint32_t form_count;
	uint32_t form_capacity;
	struct mant_structured_name_hint_view *name_hints;
	uint32_t name_hint_count;
	uint32_t name_hint_capacity;
	struct mant_structured_diagnostic_view *diagnostics;
	uint32_t diagnostic_count;
	uint32_t diagnostic_capacity;
	struct structured_source_map *source_maps;
	uint32_t source_map_count;
	/* Retained, budgeted scratch keeps repeated handle validation linear. */
	uint32_t *validation_roots;
	uint32_t validation_root_slots;
	uint32_t *validation_blocks;
	uint32_t validation_block_slots;
	uint32_t *validation_lists;
	uint32_t validation_list_slots;
	uint8_t *validation_atoms;
	uint32_t validation_atom_slots;
	uint32_t *validation_root_atom_offsets;
	uint32_t *validation_root_atoms;
	uint64_t *validation_atom_scalar_offsets;
	uint64_t *validation_root_scalar_totals;
	uint32_t validation_owner_count;
	uint32_t validation_content_root_count;
	uint32_t validation_block_count;
	uint32_t validation_list_count;
	uint32_t validation_item_count;
	uint32_t validation_content_atom_count;
	uint8_t validation_ready;
};

void mant_structured_clear_failure(struct mant_structured_failure_view *);
int mant_structured_valid_bytes(struct mant_bytes_view);
int mant_structured_valid_utf8(const uint8_t *, uint64_t);
int mant_structured_valid_identity_name(uint32_t, struct mant_bytes_view);
int mant_structured_source_position_in_maps(
    const struct structured_source_map *, uint32_t, uint32_t, uint32_t,
    uint32_t);
struct structured_session;
int mant_structured_result_is_valid(const struct mant_structured_result *,
    struct structured_session *);
void mant_structured_free_bytes(struct mant_bytes_view);

#endif
