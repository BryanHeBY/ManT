/*
 * Stable JSON boundary for a pinned copy of libmandoc.
 *
 * libmandoc deliberately does not provide a stable third-party ABI.  This
 * program is built from the same pinned source tree as the library and
 * exposes only the mant.roff-ast/v1 protocol to the TypeScript application.
 */
#include "config.h"

#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#include "mandoc.h"
#include "roff.h"
#include "mandoc_parse.h"

#define MANT_MANDOC_VERSION "1.14.6"

static void emit_document(const char *, const struct roff_meta *);
static void emit_node(const struct roff_node *);
static void emit_json_string(const char *);
static void usage(const char *);
static int document_has_body(const struct roff_meta *);

static const char *node_type_name(enum roff_type);
static const char *macroset_name(enum roff_macroset);
static const char *result_level_name(enum mandoclevel);

int
main(int argc, char *argv[])
{
	const char		*path;
	struct mparse		*parser;
	struct roff_meta	*meta;
	int			 options, fd;

	options = MPARSE_UTF8 | MPARSE_LATIN1 | MPARSE_VALIDATE | MPARSE_COMMENT;
	path = NULL;

	if (argc == 2 && strcmp(argv[1], "--help") == 0) {
		usage(argv[0]);
		return 0;
	}
	if (argc == 3 && strcmp(argv[1], "--allow-include") == 0) {
		options |= MPARSE_SO;
		path = argv[2];
	} else if (argc == 2) {
		path = argv[1];
	} else {
		usage(argv[0]);
		return 64;
	}

	setprogname("mant-mandoc-json");
	mandoc_msg_setoutfile(stderr);
	/* Unsupported roff is the compatibility boundary: normal style warnings
	 * are common in generated GNU manuals and should not reject their AST. */
	mandoc_msg_setmin(MANDOCERR_UNSUPP);
	mchars_alloc();
	parser = mparse_alloc(options, MANDOC_OS_OTHER, NULL);
	fd = mparse_open(parser, path);
	if (fd == -1) {
		perror(path);
		mparse_free(parser);
		mchars_free();
		return 66;
	}

	mparse_readfd(parser, fd, path);
	close(fd);
	meta = mparse_result(parser);
	emit_document(path, meta);

	mparse_free(parser);
	mchars_free();
	/* Parsing problems are represented in JSON and stderr, not as a protocol
	 * failure.  Only argument and file-open errors use non-zero exit codes. */
	return 0;
}

static void
usage(const char *program)
{
	fprintf(stderr, "usage: %s [--allow-include] file\n", program);
}

static void
emit_document(const char *path, const struct roff_meta *meta)
{
	fputs("{\"schema\":\"mant.roff-ast/v1\",\"engine\":{\"name\":\"libmandoc\",\"version\":\"", stdout);
	fputs(MANT_MANDOC_VERSION, stdout);
	fputs("\"},\"source\":{\"path\":", stdout);
	emit_json_string(path);
	fputs("},\"macroSet\":", stdout);
	emit_json_string(macroset_name(meta->macroset));
	fputs(",\"resultLevel\":", stdout);
	emit_json_string(result_level_name(mandoc_msg_getrc()));
	fputs(",\"meta\":{\"title\":", stdout);
	emit_json_string(meta->title);
	fputs(",\"section\":", stdout);
	emit_json_string(meta->msec);
	fputs(",\"volume\":", stdout);
	emit_json_string(meta->vol);
	fputs(",\"os\":", stdout);
	emit_json_string(meta->os);
	fputs(",\"name\":", stdout);
	emit_json_string(meta->name);
	fputs(",\"aliasTarget\":", stdout);
	emit_json_string(meta->sodest);
	fprintf(stdout, ",\"hasBody\":%s},\"root\":",
	    document_has_body(meta) ? "true" : "false");
	emit_node(meta->first);
	fputs("}\n", stdout);
}

/*
 * roff_meta.hasbody is populated by man_validate() but not by
 * mdoc_validate() in mandoc 1.14.6.  Derive the protocol field from the
 * renderer-neutral root instead, so equivalent man(7) and mdoc(7) documents
 * report the same value.
 */
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

