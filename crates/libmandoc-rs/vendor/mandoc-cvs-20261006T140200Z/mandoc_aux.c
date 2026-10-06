/* $Id: mandoc_aux.c,v 1.12 2022/04/14 16:43:44 schwarze Exp $ */
/*
 * Copyright (c) 2014, 2015, 2017, 2018 Ingo Schwarze <schwarze@openbsd.org>
 * Copyright (c) 2009, 2011 Kristaps Dzonsons <kristaps@bsd.lv>
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
#include "config.h"

#include <sys/types.h>

#if HAVE_ERR
#include <err.h>
#endif
#include <errno.h>
#include <stdint.h>
#include <stdarg.h>
#ifdef MANDOC_APPEND_TEST
#include "mant_thread_local.h"
#endif
#include <stdlib.h>
#include <stdio.h>
#include <string.h>

#define DEBUG_NODEF 1
#include "mandoc.h"
#include "mandoc_aux.h"


int
mandoc_asprintf(char **dest, const char *fmt, ...)
{
	va_list	 ap;
	int	 ret;

	va_start(ap, fmt);
	ret = vasprintf(dest, fmt, ap);
	va_end(ap);

	if (ret == -1)
		err((int)MANDOCLEVEL_SYSERR, NULL);
	return ret;
}

void *
mandoc_calloc(size_t num, size_t size)
{
	void	*ptr;

	ptr = calloc(num, size);
	if (ptr == NULL)
		err((int)MANDOCLEVEL_SYSERR, NULL);
	return ptr;
}

void *
mandoc_malloc(size_t size)
{
	void	*ptr;

	ptr = malloc(size);
	if (ptr == NULL)
		err((int)MANDOCLEVEL_SYSERR, NULL);
	return ptr;
}

void *
mandoc_realloc(void *ptr, size_t size)
{
	ptr = realloc(ptr, size);
	if (ptr == NULL)
		err((int)MANDOCLEVEL_SYSERR, NULL);
	return ptr;
}

void *
mandoc_reallocarray(void *ptr, size_t num, size_t size)
{
	ptr = reallocarray(ptr, num, size);
	if (ptr == NULL)
		err((int)MANDOCLEVEL_SYSERR, NULL);
	return ptr;
}

void *
mandoc_recallocarray(void *ptr, size_t oldnum, size_t num, size_t size)
{
	ptr = recallocarray(ptr, oldnum, num, size);
	if (ptr == NULL)
		err((int)MANDOCLEVEL_SYSERR, NULL);
	return ptr;
}

#ifdef MANDOC_APPEND_TEST
/* Explicit maintenance probes only: no production counters or output hook. */
MANT_THREAD_LOCAL struct mandoc_append_metrics append_metrics;

void
mandoc_append_test_reset(void)
{
	memset(&append_metrics, 0, sizeof(append_metrics));
}

void
mandoc_append_test_read(struct mandoc_append_metrics *out)
{
	*out = append_metrics;
}

void
mandoc_append_test_seed(size_t bytes)
{
	append_metrics.seed_calls++;
	append_metrics.seed_bytes += bytes;
}

void
mandoc_append_test_retire(size_t bytes)
{
	append_metrics.retired_runs++;
	append_metrics.retired_bytes += bytes;
}
#endif

/*
 * Grow geometrically, then copy only the new input.  Native tbl_cdata and
 * roff_word_append insert one ASCII space, including between empty words.
 * Capacity is private allocation state; strlen/AST/output facts do not change.
 * Source must be non-NULL and must not alias the destination allocation.
 */
void
mandoc_str_append(char **dest, size_t *used, size_t *capacity,
    const char *source, size_t length, int separate)
{
	size_t need, grown, separator;

	separator = separate != 0;
	if (*used > SIZE_MAX - separator - 1 ||
	    length > SIZE_MAX - *used - separator - 1) {
		errno = ENOMEM;
		err((int)MANDOCLEVEL_SYSERR, NULL);
	}
	need = *used + separator + length + 1;
	if (need > *capacity) {
		grown = *capacity < 64 ? 64 : *capacity;
		while (grown < need) {
			if (grown > SIZE_MAX / 2) {
				grown = need;
				break;
			}
			grown *= 2;
		}
#ifdef MANDOC_APPEND_TEST
		append_metrics.reserve_calls++;
		append_metrics.relocation_bytes += *used;
#endif
		*dest = mandoc_realloc(*dest, grown);
		*capacity = grown;
	}
	if (separator)
		(*dest)[(*used)++] = ' ';
	memcpy(*dest + *used, source, length);
	*used += length;
	(*dest)[*used] = '\0';
#ifdef MANDOC_APPEND_TEST
	append_metrics.append_calls++;
	append_metrics.append_bytes += length + separator;
#endif
}

char *
mandoc_strdup(const char *ptr)
{
	char	*p;

	p = strdup(ptr);
	if (p == NULL)
		err((int)MANDOCLEVEL_SYSERR, NULL);
	return p;
}

char *
mandoc_strndup(const char *ptr, size_t sz)
{
	char	*p;

	p = strndup(ptr, sz);
	if (p == NULL)
		err((int)MANDOCLEVEL_SYSERR, NULL);
	return p;
}
