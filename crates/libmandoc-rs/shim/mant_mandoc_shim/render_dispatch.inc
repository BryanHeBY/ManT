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
render_equation_within_limit(const struct eqn_box *box, unsigned int depth)
{
	for (; box != NULL; box = box->next) {
		if (depth >= 256 ||
		    !render_equation_within_limit(box->first, depth + 1))
			return 0;
	}
	return 1;
}

static int
render_tree_within_limit(const struct roff_node *node, unsigned int depth)
{
	for (; node != NULL; node = node->next) {
		if (depth >= 256 ||
		    !render_tree_within_limit(node->child, depth + 1) ||
		    !render_equation_within_limit(node->eqn, 0))
			return 0;
	}
	return 1;
}

static int
render_document(struct mant_mandoc_document *document,
    const struct roff_meta *meta, int format, size_t width,
    int html_fragment, size_t output_limit)
{
	struct manoutput options;
	void		*renderer;
	int		 status;

	/* A bounded preflight precedes every recursive native renderer; the
	 * owned Rust snapshot and output byte budget do not guard this path. */
	if (!render_tree_within_limit(meta->first, 0)) {
		document->error = copy_string(
		    "native rendering exceeds the 256-level nesting limit");
		return 0;
	}
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

