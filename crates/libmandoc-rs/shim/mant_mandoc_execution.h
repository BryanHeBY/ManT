/*
 * ManT-owned, pointer-free transfer boundary for one pinned mandoc terminal
 * execution.  This is a private same-build ABI, not an upstream interface.
 */
#ifndef MANT_MANDOC_EXECUTION_H
#define MANT_MANDOC_EXECUTION_H

#include <stddef.h>
#include <stdint.h>

struct roff_meta;
struct roff_node;

#define MANT_MANDOC_EXEC_NONE UINT32_MAX

enum mant_mandoc_execution_status {
	MANT_MANDOC_EXECUTION_BUILDING = 0,
	MANT_MANDOC_EXECUTION_COMPLETE = 1,
	MANT_MANDOC_EXECUTION_UNSUPPORTED = 2,
	MANT_MANDOC_EXECUTION_BUDGET = 3,
	MANT_MANDOC_EXECUTION_ALLOCATION = 4,
	MANT_MANDOC_EXECUTION_INTERNAL = 5,
	MANT_MANDOC_EXECUTION_CANCELLED = 6
};

enum mant_mandoc_execution_atom_kind {
	MANT_MANDOC_ATOM_GLYPH = 1,
	MANT_MANDOC_ATOM_BREAKABLE_SPACE,
	MANT_MANDOC_ATOM_NONBREAKABLE_SPACE,
	MANT_MANDOC_ATOM_BREAKABLE_HYPHEN,
	MANT_MANDOC_ATOM_ZERO_WIDTH,
	MANT_MANDOC_ATOM_TAB,
	MANT_MANDOC_ATOM_TAB_REFERENCE,
	MANT_MANDOC_ATOM_BACKSPACE,
	MANT_MANDOC_ATOM_WORD_END_BREAK,
	MANT_MANDOC_ATOM_BREAK_POINT
};

enum mant_mandoc_execution_atom_role {
	MANT_MANDOC_ATOM_AUTHORED = 1,
	MANT_MANDOC_ATOM_IMPLICIT_SPACE,
	MANT_MANDOC_ATOM_FONT_DECORATION,
	MANT_MANDOC_ATOM_MACRO_GENERATED,
	MANT_MANDOC_ATOM_DEVICE_GENERATED,
	MANT_MANDOC_ATOM_TABLE_CELL_PAYLOAD,
	MANT_MANDOC_ATOM_EQUATION_CONTENT
};

#define MANT_MANDOC_ATOM_NO_FILL (1U << 0)

#define MANT_MANDOC_EXEC_NODE_GENERATED (1U << 0)

enum mant_mandoc_execution_atom_disposition {
	MANT_MANDOC_ATOM_BUFFERED = 1,
	MANT_MANDOC_ATOM_EMITTED,
	MANT_MANDOC_ATOM_CONSUMED,
	MANT_MANDOC_ATOM_REPLACED,
	MANT_MANDOC_ATOM_TRAILING_DISCARD
};

enum mant_mandoc_execution_font {
	MANT_MANDOC_FONT_ROMAN = 0,
	MANT_MANDOC_FONT_BOLD = 1,
	MANT_MANDOC_FONT_UNDERLINE = 2,
	MANT_MANDOC_FONT_BOLD_UNDERLINE = 3
};

enum mant_mandoc_execution_fragment_role {
	MANT_MANDOC_FRAGMENT_CONTENT = 1,
	MANT_MANDOC_FRAGMENT_FONT_DECORATION = 2,
	MANT_MANDOC_FRAGMENT_MARGIN_DECORATION = 3,
	MANT_MANDOC_FRAGMENT_PAGE_DECORATION = 4
};

/*
 * Native output lifecycle active when a report record was created.  Unlike
 * fragment_role, this applies to state and geometry records that may not
 * emit a fragment of their own.
 */
enum mant_mandoc_execution_output_role {
	MANT_MANDOC_OUTPUT_CONTENT = 1,
	MANT_MANDOC_OUTPUT_MARGIN_DECORATION,
	MANT_MANDOC_OUTPUT_PAGE_DECORATION
};

enum mant_mandoc_execution_flush_outcome {
	MANT_MANDOC_FLUSH_NO_CONTENT = 1,
	MANT_MANDOC_FLUSH_EXHAUSTED = 2,
	MANT_MANDOC_FLUSH_WRAPPED = 3,
	MANT_MANDOC_FLUSH_DEFERRED_COLUMN = 4
};

enum mant_mandoc_execution_buffer_close_reason {
	MANT_MANDOC_BUFFER_RESET = 1,
	MANT_MANDOC_BUFFER_REPORT_END = 2
};

enum mant_mandoc_execution_boundary_request {
	MANT_MANDOC_BOUNDARY_NEWLINE = 1,
	MANT_MANDOC_BOUNDARY_VERTICAL_SPACE,
	MANT_MANDOC_BOUNDARY_ENDLINE,
	MANT_MANDOC_BOUNDARY_DEVICE_ENDLINE
};

