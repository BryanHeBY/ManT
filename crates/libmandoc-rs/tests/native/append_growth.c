/*
 * Copyright (c) 2026 ManT contributors
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHORS DISCLAIM ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */

/*
 * Explicit C-only maintenance probe. Compile the actual parser archive and
 * this driver with MANDOC_APPEND_TEST; never enable it for normal builds.
 * Pass exact pristine-oracle source files as argv. The helper checks alone
 * are not evidence about parser retirement: every source also executes
 * readmem -> result -> reset -> readmem -> result -> free below.
 */
#include "config.h"

#ifndef MANDOC_APPEND_TEST
#error "This maintenance probe requires MANDOC_APPEND_TEST"
#endif

#include <errno.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "mandoc.h"
#include "mandoc_aux.h"
#include "roff.h"
#include "mandoc_parse.h"
#include "mant_mandoc_shim.h"

/* Parser hooks only: the probe receives fully materialized memory sources. */
int
mant_mandoc_read_bundle(struct mparse *parser, const char *name)
{

	(void)parser;
	(void)name;
	return 0;
}

int
mant_mandoc_source_open(const char *name, int flags, ...)
{

	(void)name;
	(void)flags;
	errno = ENOENT;
	return -1;
}

void
mant_mandoc_note_escape_depth_limit(void)
{
}

static void
require(int passed, const char *reason)
{

	if (!passed) {
		fprintf(stderr, "append growth: %s\n", reason);
		exit(1);
	}
}

static void
check_helper(void)
{
	char *text = NULL;
	size_t used = 0, capacity = 0, index;
	struct mandoc_append_metrics metrics;

	mandoc_append_test_reset();
	mandoc_str_append(&text, &used, &capacity, "", 0, 0);
	require(text != NULL && used == 0 && text[0] == '\0',
	    "first empty input must allocate a NUL-terminated string");
	mandoc_str_append(&text, &used, &capacity, "", 0, 1);
	require(used == 1 && strcmp(text, " ") == 0,
	    "an appended empty input must preserve its separator");
	for (index = 0; index < 8192; index++) {
		mandoc_str_append(&text, &used, &capacity, "abc", 3, 1);
		require(text[used] == '\0', "instantaneous NUL termination");
		require(capacity >= used + 1, "capacity contains the terminator");
	}
	mandoc_append_test_read(&metrics);
	require(metrics.append_calls == 8194, "every append counted once");
	require(metrics.append_bytes == used, "separator bytes counted once");
	require(metrics.relocation_bytes <= 2 * used,
	    "geometric helper relocation upper bound");
	require(metrics.reserve_calls <= 12, "logarithmic helper reserves");
	free(text);
}

static unsigned char *
read_source(const char *name, size_t *length)
{
	FILE *input;
	unsigned char *source;
	long size;

	input = fopen(name, "rb");
	require(input != NULL, "open exact source");
	require(fseek(input, 0, SEEK_END) == 0, "seek source end");
	size = ftell(input);
	require(size >= 0 && (uintmax_t)size < SIZE_MAX, "source length");
	require(fseek(input, 0, SEEK_SET) == 0, "seek source start");
	*length = (size_t)size;
	source = mandoc_malloc(*length + 1);
	require(fread(source, 1, *length, input) == *length, "read exact source");
	require(fclose(input) == 0, "close exact source");
	source[*length] = '\0';
	return source;
}

static void
check_parser(const char *name)
{
	struct mparse *parser;
	struct mandoc_append_metrics metrics;
	unsigned char *source;
	size_t length;

	source = read_source(name, &length);
	mandoc_append_test_reset();
	mchars_alloc();
	parser = mparse_alloc(MPARSE_VALIDATE | MPARSE_UTF8 | MPARSE_LATIN1,
	    MANDOC_OS_OTHER, "ManT");
	mparse_readmem(parser, source, length, name);
	(void)mparse_result(parser);
	mparse_reset(parser);
	mparse_readmem(parser, source, length, name);
	(void)mparse_result(parser);
	mparse_free(parser);
	mchars_free();
	free(source);
	mandoc_append_test_read(&metrics);
	require(metrics.seed_calls == metrics.retired_runs,
	    "all actual parser receipts retire before reset/free");
	require(metrics.seed_bytes + metrics.append_bytes == metrics.retired_bytes,
	    "native run seed and accepted append bytes account exactly once");
	require(metrics.relocation_bytes <= 2 * metrics.retired_bytes,
	    "actual run relocation bound includes every new seed");
	require(metrics.reserve_calls <= metrics.append_calls,
	    "actual reserves belong to an append");
	printf("%s seed=%zu seed_bytes=%zu append=%zu append_bytes=%zu "
	    "reserve=%zu relocation=%zu retired=%zu retired_bytes=%zu\n", name,
	    metrics.seed_calls, metrics.seed_bytes, metrics.append_calls,
	    metrics.append_bytes, metrics.reserve_calls, metrics.relocation_bytes,
	    metrics.retired_runs, metrics.retired_bytes);
}

int
main(int argc, char **argv)
{
	char *text = NULL;
	size_t used = 0, capacity = 0;
	int index;

	mandoc_msg_setoutfile(stderr);
	mandoc_msg_setmin(MANDOCERR_MAX);
	/* Checked arithmetic must fail before attempting allocation or copying. */
	if (argc == 2 && strcmp(argv[1], "--overflow-used") == 0) {
		used = SIZE_MAX;
		mandoc_str_append(&text, &used, &capacity, "", 0, 1);
		return 1;
	}
	if (argc == 2 && strcmp(argv[1], "--overflow-source") == 0) {
		mandoc_str_append(&text, &used, &capacity, "", SIZE_MAX, 0);
		return 1;
	}
	require(argc > 1, "pass actual parser source files, not only a microtest");
	check_helper();
	for (index = 1; index < argc; index++)
		check_parser(argv[index]);
	return 0;
}
