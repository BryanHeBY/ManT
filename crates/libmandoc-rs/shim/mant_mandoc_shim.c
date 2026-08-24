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
#include "tbl.h"
#include "mandoc_parse.h"

#include "mant_mandoc_shim.h"
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
	struct mparse		*parser;
	const struct roff_meta	*meta;
	char			*equation;
	int			 equation_truncated;
#ifdef MANT_MANDOC_RENDER
	struct mant_mandoc_output *output;
	int			 render_status;
#endif
};

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
 * It does not, however, bound sub-structures that hang off a node and are
 * walked from their own source tree: the eqn box tree is copied by descending
 * eqn_box->first independently of node depth, so copy_equation applies this
 * same cap itself. Any future payload with its own nested source structure
 * must do likewise -- one cap on the node walk is not automatically enough.
 *
 * libmandoc's own parser is iterative and does not overflow on deep input, so
 * no pre-parse depth check is needed; the caps on our recursive walks suffice.
 * Real manuals nest only a handful of levels, far below this limit.
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
static char *copy_equation(const struct eqn_box *, int *);
#ifdef MANT_MANDOC_RENDER
static int render_document(struct mant_mandoc_document *,
    const struct roff_meta *, int, size_t, int, size_t);
#endif

struct mant_mandoc_document *
mant_mandoc_parse_file(const char *path, const char *include_root,
    int allow_include, int input_format, const char *operating_system)
{
	return parse_input(path, NULL, 0, include_root, allow_include,
	    input_format, operating_system, 0, 0, 0, 0, NULL, NULL, 1);
}

struct mant_mandoc_document *
mant_mandoc_parse_buffer(const char *path, const unsigned char *buffer,
    size_t length, const char *include_root, int allow_include,
    int input_format, const char *operating_system,
    mant_mandoc_source_resolver resolver, void *resolver_context)
{
	return parse_input(path, buffer, length, include_root, allow_include,
	    input_format, operating_system, 0, 0, 0, 0, resolver,
	    resolver_context, 1);
}

struct mant_mandoc_document *
mant_mandoc_parse_bundle(const char *root,
    const struct mant_mandoc_source *sources, size_t source_count,
    int input_format, const char *operating_system)
{
	return parse_bundle_mode(root, sources, source_count, input_format,
	    operating_system, 1);
}

static struct mant_mandoc_document *
parse_bundle_mode(const char *root,
    const struct mant_mandoc_source *sources, size_t source_count,
    int input_format, const char *operating_system, int retain_parser)
{
	const struct mant_mandoc_source	*source;
	struct mant_mandoc_document	*document;

	if (sources == NULL || source_count == 0) {
		document = calloc(1, sizeof(*document));
		if (document != NULL)
			document->error = copy_string("source bundle is empty");
		return document;
	}
	if (bundle_sources != NULL) {
		document = calloc(1, sizeof(*document));
		if (document != NULL)
			document->error = copy_string(
			    "recursive libmandoc bundle entry is unsupported");
		return document;
	}
	bundle_sources = sources;
	bundle_source_count = source_count;
	source = find_bundle_source(root);
	if (source == NULL) {
		document = calloc(1, sizeof(*document));
		if (document != NULL)
			document->error = copy_string(
			    "source bundle does not contain the requested root");
	} else
		document = parse_input(root, source->data, source->length,
		    NULL, 1, input_format, operating_system, 0, 0, 0, 0, NULL, NULL,
		    retain_parser);
	bundle_sources = NULL;
	bundle_source_count = 0;
	return document;
}