enum mant_mandoc_execution_boundary_effect {
	MANT_MANDOC_BOUNDARY_NO_OUTPUT = 0,
	MANT_MANDOC_BOUNDARY_FLUSHED = 1,
	MANT_MANDOC_BOUNDARY_ENDED_LINE = 2,
	MANT_MANDOC_BOUNDARY_ADDED_VERTICAL_SPACE = 3
};

enum mant_mandoc_execution_line_commit_cause {
	/* Non-device boundaries carry no physical-line cause. */
	MANT_MANDOC_LINE_COMMIT_NONE = 0,
	/* Only a raw device callback may have no enclosing formatter boundary. */
	MANT_MANDOC_LINE_COMMIT_DIRECT_DEVICE,
	/* These two causes are children of an ENDLINE boundary. */
	MANT_MANDOC_LINE_COMMIT_FIELD_WRAP,
	MANT_MANDOC_LINE_COMMIT_FIELD_END,
	/* This cause is a child of a VERTICAL_SPACE boundary. */
	MANT_MANDOC_LINE_COMMIT_VERTICAL_BLANK
};

/*
 * Checkpoints are emitted by native handlers after they establish geometry.
 * All geometry fields remain in terminal basic units; consumers must not
 * interpret them as document columns.
 */
enum mant_mandoc_execution_placement_phase {
	MANT_MANDOC_PLACEMENT_ENTER = 1,
	MANT_MANDOC_PLACEMENT_CONTENT,
	MANT_MANDOC_PLACEMENT_HEAD,
	MANT_MANDOC_PLACEMENT_BODY,
	MANT_MANDOC_PLACEMENT_ORIGIN_TRANSITION,
	MANT_MANDOC_PLACEMENT_EXIT
};

enum mant_mandoc_execution_control_request {
	MANT_MANDOC_CONTROL_BREAK = 1,
	MANT_MANDOC_CONTROL_CENTER,
	MANT_MANDOC_CONTROL_FILL,
	MANT_MANDOC_CONTROL_FONT,
	MANT_MANDOC_CONTROL_LINE_LENGTH,
	MANT_MANDOC_CONTROL_MARGIN_CHARACTER,
	MANT_MANDOC_CONTROL_NO_FILL,
	MANT_MANDOC_CONTROL_PAGE_OFFSET,
	MANT_MANDOC_CONTROL_RIGHT_JUSTIFY,
	MANT_MANDOC_CONTROL_VERTICAL_SPACE,
	MANT_MANDOC_CONTROL_TAB_STOPS,
	MANT_MANDOC_CONTROL_TEMPORARY_INDENT
};

enum mant_mandoc_execution_geometry_kind {
	MANT_MANDOC_GEOMETRY_ADVANCE = 1,
	MANT_MANDOC_GEOMETRY_GLYPH,
	MANT_MANDOC_GEOMETRY_ENDLINE,
	MANT_MANDOC_GEOMETRY_FIELD
};

enum mant_mandoc_execution_geometry_unit {
	MANT_MANDOC_UNIT_BASIC = 1,
	MANT_MANDOC_UNIT_BUFFER_SLOT,
	MANT_MANDOC_UNIT_DEVICE_LINE
};

enum mant_mandoc_execution_geometry_origin_kind {
	MANT_MANDOC_GEOMETRY_ORIGIN_NONE = 0,
	MANT_MANDOC_GEOMETRY_ORIGIN_ATOM,
	MANT_MANDOC_GEOMETRY_ORIGIN_FLUSH,
	MANT_MANDOC_GEOMETRY_ORIGIN_BOUNDARY,
	MANT_MANDOC_GEOMETRY_ORIGIN_FRAGMENT
};

enum mant_mandoc_execution_wrapper_kind {
	MANT_MANDOC_WRAPPER_NODE = 1,
	MANT_MANDOC_WRAPPER_FONT,
	MANT_MANDOC_WRAPPER_HEADING,
	MANT_MANDOC_WRAPPER_MDOC_LIST_ITEM,
	MANT_MANDOC_WRAPPER_MAN_BLOCK,
	MANT_MANDOC_WRAPPER_REGION
};

enum mant_mandoc_execution_mdoc_list_kind {
	MANT_MANDOC_MDOC_LIST_BULLET = 1,
	MANT_MANDOC_MDOC_LIST_DASH,
	MANT_MANDOC_MDOC_LIST_ENUM,
	MANT_MANDOC_MDOC_LIST_HANG,
	MANT_MANDOC_MDOC_LIST_HYPHEN,
	MANT_MANDOC_MDOC_LIST_ITEM,
	MANT_MANDOC_MDOC_LIST_OHANG,
	MANT_MANDOC_MDOC_LIST_INSET,
	MANT_MANDOC_MDOC_LIST_DIAG,
	MANT_MANDOC_MDOC_LIST_TAG,
	MANT_MANDOC_MDOC_LIST_COLUMN
};

