/*
 * Owned compatibility layer for the pinned libmandoc parser.
 *
 * libmandoc's syntax tree and diagnostic writer are private, process-global
 * implementation details.  Copying a completed parse into these small opaque
 * structures lets Rust release the parser before crossing the FFI boundary.
 */
#include "config.h"

#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "mandoc.h"
#include "mdoc.h"
#include "eqn.h"
#include "roff.h"
#include "tbl.h"
#include "mandoc_parse.h"

#include "mant_mandoc_shim.h"

struct mant_mandoc_table_cell {
	char			*text;
	unsigned int		 column_span;
	unsigned int		 row_span;
	int			 alignment;
	struct mant_mandoc_table_cell *next;
};

struct mant_mandoc_node {
	int			 kind;
	char			*macro;
	char			*text;
	char			*tag;
	int			 line;
	int			 column;
	unsigned int		 flags;
	int			 list_kind;
	int			 display_kind;
	int			 compact;
	char			*offset;
	char			*equation;
	struct mant_mandoc_table_cell *table_cells;
	struct mant_mandoc_node	*child;
	struct mant_mandoc_node	*next;
};

struct mant_mandoc_document {
	int			 ok;
	char			*error;
	char			*diagnostics;
	int			 macroset;
	char			*title;
	char			*section;
	char			*volume;
	char			*os;
	char			*arch;
	char			*name;
	char			*date;
	char			*alias_target;
	int			 has_body;
	struct mant_mandoc_node	*root;
};

static char *source_root;
static int source_root_strict;

/*
 * Maximum node nesting copied out of libmandoc's tree.
 *
 * libmandoc bounds .so include depth but not block/inline nesting, so a
 * pathological or hostile page (thousands of nested .RS or .Bl) yields a tree
 * deep enough to overflow the stack when it is copied, freed, lowered, or
 * dropped. Capping depth once at the copy boundary keeps the owned tree finite,
 * which transitively bounds every later recursive walk on both sides of the
 * FFI. Real manuals nest only a handful of levels, far below this limit.
 */
#define MANT_MANDOC_MAX_COPY_DEPTH 256

static char *copy_string(const char *);
static struct mant_mandoc_document *parse_input(const char *,
    const unsigned char *, size_t, const char *, int);
static char *read_diagnostics(FILE *);
static struct mant_mandoc_node *copy_node(const struct roff_node *, int);
static void free_node(struct mant_mandoc_node *);
static int document_has_body(const struct roff_meta *);
static void set_source_root_from_path(const char *);
static void set_source_root_directory(const char *);
static void copy_normalized_data(struct mant_mandoc_node *,
    const struct roff_node *);
static struct mant_mandoc_table_cell *copy_table_cells(
    const struct tbl_span *);
static void free_table_cells(struct mant_mandoc_table_cell *);
static char *copy_equation(const struct eqn_box *);

struct mant_mandoc_document *
mant_mandoc_parse_file(const char *path, const char *include_root,
    int allow_include)
{
	return parse_input(path, NULL, 0, include_root, allow_include);
}

struct mant_mandoc_document *
mant_mandoc_parse_buffer(const char *path, const unsigned char *buffer,
    size_t length, const char *include_root, int allow_include)
{
	return parse_input(path, buffer, length, include_root, allow_include);
}

