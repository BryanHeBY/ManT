/* Private borrowed-snapshot boundary around the pinned libmandoc structures. */
#ifndef MANT_MANDOC_SHIM_H
#define MANT_MANDOC_SHIM_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

struct mant_mandoc_document;
struct mant_mandoc_node;
struct mant_mandoc_table_cell;
struct mparse;
#ifdef MANT_MANDOC_EXECUTE
struct mant_mandoc_execution_limits;
struct mant_mandoc_execution_report;
#endif

struct mant_mandoc_source {
	const char		*path;
	const unsigned char	*data;
	size_t			 length;
};

struct mant_mandoc_resolved_source {
	const char		*path;
	const unsigned char	*data;
	size_t			 length;
};

/* Borrowed scalar/string projection of one live libmandoc syntax node. */
struct mant_mandoc_node_view {
	int32_t			 kind;
	uint32_t		 execution_node_key;
	const char		*macro_name;
	const char		*text;
	const char		*tag;
	int32_t			 line;
	int32_t			 column;
	uint64_t		 flow_epoch;
	int32_t			 table_escape;
	int32_t			 table_source_recovery_safe;
	int32_t			 table_row_kind;
	uint32_t		 flags;
	int32_t			 list_kind;
	int32_t			 definition_list_style;
	int32_t			 display_kind;
	int32_t			 font_kind;
	int32_t			 author_mode;
	int32_t			 compact;
	const char		*offset;
	const char		*width;
	const char		*enclosure_open;
	const char		*enclosure_close;
	const char		*equation;
	const struct mant_mandoc_table_cell *table_cells;
	const struct mant_mandoc_table_rule_cell *table_rule_cells;
	const struct mant_mandoc_node *child;
	const struct mant_mandoc_node *next;
};

/* Borrowed projection of one live tbl(7) data cell. */
struct mant_mandoc_table_cell_view {
	const char		*text;
	int32_t			 kind;
	int32_t			 text_block;
	int32_t			 source_recovery_safe;
	int32_t			 vertical_continuation;
	uint32_t		 column_span;
	uint32_t		 row_span;
	int32_t			 alignment;
	const struct mant_mandoc_table_cell *next;
};

/* Borrowed projection of one layout-only tbl(7) rule cell. */
struct mant_mandoc_table_rule_cell_view {
	int32_t			 kind;
	const struct mant_mandoc_table_rule_cell *next;
};

typedef int (*mant_mandoc_source_resolver)(void *, const char *,
    const char *, struct mant_mandoc_resolved_source *);

enum mant_mandoc_macroset {
	MANT_MANDOC_MACROSET_NONE = 0,
	MANT_MANDOC_MACROSET_MDOC = 1,
	MANT_MANDOC_MACROSET_MAN = 2
};

enum mant_mandoc_node_kind {
	MANT_MANDOC_ROOT = 0,
	MANT_MANDOC_BLOCK = 1,
	MANT_MANDOC_HEAD = 2,
	MANT_MANDOC_BODY = 3,
	MANT_MANDOC_TAIL = 4,
	MANT_MANDOC_ELEMENT = 5,
	MANT_MANDOC_TEXT = 6,
	MANT_MANDOC_COMMENT = 7,
	MANT_MANDOC_TABLE = 8,
	MANT_MANDOC_EQUATION = 9
};

enum mant_mandoc_table_row_kind {
	MANT_MANDOC_TABLE_ROW_NONE = 0,
	MANT_MANDOC_TABLE_ROW_DATA = 1,
	MANT_MANDOC_TABLE_ROW_HORIZ = 2,
	MANT_MANDOC_TABLE_ROW_DHORIZ = 3,
	MANT_MANDOC_TABLE_ROW_LAYOUT_RULE = 4
};

enum mant_mandoc_list_kind {
	MANT_MANDOC_LIST_NONE = 0,
	MANT_MANDOC_LIST_BULLET = 1,
	MANT_MANDOC_LIST_ORDERED = 2,
	MANT_MANDOC_LIST_DEFINITION = 3,
	MANT_MANDOC_LIST_COLUMN = 4,
	MANT_MANDOC_LIST_PLAIN = 5
};

enum mant_mandoc_definition_list_style {
	MANT_MANDOC_DEFINITION_LIST_NONE = 0,
	MANT_MANDOC_DEFINITION_LIST_TAG = 1,
	MANT_MANDOC_DEFINITION_LIST_DIAG = 2,
	MANT_MANDOC_DEFINITION_LIST_HANG = 3,
	MANT_MANDOC_DEFINITION_LIST_INSET = 4,
	MANT_MANDOC_DEFINITION_LIST_OHANG = 5
};

enum mant_mandoc_display_kind {
	MANT_MANDOC_DISPLAY_NONE = 0,
	MANT_MANDOC_DISPLAY_LITERAL = 1,
	MANT_MANDOC_DISPLAY_FILLED = 2
};

enum mant_mandoc_font_kind {
	MANT_MANDOC_FONT_NONE = 0,
	MANT_MANDOC_FONT_EMPHASIS = 1,
	MANT_MANDOC_FONT_LITERAL = 2,
	MANT_MANDOC_FONT_SYMBOLIC = 3
};

enum mant_mandoc_author_mode {
	MANT_MANDOC_AUTHOR_NONE = 0,
	MANT_MANDOC_AUTHOR_SPLIT = 1,
	MANT_MANDOC_AUTHOR_NOSPLIT = 2
};