enum mant_mandoc_execution_man_block_kind {
	MANT_MANDOC_MAN_BLOCK_IP = 1,
	MANT_MANDOC_MAN_BLOCK_TP,
	MANT_MANDOC_MAN_BLOCK_TQ,
	MANT_MANDOC_MAN_BLOCK_HP,
	MANT_MANDOC_MAN_BLOCK_RS,
	MANT_MANDOC_MAN_BLOCK_PP,
	MANT_MANDOC_MAN_BLOCK_P,
	MANT_MANDOC_MAN_BLOCK_LP
};

enum mant_mandoc_execution_region_kind {
	MANT_MANDOC_REGION_MAN_SYNOPSIS_SECTION = 1,
	MANT_MANDOC_REGION_MAN_SYNOPSIS_COMMAND,
	MANT_MANDOC_REGION_MAN_LITERAL_BEGIN,
	MANT_MANDOC_REGION_MAN_LITERAL_END,
	MANT_MANDOC_REGION_MDOC_SYNOPSIS_SECTION,
	MANT_MANDOC_REGION_MDOC_SYNOPSIS_ITEM,
	MANT_MANDOC_REGION_MDOC_DISPLAY_FILLED,
	MANT_MANDOC_REGION_MDOC_DISPLAY_UNFILLED,
	MANT_MANDOC_REGION_MDOC_DISPLAY_LITERAL,
	MANT_MANDOC_REGION_MDOC_DISPLAY_RAGGED,
	MANT_MANDOC_REGION_MDOC_DISPLAY_CENTERED,
	MANT_MANDOC_REGION_MDOC_DISPLAY_ONE_LINE,
	MANT_MANDOC_REGION_MDOC_DISPLAY_ONE_LINE_LITERAL,
	MANT_MANDOC_REGION_CENTERED_LINES,
	MANT_MANDOC_REGION_RIGHT_JUSTIFIED_LINES
};

#define MANT_MANDOC_WRAPPER_MDOC_LIST_COMPACT (1U << 0)

#define MANT_MANDOC_DEFINITION_HAS_HEAD (1U << 0)
#define MANT_MANDOC_DEFINITION_HAS_BODY (1U << 1)
#define MANT_MANDOC_DEFINITION_HEAD_UNCONDITIONAL (1U << 2)
#define MANT_MANDOC_DEFINITION_HEAD_CONDITIONAL (1U << 3)
#define MANT_MANDOC_DEFINITION_COUNT_TRAILING (1U << 4)
#define MANT_MANDOC_DEFINITION_WRAP_AT_FIELD_END (1U << 5)

enum mant_mandoc_execution_heading_kind {
	MANT_MANDOC_HEADING_MAN_SECTION = 1,
	MANT_MANDOC_HEADING_MAN_SUBSECTION,
	MANT_MANDOC_HEADING_MDOC_SECTION,
	MANT_MANDOC_HEADING_MDOC_SUBSECTION
};

enum mant_mandoc_execution_reference_kind {
	MANT_MANDOC_REFERENCE_EXTERNAL_URI = 1,
	MANT_MANDOC_REFERENCE_EMAIL,
	MANT_MANDOC_REFERENCE_MANUAL,
	MANT_MANDOC_REFERENCE_SECTION
};

enum mant_mandoc_execution_reference_presentation {
	MANT_MANDOC_REFERENCE_DIRECT = 1,
	MANT_MANDOC_REFERENCE_LABELLED_SUPPLEMENT
};

enum mant_mandoc_execution_affinity {
	MANT_MANDOC_AFFINITY_INLINE = 1,
	MANT_MANDOC_AFFINITY_BEFORE_OUTPUT
};

enum mant_mandoc_execution_table_row_kind {
	MANT_MANDOC_EXEC_TABLE_ROW_DATA = 1,
	MANT_MANDOC_EXEC_TABLE_ROW_SINGLE_RULE,
	MANT_MANDOC_EXEC_TABLE_ROW_DOUBLE_RULE,
	MANT_MANDOC_EXEC_TABLE_ROW_LAYOUT_RULE
};

enum mant_mandoc_execution_table_data_disposition {
	MANT_MANDOC_EXEC_TABLE_DATA_PROJECTED = 1,
	MANT_MANDOC_EXEC_TABLE_DATA_SUPPRESSED_BY_LAYOUT
};

enum mant_mandoc_execution_table_invocation_outcome {
	MANT_MANDOC_EXEC_TABLE_INVOKE_DATA = 1,
	MANT_MANDOC_EXEC_TABLE_INVOKE_LAYOUT_RULE,
	MANT_MANDOC_EXEC_TABLE_INVOKE_DATA_RULE,
	MANT_MANDOC_EXEC_TABLE_INVOKE_EMPTY,
	MANT_MANDOC_EXEC_TABLE_INVOKE_CONTINUATION
};