static struct mant_mandoc_document *
parse_input(const char *path, const unsigned char *buffer, size_t length,
    const char *include_root, int allow_include)
{
	struct mant_mandoc_document	*document;
	struct mparse			*parser;
	struct roff_meta			*meta;
	FILE				*input, *messages;
	int				 fd, options, saved_errno;

	document = calloc(1, sizeof(*document));
	if (document == NULL)
		return NULL;
	if (path == NULL || *path == '\0') {
		document->error = copy_string("manual source path is empty");
		return document;
	}
	if (buffer == NULL && length != 0) {
		document->error = copy_string("manual source buffer is missing");
		return document;
	}

	options = MPARSE_UTF8 | MPARSE_LATIN1 | MPARSE_VALIDATE | MPARSE_COMMENT;
	if (allow_include)
		options |= MPARSE_SO;

	messages = tmpfile();
	setprogname("mant");
	mandoc_msg_setoutfile(messages == NULL ? stderr : messages);
	mandoc_msg_setmin(MANDOCERR_BASE);
	if (allow_include) {
		if (include_root == NULL)
			set_source_root_from_path(path);
		else
			set_source_root_directory(include_root);
	}
	mchars_alloc();
	parser = mparse_alloc(options, MANDOC_OS_OTHER, NULL);
	input = NULL;
	if (buffer == NULL) {
		fd = mparse_open(parser, path);
		if (fd == -1) {
			saved_errno = errno;
			document->error = copy_string(strerror(saved_errno));
			goto cleanup;
		}
	} else {
		input = tmpfile();
		if (input == NULL ||
		    fwrite(buffer, 1, length, input) != length ||
		    fflush(input) != 0 || fseek(input, 0, SEEK_SET) != 0) {
			saved_errno = errno;
			document->error = copy_string(saved_errno == 0 ?
			    "could not stage decompressed manual source" :
			    strerror(saved_errno));
			goto cleanup;
		}
		fd = fileno(input);
	}

	mparse_readfd(parser, fd, path);
	if (input == NULL)
		close(fd);
	else {
		fclose(input);
		input = NULL;
	}
	meta = mparse_result(parser);
	document->macroset = (int)meta->macroset;
	document->title = copy_string(meta->title);
	document->section = copy_string(meta->msec);
	document->volume = copy_string(meta->vol);
	document->os = copy_string(meta->os);
	document->arch = copy_string(meta->arch);
	document->name = copy_string(meta->name);
	document->date = copy_string(meta->date);
	document->alias_target = copy_string(meta->sodest);
	document->has_body = document_has_body(meta);
	document->root = copy_node(meta->first, 0);
	document->ok = document->root != NULL;
	if (!document->ok)
		document->error = copy_string("libmandoc produced no syntax tree");

cleanup:
	if (input != NULL)
		fclose(input);
	mandoc_msg_setinfilename(NULL);
	mandoc_msg_setoutfile(stderr);
	if (messages != NULL) {
		document->diagnostics = read_diagnostics(messages);
		fclose(messages);
	}
	mparse_free(parser);
	mchars_free();
	free(source_root);
	source_root = NULL;
	source_root_strict = 0;
	return document;
}

/* Resolve includes against the original source tree without changing cwd. */
int
mant_mandoc_source_open(const char *path, int flags, ...)
{
	char		*resolved;
	int		 fd, saved_errno;
	mode_t		 mode;
	va_list		 arguments;

	mode = 0;
	if (flags & O_CREAT) {
		va_start(arguments, flags);
		mode = (mode_t)va_arg(arguments, int);
		va_end(arguments);
	}
	if (*path == '/' || source_root == NULL)
		return openat(AT_FDCWD, path, flags, mode);
	resolved = malloc(strlen(source_root) + strlen(path) + 2);
	if (resolved == NULL) {
		errno = ENOMEM;
		return -1;
	}
	sprintf(resolved, "%s/%s", source_root, path);
	fd = openat(AT_FDCWD, resolved, flags, mode);
	saved_errno = errno;
	free(resolved);
	if (fd != -1)
		return fd;
	if (source_root_strict) {
		errno = saved_errno;
		return -1;
	}
	fd = openat(AT_FDCWD, path, flags, mode);
	if (fd == -1)
		errno = saved_errno;
	return fd;
}

void
mant_mandoc_document_free(struct mant_mandoc_document *document)
{
	if (document == NULL)
		return;
	free(document->error);
	free(document->diagnostics);
	free(document->title);
	free(document->section);
	free(document->volume);
	free(document->os);
	free(document->arch);
	free(document->name);
	free(document->date);
	free(document->alias_target);
	free_node(document->root);
	free(document);
}

static char *
copy_string(const char *source)
{
	char	*copy;
	size_t	 length;

	if (source == NULL)
		return NULL;
	length = strlen(source) + 1;
	copy = malloc(length);
	if (copy != NULL)
		memcpy(copy, source, length);
	return copy;
}

static void
set_source_root_from_path(const char *path)
{
	char	*last_slash, *directory_name;

	free(source_root);
	source_root_strict = 0;
	source_root = copy_string(path);
	if (source_root == NULL)
		return;
	last_slash = strrchr(source_root, '/');
	if (last_slash == NULL) {
		free(source_root);
		source_root = copy_string(".");
		return;
	}
	if (last_slash == source_root)
		last_slash[1] = '\0';
	else
		*last_slash = '\0';

	directory_name = strrchr(source_root, '/');
	directory_name = directory_name == NULL ? source_root : directory_name + 1;
	if (strncmp(directory_name, "man", 3) == 0 ||
	    strncmp(directory_name, "cat", 3) == 0) {
		last_slash = strrchr(source_root, '/');
		if (last_slash != NULL && last_slash != source_root)
			*last_slash = '\0';
	}
}