static void
emit_node(const struct roff_node *node)
{
	const struct roff_node	*child;
	const char		*macro;
	int			 first;

	if (node == NULL) {
		fputs("null", stdout);
		return;
	}

	fputs("{\"kind\":", stdout);
	emit_json_string(node_type_name(node->type));
	if (node->type != ROFFT_ROOT && node->tok != TOKEN_NONE) {
		macro = roff_name[node->tok];
		if (macro != NULL) {
			fputs(",\"macro\":", stdout);
			emit_json_string(macro);
		}
	}
	if (node->type == ROFFT_TEXT || node->type == ROFFT_COMMENT) {
		fputs(",\"text\":", stdout);
		emit_json_string(node->string);
	}
	fprintf(stdout, ",\"loc\":{\"line\":%d,\"column\":%d}",
	    node->line, node->pos + 1);
	fputs(",\"flags\":{\"generated\":", stdout);
	fputs((node->flags & NODE_NOSRC) ? "true" : "false", stdout);
	fputs(",\"sentenceEnd\":", stdout);
	fputs((node->flags & NODE_EOS) ? "true" : "false", stdout);
	fputs(",\"noPrint\":", stdout);
	fputs((node->flags & NODE_NOPRT) ? "true" : "false", stdout);
	fputs("},\"children\":[", stdout);

	first = 1;
	for (child = node->child; child != NULL; child = child->next) {
		if (!first)
			putchar(',');
		emit_node(child);
		first = 0;
	}
	fputs("]}", stdout);
}

static void
emit_json_string(const char *string)
{
	const unsigned char	*cursor;

	if (string == NULL) {
		fputs("null", stdout);
		return;
	}

	putchar('"');
	for (cursor = (const unsigned char *)string; *cursor != '\0'; cursor++) {
		switch (*cursor) {
		case '"':
			fputs("\\\"", stdout);
			break;
		case '\\':
			fputs("\\\\", stdout);
			break;
		case '\b':
			fputs("\\b", stdout);
			break;
		case '\f':
			fputs("\\f", stdout);
			break;
		case '\n':
			fputs("\\n", stdout);
			break;
		case '\r':
			fputs("\\r", stdout);
			break;
		case '\t':
			fputs("\\t", stdout);
			break;
		default:
			if (*cursor < 0x20)
				fprintf(stdout, "\\u%04x", *cursor);
			else
				putchar(*cursor);
		}
	}
	putchar('"');
}

static const char *
node_type_name(enum roff_type type)
{
	switch (type) {
	case ROFFT_ROOT:
		return "root";
	case ROFFT_BLOCK:
		return "block";
	case ROFFT_HEAD:
		return "head";
	case ROFFT_BODY:
		return "body";
	case ROFFT_TAIL:
		return "tail";
	case ROFFT_ELEM:
		return "element";
	case ROFFT_TEXT:
		return "text";
	case ROFFT_COMMENT:
		return "comment";
	case ROFFT_TBL:
		return "table";
	case ROFFT_EQN:
		return "equation";
	}
	return "unknown";
}

static const char *
macroset_name(enum roff_macroset macroset)
{
	switch (macroset) {
	case MACROSET_MAN:
		return "man";
	case MACROSET_MDOC:
		return "mdoc";
	case MACROSET_NONE:
		return "none";
	}
	return "unknown";
}

static const char *
result_level_name(enum mandoclevel level)
{
	switch (level) {
	case MANDOCLEVEL_OK:
		return "ok";
	case MANDOCLEVEL_STYLE:
		return "style";
	case MANDOCLEVEL_WARNING:
		return "warning";
	case MANDOCLEVEL_ERROR:
		return "error";
	case MANDOCLEVEL_UNSUPP:
		return "unsupported";
	case MANDOCLEVEL_BADARG:
		return "bad-argument";
	case MANDOCLEVEL_SYSERR:
		return "system-error";
	case MANDOCLEVEL_MAX:
		break;
	}
	return "unknown";
}
