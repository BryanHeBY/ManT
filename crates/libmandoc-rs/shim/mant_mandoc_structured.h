/* Versioned private ABI for bounded native structured rendering. */
#ifndef MANT_MANDOC_STRUCTURED_H
#define MANT_MANDOC_STRUCTURED_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

struct mant_structured_result;

struct mant_bytes_view {
	const uint8_t *ptr;
	uint64_t len;
};

struct mant_slice_view {
	const void *ptr;
	uint32_t count;
	uint32_t stride;
};

enum mant_structured_status {
	MANT_STRUCTURED_OK = 0,
	MANT_STRUCTURED_INVALID_INPUT = 1,
	MANT_STRUCTURED_REENTRANT = 2,
	MANT_STRUCTURED_BUDGET = 3,
	MANT_STRUCTURED_BUILDER_ALLOC = 4,
	MANT_STRUCTURED_NATIVE = 5,
	MANT_STRUCTURED_RELATION = 6,
	MANT_STRUCTURED_UNSUPPORTED = 7
};

enum mant_structured_stage {
	MANT_STRUCTURED_STAGE_MARSHAL = 1,
	MANT_STRUCTURED_STAGE_RESOLVE = 2,
	MANT_STRUCTURED_STAGE_PARSE = 3,
	MANT_STRUCTURED_STAGE_RENDER = 4,
	MANT_STRUCTURED_STAGE_FINALIZE = 5,
	MANT_STRUCTURED_STAGE_CHECK = 6
};

enum mant_structured_view_kind {
	MANT_VIEW_INPUT_SOURCE = 1,
	MANT_VIEW_INPUT = 2,
	MANT_VIEW_FAILURE = 3,
	MANT_VIEW_LIMITS = 4,
	MANT_VIEW_RESULT = 5,
	MANT_VIEW_SOURCE = 6,
	MANT_VIEW_SPAN = 7,
	MANT_VIEW_PROVENANCE = 8,
	MANT_VIEW_OWNER = 9,
	MANT_VIEW_CONTENT_ROOT = 10,
	MANT_VIEW_CONTENT_ATOM = 11,
	MANT_VIEW_CONTENT_REF = 12,
	MANT_VIEW_CONTENT_POINT = 13,
	MANT_VIEW_LINK = 14,
	MANT_VIEW_BLOCK = 15,
	MANT_VIEW_TABLE = 16,
	MANT_VIEW_TABLE_ROW = 17,
	MANT_VIEW_TABLE_CELL = 18,
	MANT_VIEW_FIXED_VIEW = 19,
	MANT_VIEW_FIXED_LINE = 20,
	MANT_VIEW_PLACEMENT = 21,
	MANT_VIEW_DECORATION = 22,
	MANT_VIEW_FORM = 23,
	MANT_VIEW_NAME_HINT = 24,
	MANT_VIEW_RELATION = 25,
	MANT_VIEW_DIAGNOSTIC = 26,
	MANT_VIEW_METADATA = 27,
	MANT_VIEW_LIST = 28,
	MANT_VIEW_ITEM = 29
};

enum mant_structured_identity_kind {
	MANT_IDENTITY_PATH = 1,
	MANT_IDENTITY_BUNDLE_MEMBER = 2,
	MANT_IDENTITY_ANONYMOUS = 3
};

enum mant_structured_format {
	MANT_FORMAT_MAN = 1,
	MANT_FORMAT_MDOC = 2,
	MANT_FORMAT_MARKDOWN = 3
};

enum mant_structured_profile {
	MANT_PROFILE_UTF8 = 1,
	MANT_PROFILE_ASCII = 2
};

enum mant_structured_coordinate_kind {
	MANT_COORD_DECODED_UTF8_BYTES = 1,
	MANT_COORD_NATIVE_NORMALIZED_BYTES = 2
};

enum mant_structured_diagnostic_level {
	MANT_DIAGNOSTIC_STYLE = 1,
	MANT_DIAGNOSTIC_WARNING = 2,
	MANT_DIAGNOSTIC_ERROR = 3,
	MANT_DIAGNOSTIC_UNSUPPORTED = 4
};