static void
set_source_root_directory(const char *directory)
{
	free(source_root);
	source_root_strict = 1;
	source_root = copy_string(directory);
}

static char *
read_diagnostics(FILE *stream)
{
	char	*buffer;
	long	 length;
	size_t	 count;

	if (fflush(stream) != 0 || fseek(stream, 0, SEEK_END) != 0)
		return NULL;
	length = ftell(stream);
	if (length <= 0 || fseek(stream, 0, SEEK_SET) != 0)
		return NULL;
	buffer = malloc((size_t)length + 1);
	if (buffer == NULL)
		return NULL;
	count = fread(buffer, 1, (size_t)length, stream);
	buffer[count] = '\0';
	return buffer;
}

static struct mant_mandoc_node *
copy_node(const struct roff_node *source, int depth)
{
	const struct roff_node		*source_child;
	struct mant_mandoc_node		*node, **next_child;

	if (source == NULL)
		return NULL;
	/* Stop descending past the depth cap so the owned tree stays finite. */
	if (depth >= MANT_MANDOC_MAX_COPY_DEPTH)
		return NULL;
	node = calloc(1, sizeof(*node));
	if (node == NULL)
		return NULL;
	node->kind = (int)source->type;
	if (source->type != ROFFT_ROOT && source->tok != TOKEN_NONE)
		node->macro = copy_string(roff_name[source->tok]);
	if (source->type == ROFFT_TEXT || source->type == ROFFT_COMMENT)
		node->text = copy_string(source->string);
	node->tag = copy_string(source->tag);
	node->line = source->line;
	node->column = source->pos + 1;
	copy_normalized_data(node, source);
	if (source->type == ROFFT_TBL)
		node->table_cells = copy_table_cells(source->span);
	else if (source->type == ROFFT_EQN)
		node->equation = copy_equation(source->eqn);
	if (source->flags & NODE_NOSRC)
		node->flags |= MANT_MANDOC_NODE_GENERATED;
	if (source->flags & NODE_EOS)
		node->flags |= MANT_MANDOC_NODE_SENTENCE_END;
	if (source->flags & NODE_NOPRT)
		node->flags |= MANT_MANDOC_NODE_NO_PRINT;
	if (source->flags & NODE_NOFILL)
		node->flags |= MANT_MANDOC_NODE_NO_FILL;
	if (source->flags & NODE_ID)
		node->flags |= MANT_MANDOC_NODE_DEEP_LINK_TARGET;
	if (source->flags & NODE_HREF)
		node->flags |= MANT_MANDOC_NODE_PERMALINK;
	if (source->flags & NODE_LINE)
		node->flags |= MANT_MANDOC_NODE_LINE_START;

	next_child = &node->child;
	for (source_child = source->child; source_child != NULL;
	    source_child = source_child->next) {
		*next_child = copy_node(source_child, depth + 1);
		if (*next_child == NULL)
			break;
		next_child = &(*next_child)->next;
	}
	return node;
}

static void
free_node(struct mant_mandoc_node *node)
{
	struct mant_mandoc_node	*next;

	while (node != NULL) {
		next = node->next;
		free_node(node->child);
		free(node->macro);
		free(node->text);
		free(node->tag);
		free(node->offset);
		free(node->equation);
		free_table_cells(node->table_cells);
		free(node);
		node = next;
	}
}

static struct mant_mandoc_table_cell *
copy_table_cells(const struct tbl_span *span)
{
	const struct tbl_dat		*source;
	struct mant_mandoc_table_cell	*first, **next;

	if (span == NULL || span->pos != TBL_SPAN_DATA)
		return NULL;
	first = NULL;
	next = &first;
	for (source = span->first; source != NULL; source = source->next) {
		*next = calloc(1, sizeof(**next));
		if (*next == NULL)
			break;
		(*next)->text = copy_string(source->string);
		(*next)->column_span = source->hspans < 0 ? 1U :
		    (unsigned int)source->hspans + 1U;
		(*next)->row_span = source->vspans < 0 ? 1U :
		    (unsigned int)source->vspans + 1U;
		if (source->layout != NULL &&
		    source->layout->pos == TBL_CELL_CENTRE)
			(*next)->alignment = 1;
		else if (source->layout != NULL &&
		    (source->layout->pos == TBL_CELL_RIGHT ||
		     source->layout->pos == TBL_CELL_NUMBER))
			(*next)->alignment = 2;
		next = &(*next)->next;
	}
	return first;
}

