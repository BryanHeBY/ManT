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
	struct mant_structured_link_view *links;
	uint32_t link_count;
	uint32_t link_capacity;
	struct mant_structured_block_view *blocks;
	uint32_t block_count;
	uint32_t block_capacity;
	struct mant_structured_diagnostic_view *diagnostics;
	uint32_t diagnostic_count;
	uint32_t diagnostic_capacity;
	struct structured_source_map *source_maps;
	uint32_t source_map_count;
};

void mant_structured_clear_failure(struct mant_structured_failure_view *);
int mant_structured_valid_bytes(struct mant_bytes_view);
int mant_structured_valid_utf8(const uint8_t *, uint64_t);
int mant_structured_valid_identity_name(uint32_t, struct mant_bytes_view);
int mant_structured_source_position_in_maps(
    const struct structured_source_map *, uint32_t, uint32_t, uint32_t,
    uint32_t);
int mant_structured_result_is_valid(const struct mant_structured_result *);
void mant_structured_free_bytes(struct mant_bytes_view);

#endif
