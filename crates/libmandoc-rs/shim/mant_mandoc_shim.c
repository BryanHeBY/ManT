/*
 * Owned compatibility layer for the pinned libmandoc parser.
 *
 * libmandoc's syntax tree and diagnostic writer are private implementation
 * details with session-local lifetime. A parse result retains the parser only
 * while Rust copies borrowed snapshots into its owned public tree.
 */
#include "config.h"
#include "mant_thread_local.h"

#include <errno.h>
#include <ctype.h>
#ifndef MANDOC_MEMORY_ONLY
#include <fcntl.h>
#endif
#include <stdio.h>
#include <stdarg.h>
#include <stdlib.h>
#include <string.h>
#ifndef MANDOC_MEMORY_ONLY
#include <unistd.h>
#endif
#if HAVE_PROGNAME
#include <pthread.h>
#endif

#include "mandoc.h"
#include "mdoc.h"
#include "eqn.h"
#include "roff.h"
#include "roff_int.h"
#include "tbl.h"
#include "out.h"
#include "mandoc_parse.h"

#include "mant_mandoc_shim.h"
#ifdef _MSC_VER
#define MANT_VIEW_ALIGNOF(type) __alignof(type)
#else
#define MANT_VIEW_ALIGNOF(type) _Alignof(type)
#endif
#include "mant_mandoc_shim/table_measure.inc"
#ifdef MANT_MANDOC_RENDER
#include "main.h"
#include "manconf.h"
#include "mant_mandoc_output.h"
#include "term_tag.h"
#endif

struct mant_mandoc_document {
	int			 ok;
	char			*error;
	char			*diagnostics;
	int			 escape_depth_truncated;
	struct mparse		*parser;
	const struct roff_meta	*meta;
#ifdef MANT_MANDOC_RENDER
	struct mant_mandoc_output *output;
	int			 render_status;
#endif
};

/*
 * roff_escape.c reports the actual controlled omission, including ln == 0
 * renderer calls. Keep the fact on this synchronous result, never on a
 * diagnostic number or message. The pointer is cleared on every cleanup
 * path before its owning result can be released.
 */
MANT_THREAD_LOCAL struct mant_mandoc_document *active_escape_document;

void
mant_mandoc_note_escape_depth_limit(void)
{
	if (active_escape_document != NULL)
		active_escape_document->escape_depth_truncated = 1;
}

int
mant_mandoc_document_escape_depth_truncated(
    const struct mant_mandoc_document *document)
{
	return document != NULL && document->escape_depth_truncated;
}

/*
 * Ask the pinned parser's request table rather than duplicating the roff
 * request vocabulary in Rust. tbl handles these requests itself and only
 * passes operands of unknown or high-level man/mdoc macros to tbl_read().
 *
 * The table is local to this call, so this helper introduces neither parser
 * session state nor a cross-thread cache.
 */
int
mant_mandoc_is_native_roff_request(const char *name, size_t length)
{
	struct ohash	*requests;
	int		 result;

	if (name == NULL || length == 0)
		return 0;
	requests = roffhash_alloc(ROFF_br, ROFF_RENAMED);
	result = roffhash_find(requests, name, length) != TOKEN_NONE;
	roffhash_free(requests);
	return result;
}

MANT_THREAD_LOCAL const struct mant_mandoc_source *bundle_sources;
MANT_THREAD_LOCAL size_t bundle_source_count;
MANT_THREAD_LOCAL mant_mandoc_source_resolver source_resolver;
MANT_THREAD_LOCAL void *source_resolver_context;

#ifndef MANDOC_MEMORY_ONLY
MANT_THREAD_LOCAL char *source_root;
MANT_THREAD_LOCAL int source_root_strict;
/* Whether the active parse may open anything beyond its top-level source. */
MANT_THREAD_LOCAL int source_includes_allowed;
/* Absolute top-level file path that parse_file itself is allowed to open. */
MANT_THREAD_LOCAL char *source_path;
MANT_THREAD_LOCAL int source_path_pending;
/*
 * Directory that literally contains the parsed file, kept unstripped.
 *
 * source_root drops a trailing man#/cat# component so a redirect written as
 * `.so man1/foo.1` resolves against the manual hierarchy root. Some stubs
 * instead write a bare same-directory target such as `.so last.1`, which must
 * resolve next to the stub (man1/last.1). Keeping the original directory lets
 * the include resolver try both bases the way man(1) does.
 */