static void
free_table_cells(struct mant_mandoc_table_cell *cell)
{
	struct mant_mandoc_table_cell	*next;

	while (cell != NULL) {
		next = cell->next;
		free(cell->text);
		free(cell);
		cell = next;
	}
}

struct text_buffer {
	char	*data;
	size_t	 length;
	size_t	 capacity;
};

static int append_text(struct text_buffer *, const char *);
static int append_equation(struct text_buffer *, const struct eqn_box *);

static char *
copy_equation(const struct eqn_box *box)
{
	struct text_buffer	buffer;

	memset(&buffer, 0, sizeof(buffer));
	if (!append_equation(&buffer, box)) {
		free(buffer.data);
		return NULL;
	}
	return buffer.data;
}

static int
append_equation(struct text_buffer *buffer, const struct eqn_box *box)
{
	const struct eqn_box	*child;

	if (box == NULL)
		return append_text(buffer, "");
	if (box->pos == EQNPOS_SQRT && !append_text(buffer, "sqrt("))
		return 0;
	if (!append_text(buffer, box->left) || !append_text(buffer, box->text))
		return 0;
	for (child = box->first; child != NULL; child = child->next) {
		if (child != box->first && !append_text(buffer, " "))
			return 0;
		if (!append_equation(buffer, child))
			return 0;
	}
	if (!append_text(buffer, box->right))
		return 0;
	if (box->pos == EQNPOS_SQRT)
		return append_text(buffer, ")");
	if (box->pos == EQNPOS_OVER)
		return append_text(buffer, " / ");
	return 1;
}

static int
append_text(struct text_buffer *buffer, const char *text)
{
	size_t	length, capacity;
	char	*data;

	if (text == NULL)
		return 1;
	length = strlen(text);
	if (buffer->length + length + 1 > buffer->capacity) {
		capacity = buffer->capacity == 0 ? 64 : buffer->capacity;
		while (capacity < buffer->length + length + 1)
			capacity *= 2;
		data = realloc(buffer->data, capacity);
		if (data == NULL)
			return 0;
		buffer->data = data;
		buffer->capacity = capacity;
	}
	memcpy(buffer->data + buffer->length, text, length);
	buffer->length += length;
	buffer->data[buffer->length] = '\0';
	return 1;
}

static void
copy_normalized_data(struct mant_mandoc_node *node,
    const struct roff_node *source)
{
	if (source->norm == NULL)
		return;
	if (source->tok == MDOC_Bl) {
		node->compact = source->norm->Bl.comp;
		node->offset = copy_string(source->norm->Bl.offs);
		switch (source->norm->Bl.type) {
		case LIST_bullet:
		case LIST_dash:
		case LIST_hyphen:
			node->list_kind = MANT_MANDOC_LIST_BULLET;
			break;
		case LIST_enum:
			node->list_kind = MANT_MANDOC_LIST_ORDERED;
			break;
		case LIST_diag:
		case LIST_hang:
		case LIST_inset:
		case LIST_ohang:
		case LIST_tag:
			node->list_kind = MANT_MANDOC_LIST_DEFINITION;
			break;
		case LIST_column:
			node->list_kind = MANT_MANDOC_LIST_COLUMN;
			break;
		case LIST_item:
			node->list_kind = MANT_MANDOC_LIST_PLAIN;
			break;
		case LIST__NONE:
		case LIST_MAX:
			break;
		}
	} else if (source->tok == MDOC_Bd) {
		node->compact = source->norm->Bd.comp;
		node->offset = copy_string(source->norm->Bd.offs);
		switch (source->norm->Bd.type) {
		case DISP_unfilled:
		case DISP_literal:
			node->display_kind = MANT_MANDOC_DISPLAY_LITERAL;
			break;
		case DISP_centered:
		case DISP_ragged:
		case DISP_filled:
			node->display_kind = MANT_MANDOC_DISPLAY_FILLED;
			break;
		case DISP__NONE:
			break;
		}
	}
}

static int
document_has_body(const struct roff_meta *meta)
{
	const struct roff_node	*node;

	if (meta == NULL || meta->first == NULL)
		return 0;
	for (node = meta->first->child; node != NULL; node = node->next)
		if (node->type != ROFFT_COMMENT)
			return 1;
	return 0;
}

#define DOCUMENT_INT_ACCESSOR(name, field) \
	int name(const struct mant_mandoc_document *document) \
	{ return document == NULL ? 0 : document->field; }

#define DOCUMENT_STRING_ACCESSOR(name, field) \
	const char *name(const struct mant_mandoc_document *document) \
	{ return document == NULL ? NULL : document->field; }

