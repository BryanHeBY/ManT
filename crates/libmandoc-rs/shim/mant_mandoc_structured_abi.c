/* ABI identity and layout probes for the structured-rendering boundary. */
#include "mant_mandoc_structured.h"

#include <stddef.h>
#include <stdint.h>

uint32_t
mant_structured_abi_version(void)
{
	return 3;
}

uint64_t
mant_structured_discriminant_fingerprint(void)
{
	static const uint32_t values[] = {
		/* status */ MANT_STRUCTURED_OK, MANT_STRUCTURED_INVALID_INPUT,
		MANT_STRUCTURED_REENTRANT, MANT_STRUCTURED_BUDGET,
		MANT_STRUCTURED_BUILDER_ALLOC, MANT_STRUCTURED_NATIVE,
		MANT_STRUCTURED_RELATION, MANT_STRUCTURED_UNSUPPORTED,
		/* stage */ 0, MANT_STRUCTURED_STAGE_MARSHAL,
		MANT_STRUCTURED_STAGE_RESOLVE, MANT_STRUCTURED_STAGE_PARSE,
		MANT_STRUCTURED_STAGE_RENDER, MANT_STRUCTURED_STAGE_FINALIZE,
		MANT_STRUCTURED_STAGE_CHECK,
		/* view */ 0, MANT_VIEW_INPUT_SOURCE, MANT_VIEW_INPUT,
		MANT_VIEW_FAILURE, MANT_VIEW_LIMITS, MANT_VIEW_RESULT,
		MANT_VIEW_SOURCE, MANT_VIEW_SPAN, MANT_VIEW_PROVENANCE,
		MANT_VIEW_OWNER, MANT_VIEW_CONTENT_ROOT, MANT_VIEW_CONTENT_ATOM,
		MANT_VIEW_CONTENT_REF, MANT_VIEW_CONTENT_POINT, MANT_VIEW_LINK,
		MANT_VIEW_BLOCK, MANT_VIEW_TABLE, MANT_VIEW_TABLE_ROW,
		MANT_VIEW_TABLE_CELL, MANT_VIEW_FIXED_VIEW, MANT_VIEW_FIXED_LINE,
		MANT_VIEW_PLACEMENT, MANT_VIEW_DECORATION, MANT_VIEW_FORM,
		MANT_VIEW_NAME_HINT, MANT_VIEW_RELATION, MANT_VIEW_DIAGNOSTIC,
		MANT_VIEW_METADATA, MANT_VIEW_LIST, MANT_VIEW_ITEM,
		/* identity */ 0, MANT_IDENTITY_PATH, MANT_IDENTITY_BUNDLE_MEMBER,
		MANT_IDENTITY_ANONYMOUS,
		/* format */ 0, MANT_FORMAT_MAN, MANT_FORMAT_MDOC,
		MANT_FORMAT_MARKDOWN,
		/* profile */ 0, MANT_PROFILE_UTF8, MANT_PROFILE_ASCII,
		/* coordinate */ 0, MANT_COORD_DECODED_UTF8_BYTES,
		MANT_COORD_NATIVE_NORMALIZED_BYTES,
		/* diagnostic level and native range */ 0, MANT_DIAGNOSTIC_STYLE,
		MANT_DIAGNOSTIC_WARNING, MANT_DIAGNOSTIC_ERROR,
		MANT_DIAGNOSTIC_UNSUPPORTED, MANT_DIAGNOSTIC_CODE_NATIVE_FIRST,
		MANT_DIAGNOSTIC_CODE_NATIVE_LAST,
		/* provenance */ 0, MANT_PROVENANCE_AUTHORED,
		MANT_PROVENANCE_GENERATED, MANT_PROVENANCE_UNKNOWN,
		/* atom */ 0, MANT_ATOM_TEXT, MANT_ATOM_WHITESPACE,
		MANT_ATOM_BREAK_OPPORTUNITY, MANT_ATOM_HARD_BREAK,
		/* style bit values */ 0, MANT_STYLE_BOLD, MANT_STYLE_ITALIC,
		MANT_STYLE_LITERAL, MANT_STYLE_UNDERLINE,
		/* role */ 0, MANT_ROLE_FLAG, MANT_ROLE_ENVIRONMENT_VARIABLE,
		MANT_ROLE_ARGUMENT, MANT_ROLE_COMMAND_OR_DIRECTIVE, MANT_ROLE_PATH,
		/* target origin */ MANT_TARGET_ORIGIN_ABSENT,
		MANT_TARGET_ORIGIN_GENERATED, MANT_TARGET_ORIGIN_AUTHORED,
		/* owner */ 0, MANT_OWNER_DOCUMENT, MANT_OWNER_SECTION,
		MANT_OWNER_PARAGRAPH, MANT_OWNER_LIST_ITEM,
		MANT_OWNER_DEFINITION_ITEM, MANT_OWNER_TABLE_CELL,
		MANT_OWNER_FIXED_DISPLAY,
		/* root */ 0, MANT_ROOT_HEADING, MANT_ROOT_TERM, MANT_ROOT_BODY,
		MANT_ROOT_CELL, MANT_ROOT_FIXED_BODY,
		/* block */ 0, MANT_BLOCK_HEADING, MANT_BLOCK_PARAGRAPH,
		MANT_BLOCK_LIST, MANT_BLOCK_DEFINITION_LIST, MANT_BLOCK_TABLE,
		MANT_BLOCK_INDENTED, MANT_BLOCK_FIXED_DISPLAY,
		MANT_BLOCK_VERTICAL_SPACE, MANT_BLOCK_THEMATIC_BREAK,
		/* list */ 0, MANT_LIST_BULLET, MANT_LIST_ORDERED,
		MANT_LIST_PLAIN, MANT_LIST_DEFINITION, MANT_LIST_NATIVE_MARKER,
		/* link target */ 0, MANT_LINK_EXTERNAL, MANT_LINK_EMAIL,
		MANT_LINK_DOCUMENT, MANT_LINK_MANUAL, MANT_LINK_SECTION,
		/* point boundary */ 0, MANT_POINT_BETWEEN_ATOMS,
		MANT_POINT_IN_ATOM,
		/* placement target */ 0, MANT_PLACEMENT_CONTENT,
		MANT_PLACEMENT_POINT,
		/* cell map */ 0, MANT_CELL_MAP_AFFINE,
		MANT_CELL_MAP_GRAPHEME_CLUSTER, MANT_CELL_MAP_OVERLAY,
		/* table cell */ 0, MANT_TABLE_CELL_TEXT,
		MANT_TABLE_CELL_HORIZONTAL_RULE,
		MANT_TABLE_CELL_DOUBLE_HORIZONTAL_RULE,
		MANT_TABLE_CELL_ISOLATED_HORIZONTAL_RULE,
		MANT_TABLE_CELL_ISOLATED_DOUBLE_HORIZONTAL_RULE,
		/* table alignment */ 0, MANT_TABLE_ALIGN_LEFT,
		MANT_TABLE_ALIGN_CENTER, MANT_TABLE_ALIGN_RIGHT,
		/* decoration */ 0, MANT_DECORATION_BORDER, MANT_DECORATION_RULE,
		MANT_DECORATION_PADDING,
		/* relation */ 0, MANT_RELATION_ALIAS,
		MANT_RELATION_READING_CONTEXT,
		/* resolver */ MANT_RESOLVE_FOUND, MANT_RESOLVE_NOT_FOUND,
		MANT_RESOLVE_DENIED, MANT_RESOLVE_IO, MANT_RESOLVE_PANIC,
		MANT_RESOLVE_INVALID,
		/* metadata presence bits */ 0, MANT_METADATA_TITLE_PRESENT,
		MANT_METADATA_SECTION_PRESENT, MANT_METADATA_VOLUME_PRESENT,
		MANT_METADATA_OS_PRESENT, MANT_METADATA_ARCH_PRESENT,
		MANT_METADATA_NAME_PRESENT, MANT_METADATA_DATE_PRESENT,
		MANT_METADATA_ALIAS_PRESENT
	};
	uint64_t hash = UINT64_C(14695981039346656037);
	size_t i;
	unsigned int shift;

	for (i = 0; i < sizeof(values) / sizeof(values[0]); i++)
		for (shift = 0; shift < 32; shift += 8) {
			hash ^= (values[i] >> shift) & 0xffU;
			hash *= UINT64_C(1099511628211);
		}
	return hash;
}
#if defined(_MSC_VER)
#define MANT_ALIGNOF(type) __alignof(type)
#else
#define MANT_ALIGNOF(type) _Alignof(type)
#endif