static struct mant_mandoc_document *
parse_input(const char *path, const unsigned char *buffer, size_t length,
    const char *include_root, int allow_include, int input_format,
    const char *operating_system, int render_format, size_t render_width,
    int html_fragment, size_t output_limit,
    mant_mandoc_source_resolver resolver, void *resolver_context,
    int retain_parser)
{
	struct mant_mandoc_document	*document;
	struct mparse			*parser;
	struct roff_meta			*meta;
	FILE				*messages;
	int				 options;
#ifndef MANDOC_MEMORY_ONLY
	int				 fd, saved_errno;
#endif

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
#ifdef MANDOC_MEMORY_ONLY
	(void)include_root;
	if (buffer == NULL) {
		document->error = copy_string(
		    "memory-only libmandoc requires caller-owned source bytes");
		return document;
	}
	if (allow_include && bundle_sources == NULL && resolver == NULL) {
		document->error = copy_string(
		    "file inclusion is unavailable in memory-only libmandoc");
		return document;
	}
#endif
	if (resolver != NULL && source_resolver != NULL) {
		document->error = copy_string(
		    "recursive libmandoc source resolver entry is unsupported");
		return document;
	}

	options = MPARSE_UTF8 | MPARSE_LATIN1 | MPARSE_VALIDATE | MPARSE_COMMENT;
	switch (input_format) {
	case 0:
		break;
	case 1:
		options |= MPARSE_MAN;
		break;
	case 2:
		options |= MPARSE_MDOC;
		break;
	default:
		document->error = copy_string("unknown manual input format");
		return document;
	}
	if (allow_include)
		options |= MPARSE_SO;

	messages = tmpfile();
	if (messages == NULL) {
		document->error = copy_string(
		    "could not create private diagnostic capture file");
		return document;
	}
#if HAVE_PROGNAME
	pthread_once(&mant_progname_once, set_mant_progname);
#else
	setprogname("mant");
#endif
	mandoc_msg_setoutfile(messages);
	mandoc_msg_setmin(MANDOCERR_BASE);
	source_resolver = resolver;
	source_resolver_context = resolver_context;
#ifndef MANDOC_MEMORY_ONLY
	source_includes_allowed = allow_include;
	free(source_path);
	source_path = buffer == NULL ? copy_string(path) : NULL;
	source_path_pending = source_path != NULL;
	if (allow_include && bundle_sources == NULL) {
		if (include_root == NULL)
			set_source_root_from_path(path);
		else
			set_source_root_directory(include_root, path);
	}
#endif
	mchars_alloc();
	parser = mparse_alloc(options, MANDOC_OS_OTHER, operating_system);
#ifdef MANDOC_MEMORY_ONLY
	mparse_readmem(parser, buffer, length, path);
#else
	if (buffer == NULL) {
		fd = mparse_open(parser, path);
		if (fd == -1) {
			saved_errno = errno;
			document->error = copy_string(strerror(saved_errno));
			goto cleanup;
		}
		mparse_readfd(parser, fd, path);
		close(fd);
	} else
		mparse_readmem(parser, buffer, length, path);
#endif
	meta = mparse_result(parser);
#ifdef MANT_MANDOC_RENDER
	if (render_format != 0)
		document->ok = render_document(document, meta, render_format,
		    render_width, html_fragment, output_limit);
	else
#else
	(void)render_format;
	(void)render_width;
	(void)html_fragment;
	(void)output_limit;
#endif
	{
		if (!retain_parser) {
			document->error = copy_string(
			    "syntax tree requested without retaining the parser");
		} else {
			document->parser = parser;
			document->meta = meta;
			parser = NULL;
			document->ok = meta->first != NULL;
			if (!document->ok)
				document->error = copy_string(
				    "libmandoc produced no syntax tree");
		}
	}

#ifndef MANDOC_MEMORY_ONLY
cleanup:
#endif
	mandoc_msg_setinfilename(NULL);
	mandoc_msg_setoutfile(stderr);
	document->diagnostics = read_diagnostics(messages);
	fclose(messages);
	if (parser != NULL)
		mparse_free(parser);
	mchars_free();
	source_resolver = NULL;
	source_resolver_context = NULL;
#ifndef MANDOC_MEMORY_ONLY
	free(source_root);
	source_root = NULL;
	free(source_dir);
	source_dir = NULL;
	free(source_path);
	source_path = NULL;
	source_path_pending = 0;
	source_root_strict = 0;
	source_includes_allowed = 0;
#endif
	return document;
}

#ifdef MANT_MANDOC_RENDER
struct mant_mandoc_document *
mant_mandoc_render_file(const char *path, const char *include_root,
    int allow_include, int input_format, const char *operating_system,
    int render_format, size_t render_width, int html_fragment,
    size_t output_limit)
{
	return parse_input(path, NULL, 0, include_root, allow_include,
	    input_format, operating_system, render_format, render_width,
	    html_fragment, output_limit, NULL, NULL, 0);
}