DOCUMENT_INT_ACCESSOR(mant_mandoc_document_ok, ok)
DOCUMENT_INT_ACCESSOR(mant_mandoc_document_macroset, macroset)
DOCUMENT_INT_ACCESSOR(mant_mandoc_document_has_body, has_body)
DOCUMENT_STRING_ACCESSOR(mant_mandoc_document_error, error)
DOCUMENT_STRING_ACCESSOR(mant_mandoc_document_diagnostics, diagnostics)
DOCUMENT_STRING_ACCESSOR(mant_mandoc_document_title, title)
DOCUMENT_STRING_ACCESSOR(mant_mandoc_document_section, section)
DOCUMENT_STRING_ACCESSOR(mant_mandoc_document_volume, volume)
DOCUMENT_STRING_ACCESSOR(mant_mandoc_document_os, os)
DOCUMENT_STRING_ACCESSOR(mant_mandoc_document_arch, arch)
DOCUMENT_STRING_ACCESSOR(mant_mandoc_document_name, name)
DOCUMENT_STRING_ACCESSOR(mant_mandoc_document_date, date)
DOCUMENT_STRING_ACCESSOR(mant_mandoc_document_alias_target, alias_target)

const struct mant_mandoc_node *
mant_mandoc_document_root(const struct mant_mandoc_document *document)
{
	return document == NULL ? NULL : document->root;
}

int
mant_mandoc_node_kind(const struct mant_mandoc_node *node)
{
	return node == NULL ? MANT_MANDOC_ROOT : node->kind;
}

const char *
mant_mandoc_node_macro(const struct mant_mandoc_node *node)
{
	return node == NULL ? NULL : node->macro;
}

const char *
mant_mandoc_node_text(const struct mant_mandoc_node *node)
{
	return node == NULL ? NULL : node->text;
}

const char *
mant_mandoc_node_tag(const struct mant_mandoc_node *node)
{
	return node == NULL ? NULL : node->tag;
}

int
mant_mandoc_node_line(const struct mant_mandoc_node *node)
{
	return node == NULL ? 0 : node->line;
}

int
mant_mandoc_node_column(const struct mant_mandoc_node *node)
{
	return node == NULL ? 0 : node->column;
}

unsigned int
mant_mandoc_node_flags(const struct mant_mandoc_node *node)
{
	return node == NULL ? 0 : node->flags;
}

int
mant_mandoc_node_list_kind(const struct mant_mandoc_node *node)
{
	return node == NULL ? MANT_MANDOC_LIST_NONE : node->list_kind;
}

int
mant_mandoc_node_display_kind(const struct mant_mandoc_node *node)
{
	return node == NULL ? MANT_MANDOC_DISPLAY_NONE : node->display_kind;
}

int
mant_mandoc_node_compact(const struct mant_mandoc_node *node)
{
	return node == NULL ? 0 : node->compact;
}

const char *
mant_mandoc_node_offset(const struct mant_mandoc_node *node)
{
	return node == NULL ? NULL : node->offset;
}

const char *
mant_mandoc_node_equation(const struct mant_mandoc_node *node)
{
	return node == NULL ? NULL : node->equation;
}

const struct mant_mandoc_table_cell *
mant_mandoc_node_table_cells(const struct mant_mandoc_node *node)
{
	return node == NULL ? NULL : node->table_cells;
}

const char *
mant_mandoc_table_cell_text(const struct mant_mandoc_table_cell *cell)
{
	return cell == NULL ? NULL : cell->text;
}

unsigned int
mant_mandoc_table_cell_column_span(const struct mant_mandoc_table_cell *cell)
{
	return cell == NULL ? 1U : cell->column_span;
}

unsigned int
mant_mandoc_table_cell_row_span(const struct mant_mandoc_table_cell *cell)
{
	return cell == NULL ? 1U : cell->row_span;
}

int
mant_mandoc_table_cell_alignment(const struct mant_mandoc_table_cell *cell)
{
	return cell == NULL ? 0 : cell->alignment;
}

const struct mant_mandoc_table_cell *
mant_mandoc_table_cell_next(const struct mant_mandoc_table_cell *cell)
{
	return cell == NULL ? NULL : cell->next;
}

const struct mant_mandoc_node *
mant_mandoc_node_child(const struct mant_mandoc_node *node)
{
	return node == NULL ? NULL : node->child;
}

const struct mant_mandoc_node *
mant_mandoc_node_next(const struct mant_mandoc_node *node)
{
	return node == NULL ? NULL : node->next;
}