enum mant_mandoc_execution_table_layout_kind {
	MANT_MANDOC_EXEC_TABLE_LAYOUT_CENTER = 1,
	MANT_MANDOC_EXEC_TABLE_LAYOUT_RIGHT,
	MANT_MANDOC_EXEC_TABLE_LAYOUT_LEFT,
	MANT_MANDOC_EXEC_TABLE_LAYOUT_NUMERIC,
	MANT_MANDOC_EXEC_TABLE_LAYOUT_SPAN,
	MANT_MANDOC_EXEC_TABLE_LAYOUT_LONG,
	MANT_MANDOC_EXEC_TABLE_LAYOUT_DOWN,
	MANT_MANDOC_EXEC_TABLE_LAYOUT_SINGLE_RULE,
	MANT_MANDOC_EXEC_TABLE_LAYOUT_DOUBLE_RULE
};

enum mant_mandoc_execution_table_data_kind {
	MANT_MANDOC_EXEC_TABLE_DATA_NONE = 1,
	MANT_MANDOC_EXEC_TABLE_DATA_TEXT,
	MANT_MANDOC_EXEC_TABLE_DATA_SINGLE_RULE,
	MANT_MANDOC_EXEC_TABLE_DATA_DOUBLE_RULE,
	MANT_MANDOC_EXEC_TABLE_DATA_ISOLATED_SINGLE_RULE,
	MANT_MANDOC_EXEC_TABLE_DATA_ISOLATED_DOUBLE_RULE
};

enum mant_mandoc_execution_table_alignment {
	MANT_MANDOC_EXEC_TABLE_ALIGN_NONE = 0,
	MANT_MANDOC_EXEC_TABLE_ALIGN_LEFT,
	MANT_MANDOC_EXEC_TABLE_ALIGN_CENTER,
	MANT_MANDOC_EXEC_TABLE_ALIGN_RIGHT,
	MANT_MANDOC_EXEC_TABLE_ALIGN_NUMERIC,
	MANT_MANDOC_EXEC_TABLE_ALIGN_LONG
};

enum mant_mandoc_execution_equation_box_kind {
	MANT_MANDOC_EXEC_EQUATION_TEXT = 1,
	MANT_MANDOC_EXEC_EQUATION_SUBEXPRESSION,
	MANT_MANDOC_EXEC_EQUATION_LIST,
	MANT_MANDOC_EXEC_EQUATION_PILE,
	MANT_MANDOC_EXEC_EQUATION_MATRIX
};

enum mant_mandoc_execution_equation_font {
	MANT_MANDOC_EXEC_EQUATION_FONT_NONE = 0,
	MANT_MANDOC_EXEC_EQUATION_FONT_ROMAN,
	MANT_MANDOC_EXEC_EQUATION_FONT_BOLD,
	MANT_MANDOC_EXEC_EQUATION_FONT_FAT,
	MANT_MANDOC_EXEC_EQUATION_FONT_ITALIC
};

enum mant_mandoc_execution_equation_position {
	MANT_MANDOC_EXEC_EQUATION_POSITION_NONE = 0,
	MANT_MANDOC_EXEC_EQUATION_POSITION_SUPERSCRIPT,
	MANT_MANDOC_EXEC_EQUATION_POSITION_SUBSUPERSCRIPT,
	MANT_MANDOC_EXEC_EQUATION_POSITION_SUBSCRIPT,
	MANT_MANDOC_EXEC_EQUATION_POSITION_TO,
	MANT_MANDOC_EXEC_EQUATION_POSITION_FROM,
	MANT_MANDOC_EXEC_EQUATION_POSITION_FROMTO,
	MANT_MANDOC_EXEC_EQUATION_POSITION_OVER,
	MANT_MANDOC_EXEC_EQUATION_POSITION_SQUARE_ROOT
};

enum mant_mandoc_execution_equation_part_kind {
	MANT_MANDOC_EXEC_EQUATION_PART_TEXT = 1,
	MANT_MANDOC_EXEC_EQUATION_PART_LEFT_FENCE,
	MANT_MANDOC_EXEC_EQUATION_PART_RIGHT_FENCE,
	MANT_MANDOC_EXEC_EQUATION_PART_OPERATOR,
	MANT_MANDOC_EXEC_EQUATION_PART_SQUARE_ROOT,
	MANT_MANDOC_EXEC_EQUATION_PART_TOP_DECORATOR,
	MANT_MANDOC_EXEC_EQUATION_PART_BOTTOM_DECORATOR
};

#define MANT_MANDOC_EXEC_EQUATION_INLINE (1U << 0)
#define MANT_MANDOC_EXEC_EQUATION_DISPLAY (1U << 1)
#define MANT_MANDOC_EXEC_EQUATION_NO_CONTENT (1U << 2)