#define VIEW_CASE(id, type) case id: return sizeof(type)
size_t
mant_structured_view_size(uint32_t kind)
{
	switch (kind) {
	VIEW_CASE(MANT_VIEW_INPUT_SOURCE, struct mant_input_source_view);
	VIEW_CASE(MANT_VIEW_INPUT, struct mant_structured_input_view);
	VIEW_CASE(MANT_VIEW_FAILURE, struct mant_structured_failure_view);
	VIEW_CASE(MANT_VIEW_LIMITS, struct mant_structured_limits);
	VIEW_CASE(MANT_VIEW_RESULT, struct mant_structured_result_view);
	VIEW_CASE(MANT_VIEW_SOURCE, struct mant_structured_source_view);
	VIEW_CASE(MANT_VIEW_SPAN, struct mant_structured_span_view);
	VIEW_CASE(MANT_VIEW_PROVENANCE, struct mant_structured_provenance_view);
	VIEW_CASE(MANT_VIEW_OWNER, struct mant_structured_owner_view);
	VIEW_CASE(MANT_VIEW_CONTENT_ROOT, struct mant_structured_content_root_view);
	VIEW_CASE(MANT_VIEW_CONTENT_ATOM, struct mant_structured_content_atom_view);
	VIEW_CASE(MANT_VIEW_CONTENT_REF, struct mant_structured_content_ref_view);
	VIEW_CASE(MANT_VIEW_CONTENT_POINT, struct mant_structured_content_point_view);
	VIEW_CASE(MANT_VIEW_LINK, struct mant_structured_link_view);
	VIEW_CASE(MANT_VIEW_BLOCK, struct mant_structured_block_view);
	VIEW_CASE(MANT_VIEW_TABLE, struct mant_structured_table_view);
	VIEW_CASE(MANT_VIEW_TABLE_ROW, struct mant_structured_table_row_view);
	VIEW_CASE(MANT_VIEW_TABLE_CELL, struct mant_structured_table_cell_view);
	VIEW_CASE(MANT_VIEW_FIXED_VIEW, struct mant_structured_fixed_view);
	VIEW_CASE(MANT_VIEW_FIXED_LINE, struct mant_structured_fixed_line_view);
	VIEW_CASE(MANT_VIEW_PLACEMENT, struct mant_structured_placement_view);
	VIEW_CASE(MANT_VIEW_DECORATION, struct mant_structured_decoration_view);
	VIEW_CASE(MANT_VIEW_FORM, struct mant_structured_form_view);
	VIEW_CASE(MANT_VIEW_NAME_HINT, struct mant_structured_name_hint_view);
	VIEW_CASE(MANT_VIEW_RELATION, struct mant_structured_relation_view);
	VIEW_CASE(MANT_VIEW_DIAGNOSTIC, struct mant_structured_diagnostic_view);
	VIEW_CASE(MANT_VIEW_METADATA, struct mant_structured_metadata_view);
	VIEW_CASE(MANT_VIEW_LIST, struct mant_structured_list_view);
	VIEW_CASE(MANT_VIEW_ITEM, struct mant_structured_item_view);
	default: return 0;
	}
}