/* Native diagnostic codes are the pinned mandocerr ordinal plus one. */
#define MANT_DIAGNOSTIC_CODE_NATIVE_FIRST 1U
#define MANT_DIAGNOSTIC_CODE_NATIVE_LAST 210U

enum mant_structured_provenance_kind {
	MANT_PROVENANCE_AUTHORED = 1,
	MANT_PROVENANCE_GENERATED = 2,
	MANT_PROVENANCE_UNKNOWN = 3
};

enum mant_structured_atom_kind {
	MANT_ATOM_TEXT = 1,
	MANT_ATOM_WHITESPACE = 2,
	MANT_ATOM_BREAK_OPPORTUNITY = 3,
	MANT_ATOM_HARD_BREAK = 4
};

enum mant_structured_style_flag {
	MANT_STYLE_BOLD = 1U << 0,
	MANT_STYLE_ITALIC = 1U << 1,
	MANT_STYLE_LITERAL = 1U << 2,
	MANT_STYLE_UNDERLINE = 1U << 3
};

enum mant_structured_role {
	MANT_ROLE_FLAG = 1,
	MANT_ROLE_ENVIRONMENT_VARIABLE = 2,
	MANT_ROLE_ARGUMENT = 3,
	MANT_ROLE_COMMAND_OR_DIRECTIVE = 4,
	MANT_ROLE_PATH = 5
};

enum mant_structured_target_origin {
	MANT_TARGET_ORIGIN_ABSENT = 0,
	MANT_TARGET_ORIGIN_GENERATED = 1,
	MANT_TARGET_ORIGIN_AUTHORED = 2
};

enum mant_structured_owner_kind {
	MANT_OWNER_DOCUMENT = 1,
	MANT_OWNER_SECTION = 2,
	MANT_OWNER_PARAGRAPH = 3,
	MANT_OWNER_LIST_ITEM = 4,
	MANT_OWNER_DEFINITION_ITEM = 5,
	MANT_OWNER_TABLE_CELL = 6,
	MANT_OWNER_FIXED_DISPLAY = 7
};

enum mant_structured_root_kind {
	MANT_ROOT_HEADING = 1,
	MANT_ROOT_TERM = 2,
	MANT_ROOT_BODY = 3,
	MANT_ROOT_CELL = 4,
	MANT_ROOT_FIXED_BODY = 5
};

enum mant_structured_block_kind {
	MANT_BLOCK_HEADING = 1,
	MANT_BLOCK_PARAGRAPH = 2,
	MANT_BLOCK_LIST = 3,
	MANT_BLOCK_DEFINITION_LIST = 4,
	MANT_BLOCK_TABLE = 5,
	MANT_BLOCK_INDENTED = 6,
	MANT_BLOCK_FIXED_DISPLAY = 7,
	MANT_BLOCK_VERTICAL_SPACE = 8,
	MANT_BLOCK_THEMATIC_BREAK = 9
};

enum mant_structured_list_kind {
	MANT_LIST_BULLET = 1,
	MANT_LIST_ORDERED = 2,
	MANT_LIST_PLAIN = 3,
	MANT_LIST_DEFINITION = 4,
	MANT_LIST_NATIVE_MARKER = 5
};

enum mant_structured_link_target_kind {
	MANT_LINK_EXTERNAL = 1,
	MANT_LINK_EMAIL = 2,
	MANT_LINK_DOCUMENT = 3,
	MANT_LINK_MANUAL = 4,
	MANT_LINK_SECTION = 5
};

enum mant_structured_point_boundary_kind {
	MANT_POINT_BETWEEN_ATOMS = 1,
	MANT_POINT_IN_ATOM = 2
};

enum mant_structured_placement_target_kind {
	MANT_PLACEMENT_CONTENT = 1,
	MANT_PLACEMENT_POINT = 2
};

enum mant_structured_cell_map_kind {
	MANT_CELL_MAP_AFFINE = 1,
	MANT_CELL_MAP_GRAPHEME_CLUSTER = 2,
	MANT_CELL_MAP_OVERLAY = 3
};