#define MANT_MANDOC_EXEC_TABLE_CELL_TOP_ALIGN (1U << 0)
#define MANT_MANDOC_EXEC_TABLE_CELL_UP (1U << 1)
#define MANT_MANDOC_EXEC_TABLE_CELL_BOTTOM_ALIGN (1U << 2)
#define MANT_MANDOC_EXEC_TABLE_CELL_ZERO_WIDTH (1U << 3)
#define MANT_MANDOC_EXEC_TABLE_CELL_EQUAL_WIDTH (1U << 4)
#define MANT_MANDOC_EXEC_TABLE_CELL_MAX_WIDTH (1U << 5)
#define MANT_MANDOC_EXEC_TABLE_CELL_TEXT_BLOCK (1U << 6)
#define MANT_MANDOC_EXEC_TABLE_CELL_SOURCE_SAFE (1U << 7)
#define MANT_MANDOC_EXEC_TABLE_CELL_VERTICAL_CONTINUATION (1U << 8)

struct mant_mandoc_execution_limits {
	uint32_t abi_version;
	uint32_t abi_size;
	uint64_t max_nodes;
	uint64_t max_depth;
	uint64_t max_work;
	uint64_t max_records;
	uint64_t max_pool_bytes;
	uint64_t max_buffer_cells;
	uint64_t max_report_bytes;
};

#define MANT_MANDOC_EXECUTION_LIMITS_VERSION 4U
size_t mant_mandoc_execution_limits_size(void);
size_t mant_mandoc_execution_limits_align(void);
uint32_t mant_mandoc_execution_limits_field_count(void);
size_t mant_mandoc_execution_limits_offset(uint32_t);

struct mant_mandoc_source_record {
	uint32_t key;
	uint32_t parent;
	uint32_t include_node;
	uint32_t flags;
	uint32_t path_start;
	uint32_t path_length;
};

struct mant_mandoc_node_record {
	uint32_t key;
	uint32_t parent;
	uint32_t source;
	uint32_t line;
	uint32_t column;
	uint32_t kind;
	uint32_t flags;
	uint32_t macro_start;
	uint32_t macro_length;
};

struct mant_mandoc_buffer_generation_record {
	uint32_t key;
	uint32_t buffer;
	uint32_t generation;
	uint32_t capacity;
	uint32_t extent;
	uint32_t close_reason;
	uint32_t reserved;
	uint64_t open_sequence;
	uint64_t close_sequence;
};

struct mant_mandoc_word_record {
	uint32_t key;
	uint32_t node;
	uint32_t source;
	uint32_t operand_start;
	uint32_t operand_length;
	uint32_t role;
	uint32_t wrapper;
	uint32_t enter_atom;
	uint32_t leave_atom;
	uint32_t reserved;
	uint64_t enter_sequence;
	uint64_t leave_sequence;
};

struct mant_mandoc_atom_record {
	uint32_t key;
	uint32_t buffer;
	uint32_t generation;
	uint32_t buffer_generation;
	uint32_t slot;
	uint32_t kind;
	uint32_t role;
	uint32_t input_scalar;
	uint32_t display_scalar;
	int64_t width_bu;
	uint32_t node;
	uint32_t source;
	uint32_t operand_start;
	uint32_t operand_length;
	uint32_t font;
	uint32_t wrapper;
	uint32_t replaced_by;
	uint32_t disposition;
	uint32_t flags;
	uint32_t output_role;
	uint64_t sequence;
};

struct mant_mandoc_fragment_record {
	uint32_t key;
	uint32_t node;
	uint32_t buffer;
	uint32_t generation;
	uint32_t buffer_generation;
	uint32_t atom;
	uint32_t reserved_atom;
	uint32_t device_line;
	uint32_t role;
	uint32_t wrapper;
	uint32_t reserved;
	int64_t start_bu;
	int64_t end_bu;
	uint64_t sequence;
};

struct mant_mandoc_logical_tab_record {
	uint32_t row_epoch;
	uint32_t reserved;
	int64_t destination_bu;
};

struct mant_mandoc_flush_record {
	uint32_t key;
	uint32_t node;
	uint32_t buffer;
	uint32_t generation;
	uint32_t buffer_generation;
	uint32_t scan_start;
	uint32_t scan_end;
	uint32_t accepted_start;
	uint32_t accepted_end;
	uint32_t consumed_start;
	uint32_t consumed_end;
	uint32_t tail_discarded_start;
	uint32_t tail_discarded_end;
	uint32_t remaining_start;
	uint32_t remaining_end;
	uint32_t fragment_start;
	uint32_t fragment_length;
	uint32_t flags_before;
	uint32_t flags_after;
	uint32_t boundary;
	uint32_t outcome;
	uint32_t logical_tab_start;
	uint32_t logical_tab_length;
	int64_t leading_bu;
	int64_t content_bu;
	int64_t logical_content_bu;
	int64_t logical_fit_content_bu;
	int64_t logical_field_bu;
	int64_t logical_origin_bu;
	int64_t effective_content_bu;
	uint32_t logical_forced_break;
	uint32_t output_role;
	int64_t field_bu;
	int64_t target_bu;
	int64_t offset_bu;
	int64_t rmargin_bu;
	int64_t maxrmargin_bu;
	int64_t cell_bu;
	uint64_t minimum_blank_cells;
	uint64_t trailing_blank_cells;
	int64_t taboff_before;
	int64_t taboff_after;
	int64_t visual_before;
	int64_t visual_after;
	uint64_t sequence;
	uint64_t outcome_sequence;
};

