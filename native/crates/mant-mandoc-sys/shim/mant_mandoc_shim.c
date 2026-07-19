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
#include "roff.h"
#include "mandoc_parse.h"

#include "mant_mandoc_shim.h"

struct mant_mandoc_node {
	int			 kind;
	char			*macro;
	char			*text;
	int			 line;
	int			 column;
	unsigned int		 flags;
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

static char *copy_string(const char *);
static char *read_diagnostics(FILE *);
static struct mant_mandoc_node *copy_node(const struct roff_node *);
static void free_node(struct mant_mandoc_node *);
static int document_has_body(const struct roff_meta *);
static void set_source_root(const char *);

struct mant_mandoc_document *
mant_mandoc_parse_file(const char *path, int allow_include)
{
	struct mant_mandoc_document	*document;
	struct mparse			*parser;
	struct roff_meta			*meta;
	FILE				*messages;
	int				 fd, options, saved_errno;

	document = calloc(1, sizeof(*document));
	if (document == NULL)
		return NULL;
	if (path == NULL || *path == '\0') {
		document->error = copy_string("manual source path is empty");
		return document;
	}

	options = MPARSE_UTF8 | MPARSE_LATIN1 | MPARSE_VALIDATE | MPARSE_COMMENT;
	if (allow_include)
		options |= MPARSE_SO;

	messages = tmpfile();
	setprogname("mant");
	mandoc_msg_setoutfile(messages == NULL ? stderr : messages);
	mandoc_msg_setmin(MANDOCERR_BASE);
	set_source_root(path);
	mchars_alloc();
	parser = mparse_alloc(options, MANDOC_OS_OTHER, NULL);
	fd = mparse_open(parser, path);
	if (fd == -1) {
		saved_errno = errno;
		document->error = copy_string(strerror(saved_errno));
		goto cleanup;
	}

	mparse_readfd(parser, fd, path);
	close(fd);
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
	document->root = copy_node(meta->first);
	document->ok = document->root != NULL;
	if (!document->ok)
		document->error = copy_string("libmandoc produced no syntax tree");

cleanup:
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
set_source_root(const char *path)
{
	char	*last_slash, *directory_name;

	free(source_root);
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
copy_node(const struct roff_node *source)
{
	const struct roff_node		*source_child;
	struct mant_mandoc_node		*node, **next_child;

	if (source == NULL)
		return NULL;
	node = calloc(1, sizeof(*node));
	if (node == NULL)
		return NULL;
	node->kind = (int)source->type;
	if (source->type != ROFFT_ROOT && source->tok != TOKEN_NONE)
		node->macro = copy_string(roff_name[source->tok]);
	if (source->type == ROFFT_TEXT || source->type == ROFFT_COMMENT)
		node->text = copy_string(source->string);
	node->line = source->line;
	node->column = source->pos + 1;
	if (source->flags & NODE_NOSRC)
		node->flags |= MANT_MANDOC_NODE_GENERATED;
	if (source->flags & NODE_EOS)
		node->flags |= MANT_MANDOC_NODE_SENTENCE_END;
	if (source->flags & NODE_NOPRT)
		node->flags |= MANT_MANDOC_NODE_NO_PRINT;
	if (source->flags & NODE_NOFILL)
		node->flags |= MANT_MANDOC_NODE_NO_FILL;

	next_child = &node->child;
	for (source_child = source->child; source_child != NULL;
	    source_child = source_child->next) {
		*next_child = copy_node(source_child);
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
		free(node);
		node = next;
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