enum mant_structured_table_cell_kind {
	MANT_TABLE_CELL_TEXT = 1,
	MANT_TABLE_CELL_HORIZONTAL_RULE = 2,
	MANT_TABLE_CELL_DOUBLE_HORIZONTAL_RULE = 3,
	MANT_TABLE_CELL_ISOLATED_HORIZONTAL_RULE = 4,
	MANT_TABLE_CELL_ISOLATED_DOUBLE_HORIZONTAL_RULE = 5
};

enum mant_structured_table_alignment {
	MANT_TABLE_ALIGN_LEFT = 1,
	MANT_TABLE_ALIGN_CENTER = 2,
	MANT_TABLE_ALIGN_RIGHT = 3
};

enum mant_structured_decoration_kind {
	MANT_DECORATION_BORDER = 1,
	MANT_DECORATION_RULE = 2,
	MANT_DECORATION_PADDING = 3
};

enum mant_structured_relation_kind {
	MANT_RELATION_ALIAS = 1,
	MANT_RELATION_READING_CONTEXT = 2
};

enum mant_structured_resolve_status {
	MANT_RESOLVE_FOUND = 0,
	MANT_RESOLVE_NOT_FOUND = 1,
	MANT_RESOLVE_DENIED = 2,
	MANT_RESOLVE_IO = 3,
	MANT_RESOLVE_PANIC = 4,
	MANT_RESOLVE_INVALID = 5
};

struct mant_input_source_view {
	uint32_t identity_kind;
	uint32_t format;
	struct mant_bytes_view logical_name;
	struct mant_bytes_view resolver_name;
	struct mant_bytes_view source_bytes;
	uint32_t reserved;
};

typedef uint32_t (*mant_structured_resolve_fn)(void *, uint32_t,
    struct mant_bytes_view, uint32_t *);

struct mant_structured_input_view {
	struct mant_slice_view sources;
	uint32_t root_input;
	uint32_t profile;
	uint32_t width;
	mant_structured_resolve_fn resolve;
	void *resolve_context;
	uint32_t reserved;
};

struct mant_structured_failure_view {
	uint32_t status;
	uint32_t stage;
	uint32_t limit_kind;
	uint64_t observed;
	uint64_t allowed;
	uint32_t reserved;
};

/* Private measurement-only result.  The probe never returns a document. */
struct mant_structured_probe_metrics {
	uint64_t collector_events;
	uint64_t logical_events;
	uint64_t buffer_writes;
	uint64_t cursor_moves;
	uint64_t truncates;
	uint64_t consumes;
	uint64_t partial_consumes;
	uint64_t continued_consumes;
	uint64_t resets;
	uint64_t peak_columns;
	uint64_t peak_slots;
	uint64_t rendered_bytes;
	uint64_t builder_allocated_bytes;
	uint64_t content_bytes;
	uint64_t source_count;
	uint64_t token_count;
	uint64_t slot_capacity;
	uint64_t sidecar_allocated_bytes;
};

struct mant_structured_limits {
	uint64_t max_input_sources;
	uint64_t max_sources;
	uint64_t max_source_path_bytes;
	uint64_t max_decoded_source_bytes_per_source;
	uint64_t max_decoded_source_bytes_total;
	uint64_t max_source_map_entries;
	uint64_t max_source_map_bytes;
	uint64_t max_builder_operations;
	uint64_t max_builder_allocated_bytes;
	uint64_t max_content_bytes;
	uint64_t max_owners;
	uint64_t max_blocks;
	uint64_t max_content_atoms;
	uint64_t max_content_refs;
	uint64_t max_content_points;
	uint64_t max_links;
	uint64_t max_tables;
	uint64_t max_table_rows;
	uint64_t max_table_cells;
	uint64_t max_fixed_views;
	uint64_t max_fixed_lines;
	uint64_t max_placements;
	uint64_t max_decorations;
	uint64_t max_forms;
	uint64_t max_name_hints;
	uint64_t max_relations;
	uint64_t max_connection_atoms;
	uint64_t max_annotation_runs;
	uint64_t max_annotation_mutations;
	uint64_t max_relation_edges;
	uint64_t max_diagnostics;
	uint64_t max_transfer_objects;
	uint64_t max_transfer_edges;
	uint64_t max_transfer_bytes;
	uint64_t max_nesting_depth;
	uint64_t max_include_depth;
	uint32_t reserved;
};