struct mant_mandoc_boundary_record {
	uint32_t key;
	uint32_t node;
	uint32_t parent;
	uint32_t request;
	uint32_t effect;
	uint32_t flags_before;
	uint32_t flags_after;
	uint32_t control;
	int64_t line_before;
	int64_t line_after;
	int64_t visual_before;
	int64_t visual_after;
	uint32_t direct_device_lines;
	uint32_t wrapper;
	uint32_t line_commit_cause;
	uint32_t output_role;
	uint64_t enter_sequence;
	uint64_t leave_sequence;
};

struct mant_mandoc_placement_record {
	/* Dense report-local identity and native syntax owner. */
	uint32_t key;
	uint32_t node;
	/* Active structural wrapper containing node and sequence. */
	uint32_t wrapper;
	uint32_t phase;
	/* Exact terminal basic-unit geometry at this execution point. */
	int64_t offset_bu;
	int64_t rmargin_bu;
	int64_t maxrmargin_bu;
	int64_t visual_bu;
	int64_t cell_bu;
	uint64_t sequence;
};

struct mant_mandoc_control_record {
	uint32_t key;
	uint32_t node;
	uint32_t parent;
	uint32_t wrapper;
	uint32_t request;
	uint32_t buffer;
	uint32_t generation_before;
	uint32_t generation_after;
	uint32_t flags_before;
	uint32_t flags_after;
	uint32_t atom_start;
	uint32_t atom_length;
	uint32_t fragment_start;
	uint32_t fragment_length;
	uint32_t flush_start;
	uint32_t flush_length;
	uint32_t boundary_start;
	uint32_t boundary_length;
	uint32_t geometry_start;
	uint32_t geometry_length;
	uint32_t wrapper_start;
	uint32_t wrapper_length;
	uint32_t reserved;
	int64_t line_before;
	int64_t line_after;
	int64_t visual_before;
	int64_t visual_after;
	int64_t column_before;
	int64_t column_after;
	int64_t extent_before;
	int64_t extent_after;
	int64_t offset_before;
	int64_t offset_after;
	int64_t rmargin_before;
	int64_t rmargin_after;
	int64_t maxrmargin_before;
	int64_t maxrmargin_after;
	int64_t taboff_before;
	int64_t taboff_after;
	int64_t temporary_indent_before;
	int64_t temporary_indent_after;
	int64_t skip_vertical_before;
	int64_t skip_vertical_after;
	int64_t minimum_blank_before;
	int64_t minimum_blank_after;
	int64_t trailing_blank_before;
	int64_t trailing_blank_after;
	uint64_t enter_sequence;
	uint64_t leave_sequence;
};

struct mant_mandoc_geometry_record {
	uint32_t key;
	uint32_t node;
	uint32_t related;
	uint32_t kind;
	uint32_t unit;
	uint32_t origin_kind;
	uint32_t origin_key;
	uint32_t output_role;
	int64_t requested;
	int64_t effective;
	int64_t before;
	int64_t after;
	uint64_t sequence;
};

struct mant_mandoc_wrapper_record {
	uint32_t key;
	uint32_t parent;
	uint32_t node;
	uint32_t kind;
	uint32_t target_start;
	uint32_t target_length;
	uint32_t enter_atom;
	uint32_t leave_atom;
	uint32_t detail;
	uint32_t flags;
	uint32_t state_before;
	uint32_t state_after;
	uint32_t depth_before;
	uint32_t depth_after;
	uint32_t definition_head_node;
	uint32_t definition_body_node;
	uint32_t definition_flags;
	uint32_t definition_reserved;
	int64_t definition_head_offset_bu;
	int64_t definition_head_rmargin_bu;
	int64_t definition_head_maxrmargin_bu;
	int64_t definition_body_offset_bu;
	int64_t definition_body_rmargin_bu;
	int64_t definition_body_maxrmargin_bu;
	int64_t definition_cell_bu;
	uint64_t definition_trailing_blank_cells;
	uint64_t definition_head_sequence;
	uint64_t definition_body_sequence;
	uint64_t enter_sequence;
	uint64_t leave_sequence;
};

struct mant_mandoc_reference_record {
	uint32_t key;
	uint32_t parent;
	uint32_t owner_node;
	uint32_t target_node;
	uint32_t kind;
	uint32_t primary_start;
	uint32_t primary_length;
	uint32_t secondary_start;
	uint32_t secondary_length;
	uint32_t enter_atom;
	uint32_t label_start_atom;
	uint32_t label_end_atom;
	uint32_t target_start_atom;
	uint32_t target_end_atom;
	uint32_t supplement_start_atom;
	uint32_t supplement_end_atom;
	uint32_t leave_atom;
	uint32_t presentation;
	uint32_t affinity;
	uint32_t flags;
	uint64_t enter_sequence;
	uint64_t leave_sequence;
};