struct mant_mandoc_document *
mant_mandoc_render_buffer(const char *path, const unsigned char *buffer,
    size_t length, const char *include_root, int allow_include,
    int input_format, const char *operating_system, int render_format,
    size_t render_width, int html_fragment, size_t output_limit,
    mant_mandoc_source_resolver resolver, void *resolver_context)
{
	return parse_input(path, buffer, length, include_root, allow_include,
	    input_format, operating_system, render_format, render_width,
	    html_fragment, output_limit, resolver, resolver_context, 0);
}

struct mant_mandoc_document *
mant_mandoc_render_bundle(const char *root,
    const struct mant_mandoc_source *sources, size_t source_count,
    int input_format, const char *operating_system, int render_format,
    size_t render_width, int html_fragment, size_t output_limit)
{
	const struct mant_mandoc_source	*source;
	struct mant_mandoc_document	*document;

	if (sources == NULL || source_count == 0)
		return mant_mandoc_parse_bundle(root, sources, source_count,
		    input_format, operating_system);
	if (bundle_sources != NULL) {
		document = calloc(1, sizeof(*document));
		if (document != NULL)
			document->error = copy_string(
			    "recursive libmandoc bundle entry is unsupported");
		return document;
	}
	bundle_sources = sources;
	bundle_source_count = source_count;
	source = find_bundle_source(root);
	if (source == NULL) {
		document = calloc(1, sizeof(*document));
		if (document != NULL)
			document->error = copy_string(
			    "source bundle does not contain the requested root");
	} else
		document = parse_input(root, source->data, source->length,
		    NULL, 1, input_format, operating_system, render_format,
		    render_width, html_fragment, output_limit, NULL, NULL, 0);
	bundle_sources = NULL;
	bundle_source_count = 0;
	return document;
}

static int
render_document(struct mant_mandoc_document *document,
    const struct roff_meta *meta, int format, size_t width,
    int html_fragment, size_t output_limit)
{
	struct manoutput options;
	void		*renderer;
	int		 status;

	document->output = mant_mandoc_output_alloc(output_limit);
	if (document->output == NULL ||
	    !mant_mandoc_output_begin(document->output)) {
		document->render_status = 2;
		document->error = copy_string(
		    "could not initialize isolated renderer output");
		return 0;
	}
	memset(&options, 0, sizeof(options));
	options.width = width;
	options.fragment = html_fragment;
	switch (format) {
	case 1:
		renderer = ascii_alloc(&options);
		if (meta->macroset == MACROSET_MDOC)
			terminal_mdoc(renderer, meta);
		else
			terminal_man(renderer, meta);
		ascii_free(renderer);
		break;
	case 2:
		renderer = html_alloc(&options);
		if (meta->macroset == MACROSET_MDOC)
			html_mdoc(renderer, meta);
		else
			html_man(renderer, meta);
		html_free(renderer);
		break;
	case 3:
		renderer = utf8_alloc(&options);
		if (meta->macroset == MACROSET_MDOC)
			terminal_mdoc(renderer, meta);
		else
			terminal_man(renderer, meta);
		ascii_free(renderer);
		break;
	default:
		mant_mandoc_output_end();
		document->render_status = 3;
		document->error = copy_string("unknown renderer output format");
		return 0;
	}
	mant_mandoc_output_end();
	status = mant_mandoc_output_status(document->output);
	if (status == 1)
		document->render_status = 1;
	if (status == 1)
		document->error = copy_string(
		    "rendered output exceeds the configured byte limit");
	else if (status != 0) {
		document->render_status = 2;
		document->error = copy_string(
		    "could not allocate isolated renderer output");
	}
	return status == 0;
}

/* Embedded rendering never creates pager tag files. */
void
term_tag_write(struct roff_node *node, size_t line)
{
	(void)node;
	(void)line;
}
#endif