#undef VIEW_CASE
#define VIEW_CASE(id, type) case id: return MANT_ALIGNOF(type)
size_t
mant_structured_view_align(uint32_t kind)
{
	switch (kind) {
	VIEW_CASE(MANT_VIEW_INPUT_SOURCE, struct mant_input_source_view);
	VIEW_CASE(MANT_VIEW_INPUT, struct mant_structured_input_view);
	VIEW_CASE(MANT_VIEW_FAILURE, struct mant_structured_failure_view);
	VIEW_CASE(MANT_VIEW_LIMITS, struct mant_structured_limits);
	VIEW_CASE(MANT_VIEW_RESULT, struct mant_structured_result_view);
	VIEW_CASE(MANT_VIEW_SOURCE, struct mant_structured_source_view);
	VIEW_CASE(MANT_VIEW_SPAN, struct mant_structured_span_view);
	VIEW_CASE(MANT_VIEW_PROVENANCE, struct mant_structured_provenance_view);
	VIEW_CASE(MANT_VIEW_OWNER, struct mant_structured_owner_view);
	VIEW_CASE(MANT_VIEW_CONTENT_ROOT, struct mant_structured_content_root_view);
	VIEW_CASE(MANT_VIEW_CONTENT_ATOM, struct mant_structured_content_atom_view);
	VIEW_CASE(MANT_VIEW_CONTENT_REF, struct mant_structured_content_ref_view);
	VIEW_CASE(MANT_VIEW_CONTENT_POINT, struct mant_structured_content_point_view);
	VIEW_CASE(MANT_VIEW_LINK, struct mant_structured_link_view);
	VIEW_CASE(MANT_VIEW_BLOCK, struct mant_structured_block_view);
	VIEW_CASE(MANT_VIEW_TABLE, struct mant_structured_table_view);
	VIEW_CASE(MANT_VIEW_TABLE_ROW, struct mant_structured_table_row_view);
	VIEW_CASE(MANT_VIEW_TABLE_CELL, struct mant_structured_table_cell_view);
	VIEW_CASE(MANT_VIEW_FIXED_VIEW, struct mant_structured_fixed_view);
	VIEW_CASE(MANT_VIEW_FIXED_LINE, struct mant_structured_fixed_line_view);
	VIEW_CASE(MANT_VIEW_PLACEMENT, struct mant_structured_placement_view);
	VIEW_CASE(MANT_VIEW_DECORATION, struct mant_structured_decoration_view);
	VIEW_CASE(MANT_VIEW_FORM, struct mant_structured_form_view);
	VIEW_CASE(MANT_VIEW_NAME_HINT, struct mant_structured_name_hint_view);
	VIEW_CASE(MANT_VIEW_RELATION, struct mant_structured_relation_view);
	VIEW_CASE(MANT_VIEW_DIAGNOSTIC, struct mant_structured_diagnostic_view);
	VIEW_CASE(MANT_VIEW_METADATA, struct mant_structured_metadata_view);
	VIEW_CASE(MANT_VIEW_LIST, struct mant_structured_list_view);
	VIEW_CASE(MANT_VIEW_ITEM, struct mant_structured_item_view);
	default: return 0;
	}
}

