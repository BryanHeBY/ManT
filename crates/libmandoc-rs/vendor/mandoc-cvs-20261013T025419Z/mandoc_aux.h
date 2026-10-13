/* $Id: mandoc_aux.h,v 1.8 2022/04/14 16:43:44 schwarze Exp $ */
/*
 * Copyright (c) 2014, 2017, 2021 Ingo Schwarze <schwarze@openbsd.org>
 * Copyright (c) 2009, 2011 Kristaps Dzonsons <kristaps@bsd.lv>
 *
 * Permission to use, copy, modify, and distribute this software for any
 * purpose with or without fee is hereby granted, provided that the above
 * copyright notice and this permission notice appear in all copies.
 *
 * THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHOR DISCLAIMS ALL WARRANTIES
 * WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
 * MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHOR BE LIABLE FOR
 * ANY SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
 * WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN
 * ACTION OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF
 * OR IN CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.
 */

int		  mandoc_asprintf(char **, const char *, ...)
			__attribute__((__format__ (__printf__, 2, 3)));
void		 *mandoc_calloc(size_t, size_t);
void		 *mandoc_malloc(size_t);
void		 *mandoc_realloc(void *, size_t);
void		 *mandoc_reallocarray(void *, size_t, size_t);
void		 *mandoc_recallocarray(void *, size_t, size_t, size_t);
/* Private parser append storage; output remains an ordinary C string. */
void		  mandoc_str_append(char **, size_t *, size_t *,
			const char *, size_t, int);
#ifdef MANDOC_APPEND_TEST
struct mandoc_append_metrics {
	size_t seed_calls, seed_bytes, append_calls, append_bytes;
	size_t reserve_calls, relocation_bytes, retired_runs, retired_bytes;
};
void mandoc_append_test_reset(void);
void mandoc_append_test_read(struct mandoc_append_metrics *);
void mandoc_append_test_seed(size_t);
void mandoc_append_test_retire(size_t);
#endif
char		 *mandoc_strdup(const char *);
char		 *mandoc_strndup(const char *, size_t);

#if DEBUG_MEMORY
#include "mandoc_dbg.h"
#endif