struct mant_structured_metadata_view {
	uint32_t macroset;
	uint32_t presence_flags;
	struct mant_bytes_view title;
	struct mant_bytes_view section;
	struct mant_bytes_view volume;
	struct mant_bytes_view operating_system;
	struct mant_bytes_view architecture;
	struct mant_bytes_view name;
	struct mant_bytes_view date;
	struct mant_bytes_view alias_target;
	uint8_t has_body;
	uint8_t reserved_bytes[3];
	uint32_t reserved;
};

#define MANT_METADATA_TITLE_PRESENT (1U << 0)
#define MANT_METADATA_SECTION_PRESENT (1U << 1)
#define MANT_METADATA_VOLUME_PRESENT (1U << 2)
#define MANT_METADATA_OS_PRESENT (1U << 3)
#define MANT_METADATA_ARCH_PRESENT (1U << 4)
#define MANT_METADATA_NAME_PRESENT (1U << 5)
#define MANT_METADATA_DATE_PRESENT (1U << 6)
#define MANT_METADATA_ALIAS_PRESENT (1U << 7)

struct mant_structured_source_view {
	uint32_t key;
	uint32_t identity_kind;
	uint32_t format;
	uint32_t coordinate_kind;
	struct mant_bytes_view logical_name;
	uint64_t decoded_length;
	uint8_t hash_present;
	uint8_t hash[32];
	uint8_t reserved_bytes[7];
	uint32_t reserved;
};

struct mant_structured_span_view {
	uint8_t line_column_present;
	uint8_t byte_range_present;
	uint8_t reserved_bytes[2];
	uint32_t source;
	uint32_t line_start;
	uint32_t column_start;
	uint32_t line_end;
	uint32_t column_end;
	uint64_t byte_start;
	uint64_t byte_end;
	uint32_t reserved;
};

struct mant_structured_provenance_view {
	uint32_t kind;
	uint32_t authored_span;
	uint32_t generated_trigger_span;
	uint32_t reserved;
};