int
mant_mandoc_read_bundle(struct mparse *parser, const char *requested_path)
{
	const struct mant_mandoc_source	*source;
	struct mant_mandoc_resolved_source resolved;
	const char			*current, *slash;
	unsigned char			*resolved_data;
	char				*resolved_path;
	char				*path;
	char				*beside;
	size_t				 prefix_length;
	int				 error_number, status;

	if (bundle_sources == NULL && source_resolver == NULL)
		return 0;
	path = normalize_requested_bundle_path(requested_path);
	if (path == NULL)
		return -1;
	if (bundle_sources == NULL) {
		memset(&resolved, 0, sizeof(resolved));
		current = mandoc_msg_getinfilename();
		status = source_resolver(source_resolver_context, path, current,
		    &resolved);
		if (status <= 0) {
			switch (status) {
			case -2:
				error_number = EPERM;
				break;
			case -3:
				error_number = EIO;
				break;
			default:
				error_number = ENOENT;
				break;
			}
			free(path);
			errno = error_number;
			return -1;
		}
		if (resolved.path == NULL || resolved.data == NULL ||
		    !is_safe_bundle_path(resolved.path)) {
			free(path);
			errno = EINVAL;
			return -1;
		}
		resolved_path = copy_string(resolved.path);
		resolved_data = malloc(resolved.length == 0 ? 1 :
		    resolved.length);
		if (resolved_path == NULL || resolved_data == NULL) {
			free(resolved_path);
			free(resolved_data);
			free(path);
			errno = ENOMEM;
			return -1;
		}
		if (resolved.length != 0)
			memcpy(resolved_data, resolved.data, resolved.length);
		mparse_readmem(parser, resolved_data, resolved.length,
		    resolved_path);
		free(resolved_data);
		free(resolved_path);
		free(path);
		return 1;
	}
	source = find_bundle_source(path);
	if (source == NULL &&
	    (current = mandoc_msg_getinfilename()) != NULL &&
	    (slash = strrchr(current, '/')) != NULL) {
		prefix_length = (size_t)(slash - current + 1);
		beside = malloc(prefix_length + strlen(path) + 1);
		if (beside == NULL) {
			free(path);
			errno = ENOMEM;
			return -1;
		}
		memcpy(beside, current, prefix_length);
		strcpy(beside + prefix_length, path);
		source = find_bundle_source(beside);
		free(beside);
	}
	if (source == NULL) {
		free(path);
		errno = ENOENT;
		return -1;
	}
	mparse_readmem(parser, source->data, source->length, source->path);
	free(path);
	return 1;
}

static const struct mant_mandoc_source *
find_bundle_source(const char *path)
{
	size_t i;

	if (path == NULL)
		return NULL;
	for (i = 0; i < bundle_source_count; i++)
		if (bundle_sources[i].path != NULL &&
		    strcmp(bundle_sources[i].path, path) == 0)
			return bundle_sources + i;
	return NULL;
}

static int
is_safe_bundle_path(const char *path)
{
	const char *component, *end;

	if (path == NULL || *path == '\0' || *path == '/' ||
	    strchr(path, '\\') != NULL)
		return 0;
	component = path;
	while (*component != '\0') {
		end = component;
		while (*end != '\0' && *end != '/')
			end++;
		if (end == component ||
		    (end - component == 1 && component[0] == '.') ||
		    (end - component == 2 && component[0] == '.' &&
		    component[1] == '.'))
			return 0;
		if (*end == '/' && end[1] == '\0')
			return 0;
		component = *end == '/' ? end + 1 : end;
	}
	return 1;
}

/* Normalize harmless current-directory components in an untrusted request. */
static char *
normalize_requested_bundle_path(const char *path)
{
	const char	*component, *end;
	char		*normalized, *output;
	size_t		 length;

	if (path == NULL || *path == '\0' || *path == '/' ||
	    strchr(path, '\\') != NULL) {
		errno = EPERM;
		return NULL;
	}
	length = strlen(path);
	normalized = malloc(length + 1);
	if (normalized == NULL) {
		errno = ENOMEM;
		return NULL;
	}
	output = normalized;
	component = path;
	while (*component != '\0') {
		end = component;
		while (*end != '\0' && *end != '/')
			end++;
		if (end == component ||
		    (end - component == 2 && component[0] == '.' &&
		    component[1] == '.') ||
		    (*end == '/' && end[1] == '\0'))
			goto unsafe;
		if (!(end - component == 1 && component[0] == '.')) {
			if (output != normalized)
				*output++ = '/';
			memcpy(output, component, (size_t)(end - component));
			output += end - component;
		}
		component = *end == '/' ? end + 1 : end;
	}
	if (output == normalized)
		goto unsafe;
	*output = '\0';
	return normalized;

unsafe:
	free(normalized);
	errno = EPERM;
	return NULL;
}