struct mant_mandoc_anchor_record {
	uint32_t key;
	uint32_t node;
	uint32_t target_start;
	uint32_t target_length;
	uint32_t device_line;
	uint32_t atom_cursor;
	uint32_t fragment_cursor;
	uint32_t affinity;
	uint32_t reserved;
	uint64_t sequence;
};

struct mant_mandoc_table_record {
	uint32_t key;
	uint32_t first_row_node;
	uint32_t row_start;
	uint32_t row_length;
	uint32_t layout_start;
	uint32_t layout_length;
	uint32_t data_start;
	uint32_t data_length;
	uint32_t invocation_start;
	uint32_t invocation_length;
	uint32_t logical_columns;
	uint32_t wrapper;
	uint32_t flags;
	uint32_t enter_atom;
	uint32_t leave_atom;
	uint32_t enter_fragment;
	uint32_t leave_fragment;
	uint32_t enter_flush;
	uint32_t leave_flush;
	int64_t offset_bu;
	int64_t cell_bu;
	uint64_t enter_sequence;
	uint64_t leave_sequence;
};

struct mant_mandoc_table_row_record {
	uint32_t key;
	uint32_t table;
	uint32_t node;
	uint32_t ordinal;
	uint32_t kind;
	uint32_t logical_columns;
	uint32_t layout_start;
	uint32_t layout_length;
	uint32_t data_start;
	uint32_t data_length;
	uint32_t invocation_start;
	uint32_t invocation_length;
	uint32_t enter_atom;
	uint32_t leave_atom;
	uint32_t enter_fragment;
	uint32_t leave_fragment;
	uint32_t enter_flush;
	uint32_t leave_flush;
	uint64_t enter_sequence;
	uint64_t leave_sequence;
};

struct mant_mandoc_table_layout_cell_record {
	uint32_t key;
	uint32_t row;
	uint32_t ordinal;
	uint32_t logical_column;
	uint32_t kind;
	uint32_t alignment;
	uint32_t font;
	uint32_t flags;
	uint32_t horizontal_continuation;
	uint32_t spacing_en;
	uint32_t vertical_rule;
	uint32_t reserved;
};

struct mant_mandoc_table_data_cell_record {
	uint32_t key;
	uint32_t row;
	uint32_t ordinal;
	uint32_t layout;
	uint32_t kind;
	uint32_t disposition;
	uint32_t column_span;
	uint32_t row_span;
	uint32_t flags;
	uint32_t equation_delimiters;
	uint32_t equation_opening;
	uint32_t equation_closing;
};

struct mant_mandoc_table_cell_invocation_record {
	uint32_t key;
	uint32_t row;
	uint32_t node;
	uint32_t ordinal;
	uint32_t layout;
	uint32_t data;
	uint32_t outcome;
	uint32_t logical_column;
	uint32_t buffer;
	uint32_t buffer_generation;
	uint32_t enter_atom;
	uint32_t leave_atom;
	int64_t offset_bu;
	int64_t rmargin_bu;
	int64_t coloff_before_bu;
	int64_t coloff_after_bu;
	uint64_t enter_sequence;
	uint64_t leave_sequence;
};

/* One final-tree ROFFT_EQN node and its owned structural/execution ranges. */
struct mant_mandoc_equation_record {
	uint32_t key;
	uint32_t node;
	uint32_t root_box;
	uint32_t box_start;
	uint32_t box_length;
	uint32_t execution_start;
	uint32_t execution_length;
	uint32_t part_start;
	uint32_t part_length;
	uint32_t flags;
	uint32_t enter_atom;
	uint32_t leave_atom;
	uint32_t reserved0;
	uint32_t reserved1;
	uint64_t enter_sequence;
	uint64_t leave_sequence;
};

/* Pointer-free preorder snapshot of one pinned eqn_box. */
struct mant_mandoc_equation_box_record {
	uint32_t key;
	uint32_t equation;
	uint32_t parent;
	uint32_t first_child;
	uint32_t next_sibling;
	uint32_t subtree_end;
	uint32_t ordinal;
	uint32_t kind;
	uint32_t font;
	uint32_t position;
	int32_t size;
	uint64_t expected_args;
	uint64_t actual_args;
	uint32_t text_start;
	uint32_t text_length;
	uint32_t left_start;
	uint32_t left_length;
	uint32_t right_start;
	uint32_t right_length;
	uint32_t top_start;
	uint32_t top_length;
	uint32_t bottom_start;
	uint32_t bottom_length;
	uint32_t reserved;
};