MANT_THREAD_LOCAL char *source_dir;
#endif

/*
 * BSD-derived targets expose a process-global program-name slot.  All parser
 * callers use the same immutable label, so initialize it once instead of
 * racing to assign it on every parse.  Linux and Windows compile the vendored
 * TLS compatibility implementation and initialize their own thread-local
 * slots below.
 */
#if HAVE_PROGNAME
static pthread_once_t mant_progname_once = PTHREAD_ONCE_INIT;

static void
set_mant_progname(void)
{
	setprogname("mant");
}
#endif

/*
 * Maximum recursion depth for any tree copied or flattened out of libmandoc.
 *
 * libmandoc bounds .so include depth but not block/inline nesting, so a
 * pathological or hostile page (thousands of nested .RS or .Bl) yields a tree
 * deep enough to overflow the stack when it is copied, lowered, or dropped.
 * Rust caps its borrowed node walk so the resulting owned tree stays finite.
 *
 * The independently nested eqn box tree is copied and bounded in the
 * Rust owned transfer; this shim only lends one box at a time.
 *
 * These transfer caps do not protect native construction or rendering.
 * The vendor construction guard rejects syntax parent chains beyond 512
 * levels; owned syntax/equation transfer truncates at 256 with diagnostics;
 * reference rendering rejects trees beyond its separate 256-level preflight.
 * Native syntax/equation cleanup is iterative, including rejected trees.
 * Keep these lifecycle boundaries and their existing root-counting rules
 * separate: none of them substitutes for the others.
 */
#define MANT_MANDOC_MAX_COPY_DEPTH 256

static char *copy_string(const char *);
static struct mant_mandoc_document *parse_input(const char *,
    const unsigned char *, size_t, const char *, int, int, const char *,
    int, size_t, int, size_t, mant_mandoc_source_resolver, void *, int);
static struct mant_mandoc_document *parse_bundle_mode(const char *,
    const struct mant_mandoc_source *, size_t, int, const char *, int);
static char *read_diagnostics(FILE *);
static const struct mant_mandoc_source *find_bundle_source(const char *);
static char *normalize_requested_bundle_path(const char *);
static int is_safe_bundle_path(const char *);
static unsigned int snapshot_node_flags(const struct roff_node *);
static int document_has_body(const struct roff_meta *);
#ifndef MANDOC_MEMORY_ONLY
static void set_source_root_from_path(const char *);
static void set_source_root_directory(const char *, const char *);
static int open_under_base(const char *, const char *, int, mode_t);
static int open_beneath_root(const char *, int, mode_t);
static int open_beside_source(const char *, int, mode_t);
static int is_safe_relative_path(const char *);
#endif
static void snapshot_normalized_data(struct mant_mandoc_node_view *,
    const struct roff_node *);
#ifdef MANT_MANDOC_RENDER
static int render_document(struct mant_mandoc_document *,
    const struct roff_meta *, int, size_t, int, size_t);
#endif

#include "mant_mandoc_shim/session.inc"