struct mant_structured_owner_view { uint32_t key, kind, provenance, reserved; };
struct mant_structured_content_root_view {
	uint32_t key, owner, ordinal, kind, provenance, reserved;
};
struct mant_structured_content_atom_view {
	uint32_t key, root, ordinal, owner, kind, style_flags, role, link;
	struct mant_bytes_view text;
	uint8_t display_override_present;
	uint8_t display_reserved_bytes[7];
	struct mant_bytes_view display_override;
	uint8_t whitespace_breakable;
	uint8_t reserved_bytes[3];
	uint32_t provenance;
	uint32_t reserved;
};
struct mant_structured_content_ref_view {
	uint32_t atom, byte_start, byte_end, reserved;
};
struct mant_structured_content_point_view {
	uint32_t key, root, ordinal, owner, boundary_kind, atom_boundary;
	uint32_t atom, byte_offset, scalar_boundary, provenance, reserved;
};
struct mant_structured_link_view {
	uint32_t key, owner, target_kind;
	struct mant_bytes_view target_a;
	uint8_t target_b_present;
	uint8_t target_b_reserved_bytes[7];
	struct mant_bytes_view target_b;
	uint8_t title_present;
	uint8_t title_reserved_bytes[7];
	struct mant_bytes_view title;
	uint32_t first_label_ref, label_ref_count, provenance, reserved;
};
struct mant_structured_block_view {
	uint32_t key, owner, kind, parent, ordinal, provenance;
	uint32_t root, table, fixed_view, reserved;
};
struct mant_structured_list_view {
	uint32_t key, block, kind, compact, start, provenance, reserved;
};
struct mant_structured_item_view {
	uint32_t key, list, owner, ordinal, first_form, form_count;
	uint8_t target_present;
	uint8_t target_origin;
	uint8_t target_reserved_bytes[6];
	struct mant_bytes_view target;
	uint32_t provenance, reserved;
};
struct mant_structured_table_view {
	uint32_t key, block, fixed_view, provenance, reserved;
};
struct mant_structured_table_row_view {
	uint32_t key, table, ordinal, provenance, reserved;
};
struct mant_structured_table_cell_view {
	uint32_t key, row, column, owner, kind, alignment;
	uint32_t row_span, column_span, provenance, reserved;
};
struct mant_structured_fixed_view {
	uint32_t key, owner, block, table, provenance, reserved;
};
struct mant_structured_fixed_line_view {
	uint32_t key, view, ordinal, total_columns, reserved;
};
struct mant_structured_placement_view {
	uint32_t key, line, ordinal, target_kind;
	uint32_t atom, byte_start, byte_end, point;
	uint32_t scalar_start, scalar_end, column_start, column_end;
	uint32_t cell_map_kind, cell_map_value, reserved;
};
struct mant_structured_decoration_view {
	uint32_t key, line, ordinal, kind;
	struct mant_bytes_view text;
	uint32_t column_start, column_end, provenance, reserved;
};
struct mant_structured_form_view {
	uint32_t key, owner, role, first_ref, ref_count, provenance, reserved;
};
struct mant_structured_name_hint_view {
	uint32_t key, form, first_ref, ref_count, provenance, reserved;
};
struct mant_structured_relation_view {
	uint32_t key, owner, kind, target_owner, provenance, reserved;
};
struct mant_structured_diagnostic_view {
	uint32_t level, code;
	struct mant_bytes_view message;
	uint32_t span, owner, reserved;
};

struct mant_structured_result_view {
	uint32_t root_source;
	uint32_t profile;
	uint32_t width;
	struct mant_structured_metadata_view metadata;
	struct mant_slice_view sources;
	struct mant_slice_view spans;
	struct mant_slice_view provenances;
	struct mant_slice_view owners;
	struct mant_slice_view content_roots;
	struct mant_slice_view content_atoms;
	struct mant_slice_view content_refs;
	struct mant_slice_view content_points;
	struct mant_slice_view links;
	struct mant_slice_view blocks;
	struct mant_slice_view lists;
	struct mant_slice_view items;
	struct mant_slice_view tables;
	struct mant_slice_view table_rows;
	struct mant_slice_view table_cells;
	struct mant_slice_view fixed_views;
	struct mant_slice_view fixed_lines;
	struct mant_slice_view placements;
	struct mant_slice_view decorations;
	struct mant_slice_view forms;
	struct mant_slice_view name_hints;
	struct mant_slice_view relations;
	struct mant_slice_view diagnostics;
	uint32_t reserved;
};

uint32_t mant_structured_abi_version(void);
uint64_t mant_structured_discriminant_fingerprint(void);
uint32_t mant_structured_render(const struct mant_structured_input_view *,
    const struct mant_structured_limits *, struct mant_structured_result **,
    struct mant_structured_failure_view *);
uint32_t mant_structured_result_check(const struct mant_structured_result *,
    struct mant_structured_failure_view *);
uint32_t mant_structured_result_view(const struct mant_structured_result *,
    struct mant_structured_result_view *);
void mant_structured_result_free(struct mant_structured_result *);
size_t mant_structured_view_size(uint32_t);
size_t mant_structured_view_align(uint32_t);
size_t mant_structured_view_offset(uint32_t, uint32_t);
void mant_structured_test_fail_after(uint64_t);
uint32_t mant_structured_probe(const struct mant_structured_input_view *,
    const struct mant_structured_limits *,
    struct mant_structured_probe_metrics *,
    struct mant_structured_failure_view *);

struct mparse;
int mant_structured_read_bundle(struct mparse *, const char *);

#ifdef __cplusplus
}
#endif
#endif
