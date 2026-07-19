/* Stable, owned boundary around the private libmandoc 1.14.6 structures. */
#ifndef MANT_MANDOC_SHIM_H
#define MANT_MANDOC_SHIM_H

#ifdef __cplusplus
extern "C" {
#endif

struct mant_mandoc_document;
struct mant_mandoc_node;
struct mant_mandoc_table_cell;

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

enum mant_mandoc_list_kind {
	MANT_MANDOC_LIST_NONE = 0,
	MANT_MANDOC_LIST_BULLET = 1,
	MANT_MANDOC_LIST_ORDERED = 2,
	MANT_MANDOC_LIST_DEFINITION = 3,
	MANT_MANDOC_LIST_COLUMN = 4,
	MANT_MANDOC_LIST_PLAIN = 5
};

enum mant_mandoc_display_kind {
	MANT_MANDOC_DISPLAY_NONE = 0,
	MANT_MANDOC_DISPLAY_LITERAL = 1,
	MANT_MANDOC_DISPLAY_FILLED = 2
};

#define MANT_MANDOC_NODE_GENERATED (1U << 0)
#define MANT_MANDOC_NODE_SENTENCE_END (1U << 1)
#define MANT_MANDOC_NODE_NO_PRINT (1U << 2)
#define MANT_MANDOC_NODE_NO_FILL (1U << 3)

struct mant_mandoc_document *mant_mandoc_parse_file(const char *, int);
void mant_mandoc_document_free(struct mant_mandoc_document *);

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
const struct mant_mandoc_node *mant_mandoc_document_root(
    const struct mant_mandoc_document *);

int mant_mandoc_node_kind(const struct mant_mandoc_node *);
const char *mant_mandoc_node_macro(const struct mant_mandoc_node *);
const char *mant_mandoc_node_text(const struct mant_mandoc_node *);
int mant_mandoc_node_line(const struct mant_mandoc_node *);
int mant_mandoc_node_column(const struct mant_mandoc_node *);
unsigned int mant_mandoc_node_flags(const struct mant_mandoc_node *);
int mant_mandoc_node_list_kind(const struct mant_mandoc_node *);
int mant_mandoc_node_display_kind(const struct mant_mandoc_node *);
int mant_mandoc_node_compact(const struct mant_mandoc_node *);
const char *mant_mandoc_node_offset(const struct mant_mandoc_node *);
const char *mant_mandoc_node_equation(const struct mant_mandoc_node *);
const struct mant_mandoc_table_cell *mant_mandoc_node_table_cells(
    const struct mant_mandoc_node *);
const char *mant_mandoc_table_cell_text(
    const struct mant_mandoc_table_cell *);
unsigned int mant_mandoc_table_cell_column_span(
    const struct mant_mandoc_table_cell *);
unsigned int mant_mandoc_table_cell_row_span(
    const struct mant_mandoc_table_cell *);
int mant_mandoc_table_cell_alignment(
    const struct mant_mandoc_table_cell *);
const struct mant_mandoc_table_cell *mant_mandoc_table_cell_next(
    const struct mant_mandoc_table_cell *);
const struct mant_mandoc_node *mant_mandoc_node_child(
    const struct mant_mandoc_node *);
const struct mant_mandoc_node *mant_mandoc_node_next(
    const struct mant_mandoc_node *);

#ifdef __cplusplus
}
#endif

#endif
