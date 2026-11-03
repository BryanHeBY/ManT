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
	active_escape_document = document;
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
	if (meta == NULL) {
		document->error = copy_string(
		    "native syntax tree exceeds the 512-level nesting limit");
		goto cleanup;
	}
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

cleanup:
	active_escape_document = NULL;
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
