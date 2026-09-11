/* $Id: mandoc.c,v 1.121 2022/05/19 15:37:47 schwarze Exp $ */
/*
 * Copyright (c) 2010, 2011, 2015, 2017, 2018, 2019, 2020, 2021
 *               Ingo Schwarze <schwarze@openbsd.org>
 * Copyright (c) 2009, 2010 Kristaps Dzonsons <kristaps@bsd.lv>
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
 *
 * Utility functions to handle end of sentence punctuation
 * and dates and times, for use by mdoc(7) and man(7) parsers.
 * Utility functions to handle fonts and numbers,
 * for use by mandoc(1) parsers and formatters.
 */
#include "config.h"

#include <sys/types.h>

#include <assert.h>
#include <ctype.h>
#include <errno.h>
#include <limits.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <time.h>

#include "mandoc_aux.h"
#include "mandoc.h"
#include "roff.h"
#include "libmandoc.h"
#include "roff_int.h"

static	int	 a2time(time_t *, const char *, const char *);
static	int	 date_is_future(time_t);
static	char	*time2a(time_t, int);
static	int	 time2tm(time_t, int, struct tm *);


enum mandoc_esc
mandoc_font(const char *cp, int sz)
{
	switch (sz) {
	case 0:
		return ESCAPE_FONTPREV;
	case 1:
		switch (cp[0]) {
		case 'C':
		case 'V':
			return ESCAPE_FONTCR;
		case 'B':
		case '3':
			return ESCAPE_FONTBOLD;
		case 'I':
		case '2':
			return ESCAPE_FONTITALIC;
		case 'P':
			return ESCAPE_FONTPREV;
		case 'R':
		case '1':
			return ESCAPE_FONTROMAN;
		case '4':
			return ESCAPE_FONTBI;
		default:
			return ESCAPE_ERROR;
		}
	case 2:
		switch (cp[0]) {
		case 'V':
			switch (cp[1]) {
			case 'B':
				return ESCAPE_FONTCB;
			case 'I':
				return ESCAPE_FONTCI;
			default:
				return ESCAPE_ERROR;
			}
		case 'B':
			switch (cp[1]) {
			case 'I':
				return ESCAPE_FONTBI;
			default:
				return ESCAPE_ERROR;
			}
		case 'C':
			switch (cp[1]) {
			case 'B':
				return ESCAPE_FONTCB;
			case 'I':
				return ESCAPE_FONTCI;
			case 'R':
			case 'W':
				return ESCAPE_FONTCR;
			default:
				return ESCAPE_ERROR;
			}
		default:
			return ESCAPE_ERROR;
		}
	default:
		return ESCAPE_ERROR;
	}
}

static int
a2time(time_t *t, const char *fmt, const char *p)
{
	struct tm	 tm;
	char		*pp;

	memset(&tm, 0, sizeof(struct tm));

	pp = NULL;
#if HAVE_STRPTIME
	pp = strptime(p, fmt, &tm);
#endif
	if (NULL != pp && '\0' == *pp) {
		/* Manual dates are calendar values without a timezone. */
#if defined(_WIN32)
		*t = _mkgmtime64(&tm);
#else
		*t = timegm(&tm);
#endif
		return 1;
	}

	return 0;
}

static char *
time2a(time_t t, int utc)
{
	static const char *const months[] = {
		"January", "February", "March", "April", "May", "June",
		"July", "August", "September", "October", "November",
		"December"
	};
	struct tm	 tm_storage, *tm;
	char		*buf, *p;
	int		 isz;

	buf = NULL;
	if (time2tm(t, utc, &tm_storage) == 0)
		goto fail;
	tm = &tm_storage;

	/*
	 * Reserve space:
	 * up to 9 characters for the month (September) + blank
	 * up to 2 characters for the day + comma + blank
	 * 4 characters for the year and a terminating '\0'
	 */

	p = buf = mandoc_malloc(10 + 4 + 4 + 1);

	if (tm->tm_mon < 0 || tm->tm_mon >= 12)
		goto fail;
	isz = snprintf(p, 10 + 1, "%s ", months[tm->tm_mon]);
	if (isz < 0 || isz > 10)
		goto fail;
	p += isz;

	/*
	 * The output format is just "%d" here, not "%2d" or "%02d".
	 * That's also the reason why we can't just format the
	 * date as a whole with "%B %e, %Y" or "%B %d, %Y".
	 * Besides, the present approach is less prone to buffer
	 * overflows, in case anybody should ever introduce the bug
	 * of looking at LC_TIME.
	 */

	isz = snprintf(p, 4 + 1, "%d, ", tm->tm_mday);
	if (isz < 0 || isz > 4)
		goto fail;
	p += isz;

	isz = snprintf(p, 4 + 1, "%04d", tm->tm_year + 1900);
	if (isz != 4)
		goto fail;
	return buf;

fail:
	free(buf);
	return mandoc_strdup("");
}