#ifndef MANDOC_MEMORY_ONLY
/* Open `path` under `base`, leaving errno describing any failure. */
static int
open_under_base(const char *base, const char *path, int flags, mode_t mode)
{
	char	*resolved;
	int	 fd, saved_errno;

	if (base == NULL) {
		errno = ENOENT;
		return -1;
	}
	resolved = malloc(strlen(base) + strlen(path) + 2);
	if (resolved == NULL) {
		errno = ENOMEM;
		return -1;
	}
	sprintf(resolved, "%s/%s", base, path);
	fd = openat(AT_FDCWD, resolved, flags, mode);
	saved_errno = errno;
	free(resolved);
	errno = saved_errno;
	return fd;
}

/*
 * Open a relative path without following any link below an explicit root.
 * The root itself is caller-approved and may be a link; every component after
 * it is opened relative to the previous directory descriptor with O_NOFOLLOW.
 */
static int
open_beneath_root(const char *path, int flags, mode_t mode)
{
	char	*copy, *component, *separator;
	int	 dirfd, fd, saved_errno;

	if (source_root == NULL || path == NULL || *path == '\0' ||
	    *path == '/') {
		errno = EPERM;
		return -1;
	}
	dirfd = openat(AT_FDCWD, source_root,
	    O_RDONLY | O_DIRECTORY | O_CLOEXEC);
	if (dirfd == -1)
		return -1;
	copy = copy_string(path);
	if (copy == NULL) {
		close(dirfd);
		errno = ENOMEM;
		return -1;
	}
	component = copy;
	fd = -1;
	for (;;) {
		while (*component == '/')
			component++;
		if (*component == '\0') {
			errno = EINVAL;
			break;
		}
		separator = strchr(component, '/');
		if (separator != NULL)
			*separator = '\0';
		if (strcmp(component, "..") == 0) {
			errno = EPERM;
			break;
		}
		if (strcmp(component, ".") == 0) {
			if (separator == NULL) {
				errno = EINVAL;
				break;
			}
			component = separator + 1;
			continue;
		}
		if (separator == NULL) {
			fd = openat(dirfd, component,
			    flags | O_NOFOLLOW | O_CLOEXEC, mode);
			break;
		}
		fd = openat(dirfd, component,
		    O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
		if (fd == -1)
			break;
		close(dirfd);
		dirfd = fd;
		fd = -1;
		component = separator + 1;
	}
	saved_errno = errno;
	free(copy);
	close(dirfd);
	errno = saved_errno;
	return fd;
}

/* Resolve a bare include beside the source, still walking from the root. */
static int
open_beside_source(const char *path, int flags, mode_t mode)
{
	const char	*relative;
	char		*combined;
	size_t		 root_length;
	int		 fd, saved_errno;

	if (source_root == NULL || source_dir == NULL) {
		errno = ENOENT;
		return -1;
	}
	root_length = strlen(source_root);
	if (strncmp(source_dir, source_root, root_length) != 0 ||
	    (root_length != 1 && source_dir[root_length] != '/' &&
	    source_dir[root_length] != '\0')) {
		errno = EPERM;
		return -1;
	}
	relative = source_dir + root_length;
	while (*relative == '/')
		relative++;
	if (*relative == '\0')
		return open_beneath_root(path, flags, mode);
	combined = malloc(strlen(relative) + strlen(path) + 2);
	if (combined == NULL) {
		errno = ENOMEM;
		return -1;
	}
	sprintf(combined, "%s/%s", relative, path);
	fd = open_beneath_root(combined, flags, mode);
	saved_errno = errno;
	free(combined);
	errno = saved_errno;
	return fd;
}

/* Resolve includes against the original source tree without changing cwd. */
int
mant_mandoc_source_open(const char *path, int flags, ...)
{
	int		 fd, saved_errno, top_level_path;
	mode_t		 mode;
	size_t		 source_path_length;
	va_list		 arguments;

	mode = 0;
	if (flags & O_CREAT) {
		va_start(arguments, flags);
		mode = (mode_t)va_arg(arguments, int);
		va_end(arguments);
	}
	/*
	 * The top-level file is caller-selected and remains readable under
	 * IncludePolicy::Deny. Every later open comes from roff `.so` handling
	 * and must be authorized explicitly instead of treating an unset root as
	 * permission to fall back to the process working directory.
	 */
	top_level_path = 0;
	if (source_path_pending && source_path != NULL) {
		source_path_length = strlen(source_path);
		top_level_path = strcmp(path, source_path) == 0 ||
		    (strncmp(path, source_path, source_path_length) == 0 &&
		    strcmp(path + source_path_length, ".gz") == 0);
	}
	if (top_level_path) {
		fd = openat(AT_FDCWD, path, flags, mode);
		if (fd != -1)
			source_path_pending = 0;
		return fd;
	}
	if (source_includes_allowed == 0) {
		errno = EPERM;
		return -1;
	}
	if (*path == '/') {
		if (source_root_strict &&
		    (source_path == NULL || strcmp(path, source_path) != 0)) {
			errno = EPERM;
			return -1;
		}
		return openat(AT_FDCWD, path, flags, mode);
	}
	if (source_root_strict && source_root == NULL) {
		errno = ENOMEM;
		return -1;
	}
	if (source_root == NULL && source_dir == NULL)
		return openat(AT_FDCWD, path, flags, mode);
	if (source_root_strict && !is_safe_relative_path(path)) {
		errno = EPERM;
		return -1;
	}

	/*
	 * Try the stripped hierarchy root first (`.so man1/foo.1`), then the
	 * unstripped stub directory (`.so last.1` next to the stub). man(1)
	 * accepts both spellings, and mparse_open retries each base with a
	 * `.gz` suffix, so compressed targets resolve without extra handling.
	 */
	fd = source_root_strict ? open_beneath_root(path, flags, mode) :
	    open_under_base(source_root, path, flags, mode);
	if (fd != -1)
		return fd;
	saved_errno = errno;
	if (source_dir != NULL &&
	    (source_root == NULL || strcmp(source_dir, source_root) != 0)) {
		fd = source_root_strict ? open_beside_source(path, flags, mode) :
		    open_under_base(source_dir, path, flags, mode);
		if (fd != -1)
			return fd;
		saved_errno = errno;
	}
	if (source_root_strict) {
		errno = saved_errno;
		return -1;
	}
	fd = openat(AT_FDCWD, path, flags, mode);
	if (fd == -1)
		errno = saved_errno;
	return fd;
}

/* Reject lexical escapes when a caller supplied an explicit include root. */
static int
is_safe_relative_path(const char *path)
{
	const char *component, *end;

	component = path;
	while (*component != '\0') {
		while (*component == '/')
			component++;
		end = component;
		while (*end != '\0' && *end != '/')
			end++;
		if (end - component == 2 && component[0] == '.' &&
		    component[1] == '.')
			return 0;
		component = end;
	}
	return 1;
}
#endif

void
mant_mandoc_document_free(struct mant_mandoc_document *document)
{
	if (document == NULL)
		return;
	free(document->error);
	free(document->diagnostics);
	free(document->equation);
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

static unsigned int
snapshot_node_flags(const struct roff_node *source)
{
	unsigned int flags;

	flags = 0;
	if (source->flags & NODE_NOSRC)
		flags |= MANT_MANDOC_NODE_GENERATED;
	if (source->flags & NODE_EOS)
		flags |= MANT_MANDOC_NODE_SENTENCE_END;
	if (source->flags & NODE_NOPRT)
		flags |= MANT_MANDOC_NODE_NO_PRINT;
	if (source->flags & NODE_NOFILL)
		flags |= MANT_MANDOC_NODE_NO_FILL;
	if (source->flags & NODE_ID)
		flags |= MANT_MANDOC_NODE_DEEP_LINK_TARGET;
	if (source->flags & NODE_HREF)
		flags |= MANT_MANDOC_NODE_PERMALINK;
	if (source->flags & NODE_LINE)
		flags |= MANT_MANDOC_NODE_LINE_START;
	if (source->flags & NODE_DELIMO)
		flags |= MANT_MANDOC_NODE_DELIMITER_OPEN;
	if (source->flags & NODE_DELIMC)
		flags |= MANT_MANDOC_NODE_DELIMITER_CLOSE;
	if (source->flags & NODE_SYNPRETTY)
		flags |= MANT_MANDOC_NODE_SYNOPSIS_PRETTY;
	return flags;
}

struct text_buffer {
	char	*data;
	size_t	 length;
	size_t	 capacity;
};

static int append_text(struct text_buffer *, const char *);
static int append_equation(struct text_buffer *, const struct eqn_box *, int,
    int *);

static char *
copy_equation(const struct eqn_box *box, int *truncated)
{
	struct text_buffer	buffer;

	memset(&buffer, 0, sizeof(buffer));
	if (!append_equation(&buffer, box, 0, truncated)) {
		free(buffer.data);
		return NULL;
	}
	return buffer.data;
}

static int
append_equation(struct text_buffer *buffer, const struct eqn_box *box,
    int depth, int *truncated)
{
	const struct eqn_box	*child;
	const char		*operator;

	if (box == NULL)
		return append_text(buffer, "");
	/*
	 * The eqn box tree is a recursive walk the node-copy cap never reaches:
	 * copy_equation enters it once, then braces nest boxes without bound, so
	 * `{{{...}}}` overflows the stack here. Stop rendering past the same cap
	 * and keep the text gathered so far; deeper eqn content is dropped, not
	 * a whole-page failure. Real equations nest only a handful of levels.
	 */
	if (depth >= MANT_MANDOC_MAX_COPY_DEPTH) {
		*truncated = 1;
		return 1;
	}
	if (box->pos == EQNPOS_SQRT && !append_text(buffer, "sqrt("))
		return 0;
	if (!append_text(buffer, box->left) ||
	    !append_text(buffer, box->text != NULL &&
	    strcmp(box->text, "ldots") == 0 ? "..." : box->text))
		return 0;
	child = box->first;
	if (box->pos == EQNPOS_SQRT) {
		if (child != NULL &&
		    !append_equation(buffer, child, depth + 1, truncated))
			return 0;
	} else if (box->type == EQN_SUBEXPR && child != NULL &&
	    box->pos != EQNPOS_NONE) {
		if (!append_equation(buffer, child, depth + 1, truncated))
			return 0;
		operator = box->pos == EQNPOS_OVER ? " / " :
		    box->pos == EQNPOS_SUP || box->pos == EQNPOS_TO ? " ^ " : " _ ";
		if (!append_text(buffer, operator))
			return 0;
		child = child->next;
		if (child != NULL &&
		    !append_equation(buffer, child, depth + 1, truncated))
			return 0;
		if (child != NULL &&
		    (box->pos == EQNPOS_FROMTO || box->pos == EQNPOS_SUBSUP)) {
			child = child->next;
			if (child != NULL &&
			    (!append_text(buffer, " ^ ") ||
			     !append_equation(buffer, child, depth + 1,
			     truncated)))
				return 0;
		}
	} else {
		for (; child != NULL; child = child->next) {
			if (child != box->first && !append_text(buffer, " "))
				return 0;
			if (!append_equation(buffer, child, depth + 1,
			    truncated))
				return 0;
		}
	}
	if (!append_text(buffer, box->right))
		return 0;
	if (box->pos == EQNPOS_SQRT)
		return append_text(buffer, ")");
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
snapshot_normalized_data(struct mant_mandoc_node_view *view,
    const struct roff_node *source)
{
	if (source->norm == NULL)
		return;
	if (source->tok == MDOC_Bl) {
		view->compact = source->norm->Bl.comp;
		view->offset = source->norm->Bl.offs;
		view->width = source->norm->Bl.width;
		switch (source->norm->Bl.type) {
		case LIST_bullet:
		case LIST_dash:
		case LIST_hyphen:
			view->list_kind = MANT_MANDOC_LIST_BULLET;
			break;
		case LIST_enum:
			view->list_kind = MANT_MANDOC_LIST_ORDERED;
			break;
		case LIST_diag:
		case LIST_hang:
		case LIST_inset:
		case LIST_ohang:
		case LIST_tag:
			view->list_kind = MANT_MANDOC_LIST_DEFINITION;
			break;
		case LIST_column:
			view->list_kind = MANT_MANDOC_LIST_COLUMN;
			break;
		case LIST_item:
			view->list_kind = MANT_MANDOC_LIST_PLAIN;
			break;
		case LIST__NONE:
		case LIST_MAX:
			break;
		}
	} else if (source->tok == MDOC_Bd) {
		view->compact = source->norm->Bd.comp;
		view->offset = source->norm->Bd.offs;
		switch (source->norm->Bd.type) {
		case DISP_unfilled:
		case DISP_literal:
			view->display_kind = MANT_MANDOC_DISPLAY_LITERAL;
			break;
		case DISP_centered:
		case DISP_ragged:
		case DISP_filled:
			view->display_kind = MANT_MANDOC_DISPLAY_FILLED;
			break;
		case DISP__NONE:
			break;
		}
	} else if (source->tok == MDOC_Bf) {
		switch (source->norm->Bf.font) {
		case FONT_Em:
			view->font_kind = MANT_MANDOC_FONT_EMPHASIS;
			break;
		case FONT_Li:
			view->font_kind = MANT_MANDOC_FONT_LITERAL;
			break;
		case FONT_Sy:
			view->font_kind = MANT_MANDOC_FONT_SYMBOLIC;
			break;
		case FONT__NONE:
			break;
		}
	} else if (source->tok == MDOC_An) {
		switch (source->norm->An.auth) {
		case AUTH_split:
			view->author_mode = MANT_MANDOC_AUTHOR_SPLIT;
			break;
		case AUTH_nosplit:
			view->author_mode = MANT_MANDOC_AUTHOR_NOSPLIT;
			break;
		case AUTH__NONE:
			break;
		}
	} else if (source->tok == MDOC_En && source->norm->Es != NULL &&
	    source->norm->Es->child != NULL) {
		view->enclosure_open = source->norm->Es->child->string;
		if (source->norm->Es->child->next != NULL)
			view->enclosure_close =
			    source->norm->Es->child->next->string;
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

int
mant_mandoc_document_equation_truncated(
    const struct mant_mandoc_document *document)
{
	return document == NULL ? 0 : document->equation_truncated;
}

const struct mant_mandoc_node *
mant_mandoc_document_root(const struct mant_mandoc_document *document)
{
	return document == NULL ? NULL :
	    (const struct mant_mandoc_node *)(document->meta == NULL ? NULL :
	    document->meta->first);
}

int
mant_mandoc_node_snapshot(struct mant_mandoc_document *document,
    const struct mant_mandoc_node *node,
    struct mant_mandoc_node_view *view)
{
	const struct roff_node *source;

	if (document == NULL || document->parser == NULL || node == NULL ||
	    view == NULL)
		return 0;
	source = (const struct roff_node *)node;
	memset(view, 0, sizeof(*view));
	view->kind = (int)source->type;
	if (source->type != ROFFT_ROOT && source->tok != TOKEN_NONE)
		view->macro_name = roff_name[source->tok];
	if (source->type == ROFFT_TEXT || source->type == ROFFT_COMMENT)
		view->text = source->string;
	view->tag = source->tag;
	view->line = source->line;
	view->column = source->pos + 1;
	view->flags = snapshot_node_flags(source);
	snapshot_normalized_data(view, source);
	if (source->type == ROFFT_TBL && source->span != NULL &&
	    source->span->pos == TBL_SPAN_DATA)
		view->table_cells =
		    (const struct mant_mandoc_table_cell *)source->span->first;
	else if (source->type == ROFFT_EQN) {
		/* The returned equation pointer is borrowed only until the next
		 * node snapshot on this document replaces the shared buffer. */
		free(document->equation);
		document->equation = copy_equation(source->eqn,
		    &document->equation_truncated);
		view->equation = document->equation;
	}
	view->child = (const struct mant_mandoc_node *)source->child;
	view->next = (const struct mant_mandoc_node *)source->next;
	return 1;
}

int
mant_mandoc_table_cell_snapshot(const struct mant_mandoc_document *document,
    const struct mant_mandoc_table_cell *cell,
    struct mant_mandoc_table_cell_view *view)
{
	const struct tbl_dat *source;

	if (document == NULL || document->parser == NULL || cell == NULL ||
	    view == NULL)
		return 0;
	source = (const struct tbl_dat *)cell;
	memset(view, 0, sizeof(*view));
	view->text = source->string;
	view->text_block = source->block;
	view->vertical_continuation =
	    (source->layout != NULL && source->layout->pos == TBL_CELL_DOWN) ||
	    (source->string != NULL && !strcmp(source->string, "\\^"));
	view->column_span = source->hspans < 0 ? 1U :
	    (unsigned int)source->hspans + 1U;
	view->row_span = source->vspans < 0 ? 1U :
	    (unsigned int)source->vspans + 1U;
	if (source->layout != NULL &&
	    source->layout->pos == TBL_CELL_CENTRE)
		view->alignment = 1;
	else if (source->layout != NULL &&
	    (source->layout->pos == TBL_CELL_RIGHT ||
	     source->layout->pos == TBL_CELL_NUMBER))
		view->alignment = 2;
	view->next = (const struct mant_mandoc_table_cell *)source->next;
	return 1;
}

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