#define MANT_MANDOC_NODE_GENERATED (1U << 0)
#define MANT_MANDOC_NODE_SENTENCE_END (1U << 1)
#define MANT_MANDOC_NODE_NO_PRINT (1U << 2)
#define MANT_MANDOC_NODE_NO_FILL (1U << 3)
#define MANT_MANDOC_NODE_DEEP_LINK_TARGET (1U << 4)
#define MANT_MANDOC_NODE_PERMALINK (1U << 5)
#define MANT_MANDOC_NODE_LINE_START (1U << 6)
#define MANT_MANDOC_NODE_DELIMITER_OPEN (1U << 7)
#define MANT_MANDOC_NODE_DELIMITER_CLOSE (1U << 8)
#define MANT_MANDOC_NODE_SYNOPSIS_PRETTY (1U << 9)
#define MANT_MANDOC_NODE_TABLE_START (1U << 10)

struct mant_mandoc_document *mant_mandoc_parse_file(
    const char *, const char *, int, int, const char *);
struct mant_mandoc_document *mant_mandoc_parse_buffer(
    const char *, const unsigned char *, size_t, const char *, int, int,
    const char *, mant_mandoc_source_resolver, void *);
struct mant_mandoc_document *mant_mandoc_parse_bundle(
    const char *, const struct mant_mandoc_source *, size_t, int,
    const char *);
#ifdef MANT_MANDOC_EXECUTE
struct mant_mandoc_document *mant_mandoc_execute_buffer(
    const char *, const unsigned char *, size_t, int, const char *,
    const struct mant_mandoc_execution_limits *, int (*)(void *), void *);
const struct mant_mandoc_execution_report *mant_mandoc_document_execution(
    const struct mant_mandoc_document *);
#endif
#ifdef MANT_MANDOC_RENDER
struct mant_mandoc_document *mant_mandoc_render_file(
    const char *, const char *, int, int, const char *, int, size_t, int,
    size_t);
struct mant_mandoc_document *mant_mandoc_render_buffer(
    const char *, const unsigned char *, size_t, const char *, int, int,
    const char *, int, size_t, int, size_t, mant_mandoc_source_resolver,
    void *);
struct mant_mandoc_document *mant_mandoc_render_bundle(
    const char *, const struct mant_mandoc_source *, size_t, int,
    const char *, int, size_t, int, size_t);
#endif
void mant_mandoc_document_free(struct mant_mandoc_document *);

/* Called by the patched memory parser for one active bundle or root resolver. */
int mant_mandoc_read_bundle(struct mparse *, const char *);

/* Internal target of the parser-only open() compile redirect. */
int mant_mandoc_source_open(const char *, int, ...);

int mant_mandoc_document_ok(const struct mant_mandoc_document *);
const char *mant_mandoc_document_error(const struct mant_mandoc_document *);
const char *mant_mandoc_document_diagnostics(const struct mant_mandoc_document *);
int mant_mandoc_document_macroset(const struct mant_mandoc_document *);
const char *mant_mandoc_document_title(const struct mant_mandoc_document *);
const char *mant_mandoc_document_section(const struct mant_mandoc_document *);
const char *mant_mandoc_document_volume(const struct mant_mandoc_document *);
const char *mant_mandoc_document_os(const struct mant_mandoc_document *);
const char *mant_mandoc_document_arch(const struct mant_mandoc_document *);
const char *mant_mandoc_document_name(const struct mant_mandoc_document *);
const char *mant_mandoc_document_date(const struct mant_mandoc_document *);
const char *mant_mandoc_document_alias_target(const struct mant_mandoc_document *);
int mant_mandoc_document_has_body(const struct mant_mandoc_document *);
int mant_mandoc_document_equation_truncated(
    const struct mant_mandoc_document *);
int mant_mandoc_is_native_roff_request(const char *, size_t);
size_t mant_mandoc_node_view_size(void);
size_t mant_mandoc_node_view_align(void);
uint32_t mant_mandoc_node_view_field_count(void);
size_t mant_mandoc_node_view_offset(unsigned int);
size_t mant_mandoc_table_cell_view_size(void);
size_t mant_mandoc_table_cell_view_align(void);
uint32_t mant_mandoc_table_cell_view_field_count(void);
size_t mant_mandoc_table_cell_view_offset(unsigned int);
size_t mant_mandoc_table_rule_cell_view_size(void);
size_t mant_mandoc_table_rule_cell_view_align(void);
uint32_t mant_mandoc_table_rule_cell_view_field_count(void);
size_t mant_mandoc_table_rule_cell_view_offset(unsigned int);
const struct mant_mandoc_node *mant_mandoc_document_root(
    const struct mant_mandoc_document *);
int mant_mandoc_node_snapshot(struct mant_mandoc_document *,
    const struct mant_mandoc_node *, struct mant_mandoc_node_view *, size_t);
int mant_mandoc_table_cell_snapshot(const struct mant_mandoc_document *,
    const struct mant_mandoc_table_cell *,
    struct mant_mandoc_table_cell_view *, size_t);
int mant_mandoc_table_rule_cell_snapshot(const struct mant_mandoc_document *,
    const struct mant_mandoc_table_rule_cell *,
    struct mant_mandoc_table_rule_cell_view *, size_t);
#ifdef MANT_MANDOC_RENDER
const unsigned char *mant_mandoc_document_output(
    const struct mant_mandoc_document *);
size_t mant_mandoc_document_output_length(
    const struct mant_mandoc_document *);
int mant_mandoc_document_render_status(
    const struct mant_mandoc_document *);
#endif

#ifdef __cplusplus
}
#endif

#endif