#define FIELD(type, member) offsetof(type, member)
#define PICK(array) ((field > 0 && field <= sizeof(array) / sizeof(array[0])) ? \
	array[field - 1] : SIZE_MAX)

size_t
mant_structured_view_offset(uint32_t kind, uint32_t field)
{
	static const size_t input_source[] = {
		FIELD(struct mant_input_source_view, identity_kind),
		FIELD(struct mant_input_source_view, format),
		FIELD(struct mant_input_source_view, logical_name),
		FIELD(struct mant_input_source_view, resolver_name),
		FIELD(struct mant_input_source_view, source_bytes),
		FIELD(struct mant_input_source_view, reserved) };
	static const size_t input[] = {
		FIELD(struct mant_structured_input_view, sources),
		FIELD(struct mant_structured_input_view, root_input),
		FIELD(struct mant_structured_input_view, profile),
		FIELD(struct mant_structured_input_view, width),
		FIELD(struct mant_structured_input_view, resolve),
		FIELD(struct mant_structured_input_view, resolve_context),
		FIELD(struct mant_structured_input_view, reserved) };
	static const size_t failure[] = {
		FIELD(struct mant_structured_failure_view, status),
		FIELD(struct mant_structured_failure_view, stage),
		FIELD(struct mant_structured_failure_view, limit_kind),
		FIELD(struct mant_structured_failure_view, observed),
		FIELD(struct mant_structured_failure_view, allowed),
		FIELD(struct mant_structured_failure_view, reserved) };
	static const size_t limits[] = {
		FIELD(struct mant_structured_limits, max_input_sources),
		FIELD(struct mant_structured_limits, max_sources),
		FIELD(struct mant_structured_limits, max_source_path_bytes),
		FIELD(struct mant_structured_limits, max_decoded_source_bytes_per_source),
		FIELD(struct mant_structured_limits, max_decoded_source_bytes_total),
		FIELD(struct mant_structured_limits, max_source_map_entries),
		FIELD(struct mant_structured_limits, max_source_map_bytes),
		FIELD(struct mant_structured_limits, max_builder_operations),
		FIELD(struct mant_structured_limits, max_builder_allocated_bytes),
		FIELD(struct mant_structured_limits, max_content_bytes),
		FIELD(struct mant_structured_limits, max_owners),
		FIELD(struct mant_structured_limits, max_blocks),
		FIELD(struct mant_structured_limits, max_content_atoms),
		FIELD(struct mant_structured_limits, max_content_refs),
		FIELD(struct mant_structured_limits, max_content_points),
		FIELD(struct mant_structured_limits, max_links),
		FIELD(struct mant_structured_limits, max_tables),
		FIELD(struct mant_structured_limits, max_table_rows),
		FIELD(struct mant_structured_limits, max_table_cells),
		FIELD(struct mant_structured_limits, max_fixed_views),
		FIELD(struct mant_structured_limits, max_fixed_lines),
		FIELD(struct mant_structured_limits, max_placements),
		FIELD(struct mant_structured_limits, max_decorations),
		FIELD(struct mant_structured_limits, max_forms),
		FIELD(struct mant_structured_limits, max_name_hints),
		FIELD(struct mant_structured_limits, max_relations),
		FIELD(struct mant_structured_limits, max_connection_atoms),
		FIELD(struct mant_structured_limits, max_annotation_runs),
		FIELD(struct mant_structured_limits, max_annotation_mutations),
		FIELD(struct mant_structured_limits, max_relation_edges),
		FIELD(struct mant_structured_limits, max_diagnostics),
		FIELD(struct mant_structured_limits, max_transfer_objects),
		FIELD(struct mant_structured_limits, max_transfer_edges),
		FIELD(struct mant_structured_limits, max_transfer_bytes),
		FIELD(struct mant_structured_limits, max_nesting_depth),
		FIELD(struct mant_structured_limits, max_include_depth),
		FIELD(struct mant_structured_limits, reserved) };
	static const size_t result[] = {
		FIELD(struct mant_structured_result_view, root_source),
		FIELD(struct mant_structured_result_view, profile),
		FIELD(struct mant_structured_result_view, width),
		FIELD(struct mant_structured_result_view, metadata),
		FIELD(struct mant_structured_result_view, sources),
		FIELD(struct mant_structured_result_view, spans),
		FIELD(struct mant_structured_result_view, provenances),
		FIELD(struct mant_structured_result_view, owners),
		FIELD(struct mant_structured_result_view, content_roots),
		FIELD(struct mant_structured_result_view, content_atoms),
		FIELD(struct mant_structured_result_view, content_refs),
		FIELD(struct mant_structured_result_view, content_points),
		FIELD(struct mant_structured_result_view, links),
		FIELD(struct mant_structured_result_view, blocks),
		FIELD(struct mant_structured_result_view, lists),
		FIELD(struct mant_structured_result_view, items),
		FIELD(struct mant_structured_result_view, tables),
		FIELD(struct mant_structured_result_view, table_rows),
		FIELD(struct mant_structured_result_view, table_cells),
		FIELD(struct mant_structured_result_view, fixed_views),
		FIELD(struct mant_structured_result_view, fixed_lines),
		FIELD(struct mant_structured_result_view, placements),
		FIELD(struct mant_structured_result_view, decorations),
		FIELD(struct mant_structured_result_view, forms),
		FIELD(struct mant_structured_result_view, name_hints),
		FIELD(struct mant_structured_result_view, relations),
		FIELD(struct mant_structured_result_view, diagnostics),
		FIELD(struct mant_structured_result_view, reserved) };
	static const size_t source[] = {
		FIELD(struct mant_structured_source_view, key),
		FIELD(struct mant_structured_source_view, identity_kind),
		FIELD(struct mant_structured_source_view, format),
		FIELD(struct mant_structured_source_view, coordinate_kind),
		FIELD(struct mant_structured_source_view, logical_name),
		FIELD(struct mant_structured_source_view, decoded_length),
		FIELD(struct mant_structured_source_view, hash_present),
		FIELD(struct mant_structured_source_view, hash),
		FIELD(struct mant_structured_source_view, reserved_bytes),
		FIELD(struct mant_structured_source_view, reserved) };
	static const size_t span[] = {
		FIELD(struct mant_structured_span_view, line_column_present),
		FIELD(struct mant_structured_span_view, byte_range_present),
		FIELD(struct mant_structured_span_view, reserved_bytes),
		FIELD(struct mant_structured_span_view, source),
		FIELD(struct mant_structured_span_view, line_start),
		FIELD(struct mant_structured_span_view, column_start),
		FIELD(struct mant_structured_span_view, line_end),
		FIELD(struct mant_structured_span_view, column_end),
		FIELD(struct mant_structured_span_view, byte_start),
		FIELD(struct mant_structured_span_view, byte_end),
		FIELD(struct mant_structured_span_view, reserved) };
	static const size_t provenance[] = {
		FIELD(struct mant_structured_provenance_view, kind),
		FIELD(struct mant_structured_provenance_view, authored_span),
		FIELD(struct mant_structured_provenance_view, generated_trigger_span),
		FIELD(struct mant_structured_provenance_view, reserved) };
	static const size_t owner[] = {
		FIELD(struct mant_structured_owner_view, key),
		FIELD(struct mant_structured_owner_view, kind),
		FIELD(struct mant_structured_owner_view, provenance),
		FIELD(struct mant_structured_owner_view, reserved) };
	static const size_t content_root[] = {
		FIELD(struct mant_structured_content_root_view, key),
		FIELD(struct mant_structured_content_root_view, owner),
		FIELD(struct mant_structured_content_root_view, ordinal),
		FIELD(struct mant_structured_content_root_view, kind),
		FIELD(struct mant_structured_content_root_view, provenance),
		FIELD(struct mant_structured_content_root_view, reserved) };
	static const size_t content_atom[] = {
		FIELD(struct mant_structured_content_atom_view, key),
		FIELD(struct mant_structured_content_atom_view, root),
		FIELD(struct mant_structured_content_atom_view, ordinal),
		FIELD(struct mant_structured_content_atom_view, owner),
		FIELD(struct mant_structured_content_atom_view, kind),
		FIELD(struct mant_structured_content_atom_view, style_flags),
		FIELD(struct mant_structured_content_atom_view, role),
		FIELD(struct mant_structured_content_atom_view, link),
		FIELD(struct mant_structured_content_atom_view, text),
		FIELD(struct mant_structured_content_atom_view, display_override_present),
		FIELD(struct mant_structured_content_atom_view, display_reserved_bytes),
		FIELD(struct mant_structured_content_atom_view, display_override),
		FIELD(struct mant_structured_content_atom_view, whitespace_breakable),
		FIELD(struct mant_structured_content_atom_view, reserved_bytes),
		FIELD(struct mant_structured_content_atom_view, provenance),
		FIELD(struct mant_structured_content_atom_view, reserved) };
	static const size_t content_ref[] = {
		FIELD(struct mant_structured_content_ref_view, atom),
		FIELD(struct mant_structured_content_ref_view, byte_start),
		FIELD(struct mant_structured_content_ref_view, byte_end),
		FIELD(struct mant_structured_content_ref_view, reserved) };
	static const size_t content_point[] = {
		FIELD(struct mant_structured_content_point_view, key),
		FIELD(struct mant_structured_content_point_view, root),
		FIELD(struct mant_structured_content_point_view, ordinal),
		FIELD(struct mant_structured_content_point_view, owner),
		FIELD(struct mant_structured_content_point_view, boundary_kind),
		FIELD(struct mant_structured_content_point_view, atom_boundary),
		FIELD(struct mant_structured_content_point_view, atom),
		FIELD(struct mant_structured_content_point_view, byte_offset),
		FIELD(struct mant_structured_content_point_view, scalar_boundary),
		FIELD(struct mant_structured_content_point_view, provenance),
		FIELD(struct mant_structured_content_point_view, reserved) };
	static const size_t link[] = {
		FIELD(struct mant_structured_link_view, key),
		FIELD(struct mant_structured_link_view, owner),
		FIELD(struct mant_structured_link_view, target_kind),
		FIELD(struct mant_structured_link_view, target_a),
		FIELD(struct mant_structured_link_view, target_b_present),
		FIELD(struct mant_structured_link_view, target_b_reserved_bytes),
		FIELD(struct mant_structured_link_view, target_b),
		FIELD(struct mant_structured_link_view, title_present),
		FIELD(struct mant_structured_link_view, title_reserved_bytes),
		FIELD(struct mant_structured_link_view, title),
		FIELD(struct mant_structured_link_view, first_label_ref),
		FIELD(struct mant_structured_link_view, label_ref_count),
		FIELD(struct mant_structured_link_view, provenance),
		FIELD(struct mant_structured_link_view, reserved) };
	static const size_t block[] = {
		FIELD(struct mant_structured_block_view, key),
		FIELD(struct mant_structured_block_view, owner),
		FIELD(struct mant_structured_block_view, kind),
		FIELD(struct mant_structured_block_view, parent),
		FIELD(struct mant_structured_block_view, ordinal),
		FIELD(struct mant_structured_block_view, provenance),
		FIELD(struct mant_structured_block_view, root),
		FIELD(struct mant_structured_block_view, table),
		FIELD(struct mant_structured_block_view, fixed_view),
		FIELD(struct mant_structured_block_view, reserved) };
	static const size_t table[] = {
		FIELD(struct mant_structured_table_view, key),
		FIELD(struct mant_structured_table_view, block),
		FIELD(struct mant_structured_table_view, fixed_view),
		FIELD(struct mant_structured_table_view, provenance),
		FIELD(struct mant_structured_table_view, reserved) };
	static const size_t table_row[] = {
		FIELD(struct mant_structured_table_row_view, key),
		FIELD(struct mant_structured_table_row_view, table),
		FIELD(struct mant_structured_table_row_view, ordinal),
		FIELD(struct mant_structured_table_row_view, provenance),
		FIELD(struct mant_structured_table_row_view, reserved) };
	static const size_t table_cell[] = {
		FIELD(struct mant_structured_table_cell_view, key),
		FIELD(struct mant_structured_table_cell_view, row),
		FIELD(struct mant_structured_table_cell_view, column),
		FIELD(struct mant_structured_table_cell_view, owner),
		FIELD(struct mant_structured_table_cell_view, kind),
		FIELD(struct mant_structured_table_cell_view, alignment),
		FIELD(struct mant_structured_table_cell_view, row_span),
		FIELD(struct mant_structured_table_cell_view, column_span),
		FIELD(struct mant_structured_table_cell_view, provenance),
		FIELD(struct mant_structured_table_cell_view, reserved) };
	static const size_t fixed_view[] = {
		FIELD(struct mant_structured_fixed_view, key),
		FIELD(struct mant_structured_fixed_view, owner),
		FIELD(struct mant_structured_fixed_view, block),
		FIELD(struct mant_structured_fixed_view, table),
		FIELD(struct mant_structured_fixed_view, provenance),
		FIELD(struct mant_structured_fixed_view, reserved) };
	static const size_t fixed_line[] = {
		FIELD(struct mant_structured_fixed_line_view, key),
		FIELD(struct mant_structured_fixed_line_view, view),
		FIELD(struct mant_structured_fixed_line_view, ordinal),
		FIELD(struct mant_structured_fixed_line_view, total_columns),
		FIELD(struct mant_structured_fixed_line_view, reserved) };
	static const size_t placement[] = {
		FIELD(struct mant_structured_placement_view, key),
		FIELD(struct mant_structured_placement_view, line),
		FIELD(struct mant_structured_placement_view, ordinal),
		FIELD(struct mant_structured_placement_view, target_kind),
		FIELD(struct mant_structured_placement_view, atom),
		FIELD(struct mant_structured_placement_view, byte_start),
		FIELD(struct mant_structured_placement_view, byte_end),
		FIELD(struct mant_structured_placement_view, point),
		FIELD(struct mant_structured_placement_view, scalar_start),
		FIELD(struct mant_structured_placement_view, scalar_end),
		FIELD(struct mant_structured_placement_view, column_start),
		FIELD(struct mant_structured_placement_view, column_end),
		FIELD(struct mant_structured_placement_view, cell_map_kind),
		FIELD(struct mant_structured_placement_view, cell_map_value),
		FIELD(struct mant_structured_placement_view, reserved) };
	static const size_t decoration[] = {
		FIELD(struct mant_structured_decoration_view, key),
		FIELD(struct mant_structured_decoration_view, line),
		FIELD(struct mant_structured_decoration_view, ordinal),
		FIELD(struct mant_structured_decoration_view, kind),
		FIELD(struct mant_structured_decoration_view, text),
		FIELD(struct mant_structured_decoration_view, column_start),
		FIELD(struct mant_structured_decoration_view, column_end),
		FIELD(struct mant_structured_decoration_view, provenance),
		FIELD(struct mant_structured_decoration_view, reserved) };
	static const size_t form[] = {
		FIELD(struct mant_structured_form_view, key),
		FIELD(struct mant_structured_form_view, owner),
		FIELD(struct mant_structured_form_view, role),
		FIELD(struct mant_structured_form_view, first_ref),
		FIELD(struct mant_structured_form_view, ref_count),
		FIELD(struct mant_structured_form_view, provenance),
		FIELD(struct mant_structured_form_view, reserved) };
	static const size_t name_hint[] = {
		FIELD(struct mant_structured_name_hint_view, key),
		FIELD(struct mant_structured_name_hint_view, form),
		FIELD(struct mant_structured_name_hint_view, first_ref),
		FIELD(struct mant_structured_name_hint_view, ref_count),
		FIELD(struct mant_structured_name_hint_view, provenance),
		FIELD(struct mant_structured_name_hint_view, reserved) };
	static const size_t relation[] = {
		FIELD(struct mant_structured_relation_view, key),
		FIELD(struct mant_structured_relation_view, owner),
		FIELD(struct mant_structured_relation_view, kind),
		FIELD(struct mant_structured_relation_view, target_owner),
		FIELD(struct mant_structured_relation_view, provenance),
		FIELD(struct mant_structured_relation_view, reserved) };
	static const size_t diagnostic[] = {
		FIELD(struct mant_structured_diagnostic_view, level),
		FIELD(struct mant_structured_diagnostic_view, code),
		FIELD(struct mant_structured_diagnostic_view, message),
		FIELD(struct mant_structured_diagnostic_view, span),
		FIELD(struct mant_structured_diagnostic_view, owner),
		FIELD(struct mant_structured_diagnostic_view, reserved) };
	static const size_t metadata[] = {
		FIELD(struct mant_structured_metadata_view, macroset),
		FIELD(struct mant_structured_metadata_view, presence_flags),
		FIELD(struct mant_structured_metadata_view, title),
		FIELD(struct mant_structured_metadata_view, section),
		FIELD(struct mant_structured_metadata_view, volume),
		FIELD(struct mant_structured_metadata_view, operating_system),
		FIELD(struct mant_structured_metadata_view, architecture),
		FIELD(struct mant_structured_metadata_view, name),
		FIELD(struct mant_structured_metadata_view, date),
		FIELD(struct mant_structured_metadata_view, alias_target),
		FIELD(struct mant_structured_metadata_view, has_body),
		FIELD(struct mant_structured_metadata_view, reserved_bytes),
		FIELD(struct mant_structured_metadata_view, reserved) };
	static const size_t list[] = {
		FIELD(struct mant_structured_list_view, key),
		FIELD(struct mant_structured_list_view, block),
		FIELD(struct mant_structured_list_view, kind),
		FIELD(struct mant_structured_list_view, compact),
		FIELD(struct mant_structured_list_view, start),
		FIELD(struct mant_structured_list_view, provenance),
		FIELD(struct mant_structured_list_view, reserved) };
	static const size_t item[] = {
		FIELD(struct mant_structured_item_view, key),
		FIELD(struct mant_structured_item_view, list),
		FIELD(struct mant_structured_item_view, owner),
		FIELD(struct mant_structured_item_view, ordinal),
		FIELD(struct mant_structured_item_view, first_form),
		FIELD(struct mant_structured_item_view, form_count),
		FIELD(struct mant_structured_item_view, target_present),
		FIELD(struct mant_structured_item_view, target_origin),
		FIELD(struct mant_structured_item_view, target_reserved_bytes),
		FIELD(struct mant_structured_item_view, target),
		FIELD(struct mant_structured_item_view, provenance),
		FIELD(struct mant_structured_item_view, reserved) };

	if (field == 0)
		return SIZE_MAX;
	switch (kind) {
	case MANT_VIEW_INPUT_SOURCE: return PICK(input_source);
	case MANT_VIEW_INPUT: return PICK(input);
	case MANT_VIEW_FAILURE: return PICK(failure);
	case MANT_VIEW_LIMITS: return PICK(limits);
	case MANT_VIEW_RESULT: return PICK(result);
	case MANT_VIEW_SOURCE: return PICK(source);
	case MANT_VIEW_SPAN: return PICK(span);
	case MANT_VIEW_PROVENANCE: return PICK(provenance);
	case MANT_VIEW_OWNER: return PICK(owner);
	case MANT_VIEW_CONTENT_ROOT: return PICK(content_root);
	case MANT_VIEW_CONTENT_ATOM: return PICK(content_atom);
	case MANT_VIEW_CONTENT_REF: return PICK(content_ref);
	case MANT_VIEW_CONTENT_POINT: return PICK(content_point);
	case MANT_VIEW_LINK: return PICK(link);
	case MANT_VIEW_BLOCK: return PICK(block);
	case MANT_VIEW_TABLE: return PICK(table);
	case MANT_VIEW_TABLE_ROW: return PICK(table_row);
	case MANT_VIEW_TABLE_CELL: return PICK(table_cell);
	case MANT_VIEW_FIXED_VIEW: return PICK(fixed_view);
	case MANT_VIEW_FIXED_LINE: return PICK(fixed_line);
	case MANT_VIEW_PLACEMENT: return PICK(placement);
	case MANT_VIEW_DECORATION: return PICK(decoration);
	case MANT_VIEW_FORM: return PICK(form);
	case MANT_VIEW_NAME_HINT: return PICK(name_hint);
	case MANT_VIEW_RELATION: return PICK(relation);
	case MANT_VIEW_DIAGNOSTIC: return PICK(diagnostic);
	case MANT_VIEW_METADATA: return PICK(metadata);
	case MANT_VIEW_LIST: return PICK(list);
	case MANT_VIEW_ITEM: return PICK(item);
	default: return SIZE_MAX;
	}
}