static int
time2tm(time_t t, int utc, struct tm *tm)
{
#if defined(_WIN32)
	return (utc ? gmtime_s(tm, &t) : localtime_s(tm, &t)) == 0;
#else
	return (utc ? gmtime_r(&t, tm) : localtime_r(&t, tm)) != NULL;
#endif
}

static int
date_is_future(time_t t)
{
	struct tm	 date, tomorrow;
	time_t		 cutoff;

	/* Compare calendar days while retaining the user's local "tomorrow". */
	cutoff = time(NULL) + 86400;
	if (time2tm(t, 1, &date) == 0 ||
	    time2tm(cutoff, 0, &tomorrow) == 0)
		return t > cutoff;
	return date.tm_year > tomorrow.tm_year ||
	    (date.tm_year == tomorrow.tm_year &&
	    date.tm_yday > tomorrow.tm_yday);
}

char *
mandoc_normdate(struct roff_node *nch, struct roff_node *nbl)
{
	char		*cp;
	time_t		 t;

	/* No date specified. */

	if (nch == NULL) {
		if (nbl == NULL)
			mandoc_msg(MANDOCERR_DATE_MISSING, 0, 0, NULL);
		else
			mandoc_msg(MANDOCERR_DATE_MISSING, nbl->line,
			    nbl->pos, "%s", roff_name[nbl->tok]);
		return mandoc_strdup("");
	}
	if (*nch->string == '\0') {
		mandoc_msg(MANDOCERR_DATE_MISSING, nch->line,
		    nch->pos, "%s", roff_name[nbl->tok]);
		return mandoc_strdup("");
	}
	if (strcmp(nch->string, "$" "Mdocdate$") == 0)
		return time2a(time(NULL), 0);

	/* Valid mdoc(7) date format. */

	if (a2time(&t, "$" "Mdocdate: %b %d %Y $", nch->string) ||
	    a2time(&t, "%b %d, %Y", nch->string)) {
		cp = time2a(t, 1);
		if (date_is_future(t))
			mandoc_msg(MANDOCERR_DATE_FUTURE, nch->line,
			    nch->pos, "%s %s", roff_name[nbl->tok], cp);
		else if (*nch->string != '$' &&
		    strcmp(nch->string, cp) != 0)
			mandoc_msg(MANDOCERR_DATE_NORM, nch->line,
			    nch->pos, "%s %s", roff_name[nbl->tok], cp);
		return cp;
	}

	/* In man(7), do not warn about the legacy format. */

	if (a2time(&t, "%Y-%m-%d", nch->string) == 0)
		mandoc_msg(MANDOCERR_DATE_BAD, nch->line, nch->pos,
		    "%s %s", roff_name[nbl->tok], nch->string);
	else if (date_is_future(t))
		mandoc_msg(MANDOCERR_DATE_FUTURE, nch->line, nch->pos,
		    "%s %s", roff_name[nbl->tok], nch->string);
	else if (nbl->tok == MDOC_Dd)
		mandoc_msg(MANDOCERR_DATE_LEGACY, nch->line, nch->pos,
		    "Dd %s", nch->string);

	/* Use any non-mdoc(7) date verbatim. */

	return mandoc_strdup(nch->string);
}

int
mandoc_eos(const char *p, size_t sz)
{
	const char	*q;
	int		 enclosed, found;

	if (0 == sz)
		return 0;

	/*
	 * End-of-sentence recognition must include situations where
	 * some symbols, such as `)', allow prior EOS punctuation to
	 * propagate outward.
	 */

	enclosed = found = 0;
	for (q = p + (int)sz - 1; q >= p; q--) {
		switch (*q) {
		case '\"':
		case '\'':
		case ']':
		case ')':
			if (0 == found)
				enclosed = 1;
			break;
		case '.':
		case '!':
		case '?':
			found = 1;
			break;
		default:
			return found &&
			    (!enclosed || isalnum((unsigned char)*q));
		}
	}

	return found && !enclosed;
}

/*
 * Convert a string to a long that may not be <0.
 * If the string is invalid, or is less than 0, return -1.
 */
int
mandoc_strntoi(const char *p, size_t sz, int base)
{
	char		 buf[32];
	char		*ep;
	long		 v;

	if (sz > 31)
		return -1;

	memcpy(buf, p, sz);
	buf[(int)sz] = '\0';

	errno = 0;
	v = strtol(buf, &ep, base);

	if (buf[0] == '\0' || *ep != '\0')
		return -1;

	if (v > INT_MAX)
		v = INT_MAX;
	if (v < INT_MIN)
		v = INT_MIN;

	return (int)v;
}