/* One actual recursive eqn_box() invocation; structural wrappers may be skipped. */
struct mant_mandoc_equation_box_execution_record {
	uint32_t key;
	uint32_t equation;
	uint32_t box;
	uint32_t parent_execution;
	uint32_t part_start;
	uint32_t part_length;
	uint32_t enter_atom;
	uint32_t leave_atom;
	uint32_t reserved0;
	uint32_t reserved1;
	uint64_t enter_sequence;
	uint64_t leave_sequence;
};

/* One eqn_term.c generated formatter word and its exact native owner. */
struct mant_mandoc_equation_part_record {
	uint32_t key;
	uint32_t equation;
	uint32_t box;
	uint32_t execution;
	uint32_t kind;
	uint32_t word_start;
	uint32_t word_length;
	uint32_t atom_start;
	uint32_t atom_length;
	uint32_t reserved;
	uint64_t enter_sequence;
	uint64_t leave_sequence;
};

struct mant_mandoc_execution_diagnostic_record {
	uint32_t code;
	uint32_t node;
	uint32_t message_start;
	uint32_t message_length;
	uint64_t sequence;
};

struct mant_mandoc_execution_report;

struct mant_mandoc_execution_report *mant_mandoc_execution_alloc(
    const char *, const struct mant_mandoc_execution_limits *,
    int (*)(void *), void *);
int mant_mandoc_execution_run(struct mant_mandoc_execution_report *,
    const struct roff_meta *);
void mant_mandoc_execution_free(struct mant_mandoc_execution_report *);
int mant_mandoc_execution_status(const struct mant_mandoc_execution_report *);
const char *mant_mandoc_execution_error(
    const struct mant_mandoc_execution_report *);

/* Native invariant smoke test used only by the repository test suite. */
uint32_t mant_mandoc_execution_validation_selftest(void);
size_t mant_mandoc_execution_pool_length(
    const struct mant_mandoc_execution_report *);
uint64_t mant_mandoc_execution_work_count(
    const struct mant_mandoc_execution_report *);
uint64_t mant_mandoc_execution_record_count(
    const struct mant_mandoc_execution_report *);
uint64_t mant_mandoc_execution_allocated_record_bytes(
    const struct mant_mandoc_execution_report *);
uint64_t mant_mandoc_execution_buffer_cell_count(
    const struct mant_mandoc_execution_report *);
uint32_t mant_mandoc_execution_content_indent_columns(
    const struct mant_mandoc_execution_report *);
int mant_mandoc_execution_node_key(
    const struct mant_mandoc_execution_report *, const struct roff_node *,
    uint32_t *);

#define MANT_DECLARE_RECORD_API(name, plural) \
	size_t mant_mandoc_execution_##name##_count( \
	    const struct mant_mandoc_execution_report *); \
	size_t mant_mandoc_execution_##name##_size(void); \
	size_t mant_mandoc_execution_##name##_align(void); \
	uint32_t mant_mandoc_execution_##name##_field_count(void); \
	size_t mant_mandoc_execution_##name##_offset(uint32_t); \
	int mant_mandoc_execution_copy_##plural( \
	    const struct mant_mandoc_execution_report *, size_t, void *, \
	    size_t, size_t, size_t, size_t *)

MANT_DECLARE_RECORD_API(source, sources);
MANT_DECLARE_RECORD_API(node, nodes);
MANT_DECLARE_RECORD_API(buffer_generation, buffer_generations);
MANT_DECLARE_RECORD_API(word, words);
MANT_DECLARE_RECORD_API(atom, atoms);
MANT_DECLARE_RECORD_API(fragment, fragments);
MANT_DECLARE_RECORD_API(logical_tab, logical_tabs);
MANT_DECLARE_RECORD_API(flush, flushes);
MANT_DECLARE_RECORD_API(boundary, boundaries);
MANT_DECLARE_RECORD_API(control, controls);
MANT_DECLARE_RECORD_API(geometry, geometries);
MANT_DECLARE_RECORD_API(placement, placements);
MANT_DECLARE_RECORD_API(wrapper, wrappers);
MANT_DECLARE_RECORD_API(reference, references);
MANT_DECLARE_RECORD_API(anchor, anchors);
MANT_DECLARE_RECORD_API(table, tables);
MANT_DECLARE_RECORD_API(table_row, table_rows);
MANT_DECLARE_RECORD_API(table_layout_cell, table_layout_cells);
MANT_DECLARE_RECORD_API(table_data_cell, table_data_cells);
MANT_DECLARE_RECORD_API(table_cell_invocation, table_cell_invocations);
MANT_DECLARE_RECORD_API(equation, equations);
MANT_DECLARE_RECORD_API(equation_box, equation_boxes);
MANT_DECLARE_RECORD_API(equation_box_execution, equation_box_executions);
MANT_DECLARE_RECORD_API(equation_part, equation_parts);
MANT_DECLARE_RECORD_API(diagnostic, diagnostics);

#undef MANT_DECLARE_RECORD_API

int mant_mandoc_execution_copy_pool(
    const struct mant_mandoc_execution_report *, size_t, void *, size_t,
    size_t *);

#endif