#include "mant_mandoc_shim/render_dispatch.inc"
#include "mant_mandoc_shim/sources.inc"
void
mant_mandoc_document_free(struct mant_mandoc_document *document)
{
	if (document == NULL)
		return;
	free(document->error);
	free(document->diagnostics);
	if (document->parser != NULL)
		mparse_free(document->parser);
#ifdef MANT_MANDOC_RENDER
	mant_mandoc_output_free(document->output);
#endif
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

#ifndef MANDOC_MEMORY_ONLY
static void
set_source_root_from_path(const char *path)
{
	char	*last_slash, *directory_name;

	free(source_root);
	free(source_dir);
	source_dir = NULL;
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

	/* Remember the literal stub directory before stripping man#/cat#. */
	source_dir = copy_string(source_root);

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
set_source_root_directory(const char *directory, const char *path)
{
	char	*last_slash;
	size_t	 root_length;

	free(source_root);
	free(source_dir);
	source_dir = NULL;
	source_root_strict = 1;
	source_root = copy_string(directory);
	if (source_root == NULL || path == NULL)
		return;

	/* Bare redirects may resolve beside the source, but only inside root. */
	root_length = strlen(source_root);
	if (root_length == 0)
		return;
	while (root_length > 1 && source_root[root_length - 1] == '/')
		source_root[--root_length] = '\0';
	if (strncmp(path, source_root, root_length) != 0 ||
	    (root_length != 1 && path[root_length] != '/'))
		return;
	source_dir = copy_string(path);
	if (source_dir == NULL)
		return;
	last_slash = strrchr(source_dir, '/');
	if (last_slash == NULL) {
		free(source_dir);
		source_dir = NULL;
	} else if (last_slash == source_dir) {
		last_slash[1] = '\0';
	} else {
		*last_slash = '\0';
	}
}
#endif

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

#include "mant_mandoc_shim/snapshot_normalized.inc"
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

int
mant_mandoc_document_ok(const struct mant_mandoc_document *document)
{
	return document == NULL ? 0 : document->ok;
}

const char *
mant_mandoc_document_error(const struct mant_mandoc_document *document)
{
	return document == NULL ? NULL : document->error;
}

const char *
mant_mandoc_document_diagnostics(const struct mant_mandoc_document *document)
{
	return document == NULL ? NULL : document->diagnostics;
}

int
mant_mandoc_document_macroset(const struct mant_mandoc_document *document)
{
	return document == NULL || document->meta == NULL ? 0 :
	    (int)document->meta->macroset;
}

#define DOCUMENT_META_STRING_ACCESSOR(name, field) \
	const char *name(const struct mant_mandoc_document *document) \
	{ return document == NULL || document->meta == NULL ? NULL : \
	    document->meta->field; }

DOCUMENT_META_STRING_ACCESSOR(mant_mandoc_document_title, title)
DOCUMENT_META_STRING_ACCESSOR(mant_mandoc_document_section, msec)
DOCUMENT_META_STRING_ACCESSOR(mant_mandoc_document_volume, vol)
DOCUMENT_META_STRING_ACCESSOR(mant_mandoc_document_os, os)
DOCUMENT_META_STRING_ACCESSOR(mant_mandoc_document_arch, arch)
DOCUMENT_META_STRING_ACCESSOR(mant_mandoc_document_name, name)
DOCUMENT_META_STRING_ACCESSOR(mant_mandoc_document_date, date)
DOCUMENT_META_STRING_ACCESSOR(mant_mandoc_document_alias_target, sodest)

int
mant_mandoc_document_has_body(const struct mant_mandoc_document *document)
{
	return document == NULL ? 0 : document_has_body(document->meta);
}

#include "mant_mandoc_shim/abi.inc"
const struct mant_mandoc_node *
mant_mandoc_document_root(const struct mant_mandoc_document *document)
{
	return document == NULL ? NULL :
	    (const struct mant_mandoc_node *)(document->meta == NULL ? NULL :
	    document->meta->first);
}

#include "mant_mandoc_shim/snapshots.inc"
#ifdef MANT_MANDOC_RENDER
const unsigned char *
mant_mandoc_document_output(const struct mant_mandoc_document *document)
{
	return document == NULL ? NULL :
	    mant_mandoc_output_data(document->output);
}

size_t
mant_mandoc_document_output_length(
    const struct mant_mandoc_document *document)
{
	return document == NULL ? 0 :
	    mant_mandoc_output_length(document->output);
}

int
mant_mandoc_document_render_status(
    const struct mant_mandoc_document *document)
{
	return document == NULL ? 2 : document->render_status;
}
#endif
