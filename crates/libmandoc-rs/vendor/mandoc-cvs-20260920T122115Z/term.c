/* $Id: term.c,v 1.295 2026/01/06 21:16:38 schwarze Exp $ */
/*
 * Copyright (c) 2010-2022, 2025, 2026 Ingo Schwarze <schwarze@openbsd.org>
 * Copyright (c) 2008, 2009, 2010, 2011 Kristaps Dzonsons <kristaps@bsd.lv>
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

#include <assert.h>
#include <ctype.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "mandoc.h"
#include "mandoc_aux.h"
#include "out.h"
#include "term.h"
#include "main.h"

static	size_t		 cond_width(const struct termp *, int, int *);
static	void		 adjbuf(struct termp *, size_t);
static	void		 bufferc(struct termp *, char,
				enum term_collector_reason);
static	void		 directc(struct termp *, int,
				enum term_collector_reason);
static	void		 encode(struct termp *, const char *, size_t,
				enum term_collector_reason);
static	void		 encode1(struct termp *, int,
				enum term_collector_reason);
static	void		 endline(struct termp *, enum term_collector_reason);
static	void		 term_field(struct termp *, size_t, size_t);
static	void		 term_fill(struct termp *, size_t *, size_t *,
				size_t);
static	void		 collect_emit(struct termp *, enum term_collector_op,
				enum term_collector_phase,
				enum term_collector_reason, size_t, size_t,
				size_t, int, int, enum termfont);
static	void		 buffer_write(struct termp *, size_t, int,
				enum term_collector_reason, enum termfont);
static	void		 logical_emit(struct termp *, int,
				enum term_collector_reason);


static void
collect_emit(struct termp *p, enum term_collector_op op,
		enum term_collector_phase phase,
		enum term_collector_reason reason, size_t pos, size_t end,
		size_t visual, int value, int previous, enum termfont font)
{
	struct term_collector_event ev;

	if (p->collector == NULL)
		return;
	memset(&ev, 0, sizeof(ev));
	ev.op = op;
	ev.phase = phase;
	ev.reason = reason;
	ev.node = p->collector_node;
	ev.column = p->tcol == NULL ? 0 : (size_t)(p->tcol - p->tcols);
	ev.pos = pos;
	ev.end = end;
	ev.visual = visual;
	ev.value = value;
	ev.previous = previous;
	ev.font = font;
	(*p->collector)(p, p->collector_arg, &ev);
}

void
term_setcollector(struct termp *p, term_collector collector, void *arg)
{
	p->collector = collector;
	p->collector_arg = arg;
	if (collector != NULL)
		collect_emit(p, TERM_COLLECT_COL_SELECT, TERM_COLLECT_ENTER,
		    TERM_COLLECT_NONE, 0, p->lasttcol + 1, 0, 0, 0,
		    TERMFONT_NONE);
}

void
term_collect_node(struct termp *p, const struct roff_node *n,
		enum term_collector_phase phase)
{
	const struct roff_node *saved;

	if (p->collector == NULL)
		return;
	saved = p->collector_node;
	p->collector_node = n;
	collect_emit(p, TERM_COLLECT_NODE, phase, TERM_COLLECT_NONE,
	    0, 0, 0, 0, 0, TERMFONT_NONE);
	p->collector_node = saved;
}

void
term_collect_table_cell(struct termp *p, const struct tbl_dat *cell,
		enum term_collector_phase phase)
{
	struct term_collector_event ev;

	if (p->collector == NULL)
		return;
	memset(&ev, 0, sizeof(ev));
	ev.op = TERM_COLLECT_TABLE_CELL;
	ev.phase = phase;
	ev.node = p->collector_node;
	ev.cell = cell;
	ev.column = p->tcol == NULL ? 0 : (size_t)(p->tcol - p->tcols);
	(*p->collector)(p, p->collector_arg, &ev);
}

static void
buffer_write(struct termp *p, size_t pos, int value,
		enum term_collector_reason reason, enum termfont font)
{
	int previous;

	previous = pos < p->tcol->lastcol ? p->tcol->buf[pos] : 0;
	collect_emit(p, TERM_COLLECT_BUFFER_WRITE, TERM_COLLECT_ENTER,
	    reason, pos, pos + 1, 0, value, previous, font);
	p->tcol->buf[pos] = value;
}

static void
logical_emit(struct termp *p, int value, enum term_collector_reason reason)
{
	collect_emit(p, TERM_COLLECT_LOGICAL, TERM_COLLECT_ENTER,
	    reason, p->col, p->col, 0, value, 0,
	    p->fontq[p->fonti]);
}


void
term_setcol(struct termp *p, size_t maxtcol)
{
	collect_emit(p, TERM_COLLECT_COL_SELECT, TERM_COLLECT_ENTER,
	    TERM_COLLECT_NONE, 0, maxtcol, 0, 0, 0, TERMFONT_NONE);
	if (maxtcol > p->maxtcol) {
		collect_emit(p, TERM_COLLECT_COL_RESIZE, TERM_COLLECT_ENTER,
		    TERM_COLLECT_NONE, p->maxtcol, maxtcol, 0, 0, 0,
		    TERMFONT_NONE);
		p->tcols = mandoc_recallocarray(p->tcols,
		    p->maxtcol, maxtcol, sizeof(*p->tcols));
		p->maxtcol = maxtcol;
	}
	p->lasttcol = maxtcol - 1;
	p->tcol = p->tcols;
}

void
term_free(struct termp *p)
{
	term_tab_free();
	for (p->tcol = p->tcols; p->tcol < p->tcols + p->maxtcol; p->tcol++) {
		collect_emit(p, TERM_COLLECT_COL_FREE, TERM_COLLECT_ENTER,
		    TERM_COLLECT_NONE, 0, p->tcol->maxcols, 0, 0, 0,
		    TERMFONT_NONE);
		free(p->tcol->buf);
	}
	free(p->tcols);
	free(p->fontq);
	free(p);
}

void
term_begin(struct termp *p, term_margin head,
		term_margin foot, const struct roff_meta *arg)
{

	p->headf = head;
	p->footf = foot;
	p->argf = arg;
	collect_emit(p, TERM_COLLECT_OUTPUT, TERM_COLLECT_ENTER,
	    TERM_COLLECT_HEADER, 0, 0, 0, 0, 0, TERMFONT_NONE);
	(*p->begin)(p);
	collect_emit(p, TERM_COLLECT_OUTPUT, TERM_COLLECT_LEAVE,
	    TERM_COLLECT_HEADER, 0, 0, 0, 0, 0, TERMFONT_NONE);
}

void
term_end(struct termp *p)
{

	collect_emit(p, TERM_COLLECT_OUTPUT, TERM_COLLECT_ENTER,
	    TERM_COLLECT_FOOTER, 0, 0, 0, 0, 0, TERMFONT_NONE);
	(*p->end)(p);
	collect_emit(p, TERM_COLLECT_OUTPUT, TERM_COLLECT_LEAVE,
	    TERM_COLLECT_FOOTER, 0, 0, 0, 0, 0, TERMFONT_NONE);
}

/*
 * Flush a chunk of text.  By default, break the output line each time
 * the right margin is reached, and continue output on the next line
 * at the same offset as the chunk itself.  By default, also break the
 * output line at the end of the chunk.  There are many flags modifying
 * this behaviour, see the comments in the body of the function.
 */
void
term_flushln(struct termp *p)
{
	/* Widths in basic units. */
	size_t	 vbl;      /* Whitespace to prepend to the output. */
	size_t	 vbr;      /* Actual visual position of the end of field. */
	size_t	 vfield;   /* Desired visual field width. */
	size_t	 vtarget;  /* Desired visual position of the right margin. */

	/* Bytes. */
	size_t	 ic;       /* Byte index in the input buffer. */
	size_t	 nbr;      /* Number of bytes to print in this field. */

	/*
	 * Normally, start writing at the left margin, but with the
	 * NOPAD flag, start writing at the current position instead.
	 */

	vbl = (p->flags & TERMP_NOPAD) || p->tcol->offset < p->viscol ?
	    0 : p->tcol->offset - p->viscol;
	if (p->minbl > 0 && vbl < term_len(p, p->minbl))
		vbl = term_len(p, p->minbl);

	if ((p->flags & TERMP_MULTICOL) == 0)
		p->tcol->col = 0;

	/* Loop over output lines. */

	for (;;) {
		vfield = p->tcol->rmargin > p->viscol + vbl ?
		    p->tcol->rmargin - p->viscol - vbl : 0;

		/*
		 * Normally, break the line at the the right margin
		 * of the field, but with the NOBREAK flag, only
		 * break it at the max right margin of the screen,
		 * and with the BRNEVER flag, never break it at all.
		 */

		vtarget = (p->flags & TERMP_NOBREAK) == 0 ? vfield :
		    p->maxrmargin > p->viscol + vbl ?
		    p->maxrmargin - p->viscol - vbl : 0;

		/*
		 * Figure out how much text will fit in the field.
		 * If there is whitespace only, print nothing.
		 */

		term_fill(p, &nbr, &vbr,
		    p->flags & TERMP_BRNEVER ? SIZE_MAX / 2 : vtarget);
		if (nbr == 0)
			break;

		/*
		 * With the CENTER or RIGHT flag, increase the indentation
		 * to center the text between the left and right margins
		 * or to adjust it to the right margin, respectively.
		 */

		if (vbr < vtarget) {
			if (p->flags & TERMP_CENTER)
				vbl += (vtarget - vbr) / 2;
			else if (p->flags & TERMP_RIGHT)
				vbl += vtarget - vbr;
		}

		/* Finally, print the field content. */

		term_field(p, vbl, nbr);
		if (vbr < vtarget)
			p->tcol->taboff += vbr;
		else
			p->tcol->taboff += vtarget;
		p->tcol->taboff += term_len(p, 1);

		/*
		 * If there is no text left in the field, exit the loop.
		 * If the BRTRSP flag is set, consider trailing
		 * whitespace significant when deciding whether
		 * the field fits or not.
		 */

		for (ic = p->tcol->col; ic < p->tcol->lastcol; ic++) {
			switch (p->tcol->buf[ic]) {
			case '\t':
				if (p->flags & TERMP_BRTRSP)
					vbr = term_tab_next(vbr);
				continue;
			case ' ':
				if (p->flags & TERMP_BRTRSP)
					vbr += term_len(p, 1);
				continue;
			case '\n':
			case ASCII_NBRZW:
			case ASCII_BREAK:
			case ASCII_TABREF:
				continue;
			default:
				break;
			}
			break;
		}
		if (ic == p->tcol->lastcol)
			break;

		/*
		 * At the location of an automatic line break, input
		 * space characters are consumed by the line break.
		 */

		ic = p->tcol->col;
		while (ic < p->tcol->lastcol && p->tcol->buf[ic] == ' ')
			ic++;
		if (ic != p->tcol->col) {
			collect_emit(p, TERM_COLLECT_BUFFER_CONSUME,
			    TERM_COLLECT_ENTER, TERM_COLLECT_WRAP,
			    p->tcol->col, ic, 0, 0, 0, TERMFONT_NONE);
			p->tcol->col = ic;
		}

		/*
		 * In multi-column mode, leave the rest of the text
		 * in the buffer to be handled by a subsequent
		 * invocation, such that the other columns of the
		 * table can be handled first.
		 * In single-column mode, simply break the line.
		 */

		if (p->flags & TERMP_MULTICOL)
			return;

		endline(p, TERM_COLLECT_WRAP);

		/*
		 * Normally, start the next line at the same indentation
		 * as this one, but with the BRIND flag, start it at the
		 * right margin instead.  This is used together with
		 * NOBREAK for the tags in various kinds of tagged lists.
		 */

		vbl = p->flags & TERMP_BRIND ?
		    p->tcol->rmargin : p->tcol->offset;
	}

	/* Reset output state in preparation for the next field. */

	collect_emit(p, TERM_COLLECT_BUFFER_RESET, TERM_COLLECT_ENTER,
	    TERM_COLLECT_FINAL, 0, p->tcol->lastcol, p->col,
	    0, 0, TERMFONT_NONE);
	p->col = p->tcol->col = p->tcol->lastcol = 0;
	p->minbl = p->trailspace;
	p->flags &= ~(TERMP_BACKAFTER | TERMP_BACKBEFORE | TERMP_NOPAD);

	if (p->flags & TERMP_MULTICOL)
		return;

	/*
	 * The HANG flag means that the next field
	 * always follows on the same line.
	 * The NOBREAK flag means that the next field
	 * follows on the same line unless the field was overrun.
	 * Normally, break the line at the end of each field.
	 */

	if ((p->flags & TERMP_HANG) == 0 &&
	    ((p->flags & TERMP_NOBREAK) == 0 ||
	     vbr + term_len(p, p->trailspace) > vfield + term_len(p, 1) / 2))
		endline(p, TERM_COLLECT_FINAL);
}

/*
 * Store the number of input bytes to print in this field in *nbr
 * and their total visual width in basic units in *vbr.
 * If there is only whitespace in the field, both remain zero.
 * The desired visual width of the field is provided by vtarget.
 * If the first word is longer, the field will be overrun.
 */
static void
term_fill(struct termp *p, size_t *nbr, size_t *vbr, size_t vtarget)
{
	/* Widths in basic units. */
	size_t	 vis;       /* Visual position of the current character. */
	size_t	 vn;        /* Visual position of the next character. */
	size_t	 enw;       /* Width of an EN unit. */
	int	 taboff;    /* Temporary offset for literal tabs. */

	size_t	 ic;        /* Byte index in the input buffer. */
	int	 breakline; /* Break at the end of this word. */
	int	 graph;     /* Last character was non-blank. */

	*nbr = *vbr = vis = 0;
	breakline = graph = 0;
	taboff = p->tcol->taboff;
	enw = (*p->getwidth)(p, ' ');
	vtarget += enw / 2;
	for (ic = p->tcol->col; ic < p->tcol->lastcol; ic++) {
		switch (p->tcol->buf[ic]) {
		case '\b':  /* Escape \o (overstrike) or backspace markup. */
			assert(ic > 0);
			vis -= (*p->getwidth)(p, p->tcol->buf[ic - 1]);
			continue;

		case ' ':
		case ASCII_BREAK:  /* Escape \: (breakpoint). */
			vn = vis;
			if (p->tcol->buf[ic] == ' ')
				vn += enw;
			/* Can break at the end of a word. */
			if (breakline || vn > vtarget)
				break;
			if (graph) {
				*nbr = ic;
				*vbr = vis;
				graph = 0;
			}
			vis = vn;
			continue;

		case '\n':  /* Escape \p (break at the end of the word). */
			breakline = 1;
			continue;

		case ASCII_HYPH:  /* Breakable hyphen. */
			graph = 1;
			/*
			 * We are about to decide whether to break the
			 * line or not, so we no longer need this hyphen
			 * to be marked as breakable.  Put back a real
			 * hyphen such that we get the correct width.
			 */
			buffer_write(p, ic, '-', TERM_COLLECT_NORMALIZE,
			    TERMFONT_NONE);
			vis += (*p->getwidth)(p, '-');
			if (vis > vtarget) {
				ic++;
				break;
			}
			*nbr = ic + 1;
			*vbr = vis;
			continue;

		case ASCII_TABREF:
			taboff = -vis - enw;
			continue;

		default:
			switch (p->tcol->buf[ic]) {
			case '\t':
				if (taboff < 0 && (size_t)-taboff > vis)
					vis = 0;
				else
					vis += taboff;
				vis = term_tab_next(vis);
				vis -= taboff;
				break;
			case ASCII_NBRZW:  /* Non-breakable zero-width. */
				break;
			case ASCII_NBRSP:  /* Non-breakable space. */
				buffer_write(p, ic, ' ', TERM_COLLECT_NORMALIZE,
				    TERMFONT_NONE);
				/* FALLTHROUGH */
			default:  /* Printable character. */
				vis += (*p->getwidth)(p, p->tcol->buf[ic]);
				break;
			}
			graph = 1;
			if (vis > vtarget && *nbr > 0)
				return;
			continue;
		}
		break;
	}

	/*
	 * If the last word extends to the end of the field without any
	 * trailing whitespace, the loop could not check yet whether it
	 * can remain on this line.  So do the check now.
	 */

	if (graph && (vis <= vtarget || *nbr == 0)) {
		*nbr = ic;
		*vbr = vis;
	}
}

/*
 * Print the contents of one field
 * with an indentation        of  vbl  basic units
 * and an input string length of  nbr  bytes.
 */
static void
term_field(struct termp *p, size_t vbl, size_t nbr)
{
	/* Widths in basic units. */
	size_t	 vis;	/* Visual position of the current character. */
	size_t	 vt;	/* Visual position including tab offset. */
	size_t	 dv;	/* Visual width of the current character. */
	int	 taboff; /* Temporary offset for literal tabs. */

	size_t	 ic;	/* Byte position in the input buffer. */

	vis = 0;
	taboff = p->tcol->taboff;
	for (ic = p->tcol->col; ic < nbr; ic++) {

		/*
		 * To avoid the printing of trailing whitespace,
		 * do not print whitespace right away, only count it.
		 */

		switch (p->tcol->buf[ic]) {
		case '\n':
		case ASCII_BREAK:
		case ASCII_NBRZW:
			collect_emit(p, TERM_COLLECT_FIELD_SKIP,
			    TERM_COLLECT_ENTER, TERM_COLLECT_FIELD,
			    ic, ic + 1, 0, p->tcol->buf[ic], 0,
			    TERMFONT_NONE);
			continue;
		case ASCII_TABREF:
			collect_emit(p, TERM_COLLECT_FIELD_SKIP,
			    TERM_COLLECT_ENTER, TERM_COLLECT_FIELD,
			    ic, ic + 1, 0, p->tcol->buf[ic], 0,
			    TERMFONT_NONE);
			taboff = -vis - (*p->getwidth)(p, ' ');
			continue;
		case '\t':
		case ' ':
		case ASCII_NBRSP:
			if (p->tcol->buf[ic] == '\t') {
				if (taboff < 0 && (size_t)-taboff > vis)
					vt = 0;
				else
					vt = vis + taboff;
				dv = term_tab_next(vt) - vt;
			} else
				dv = (*p->getwidth)(p, ' ');
			collect_emit(p, TERM_COLLECT_FIELD_SKIP,
			    TERM_COLLECT_ENTER, TERM_COLLECT_FIELD,
			    ic, ic + 1, dv, p->tcol->buf[ic], 0,
			    TERMFONT_NONE);
			vbl += dv;
			vis += dv;
			continue;
		default:
			break;
		}

		/*
		 * We found a non-blank character to print,
		 * so write preceding white space now.
		 */

		if (vbl > 0) {
			(*p->advance)(p, vbl);
			vbl = 0;
		}

		/* Print the character and adjust the visual position. */

		collect_emit(p, TERM_COLLECT_FIELD_PLACE,
		    TERM_COLLECT_ENTER, TERM_COLLECT_FIELD,
		    ic, ic + 1, (*p->getwidth)(p, p->tcol->buf[ic]),
		    p->tcol->buf[ic], 0, TERMFONT_NONE);
		(*p->letter)(p, p->tcol->buf[ic]);
		if (p->tcol->buf[ic] == '\b') {
			dv = (*p->getwidth)(p, p->tcol->buf[ic - 1]);
			p->viscol -= dv;
			vis -= dv;
		} else {
			dv = (*p->getwidth)(p, p->tcol->buf[ic]);
			p->viscol += dv;
			vis += dv;
		}
	}
	collect_emit(p, TERM_COLLECT_BUFFER_CONSUME, TERM_COLLECT_ENTER,
	    TERM_COLLECT_FIELD, p->tcol->col, nbr, 0, 0, 0,
	    TERMFONT_NONE);
	p->tcol->col = nbr;
}

/*
 * Print the margin character, if one is configured,
 * and end the output line.
 */
static void
endline(struct termp *p, enum term_collector_reason reason)
{
	if ((p->flags & (TERMP_NEWMC | TERMP_ENDMC)) == TERMP_ENDMC) {
		p->mc = NULL;
		p->flags &= ~TERMP_ENDMC;
	}
	if (p->mc != NULL) {
		if (p->viscol > 0 && p->viscol <= p->maxrmargin)
			(*p->advance)(p,
			    p->maxrmargin - p->viscol + term_len(p, 1));
		collect_emit(p, TERM_COLLECT_OUTPUT, TERM_COLLECT_ENTER,
		    TERM_COLLECT_MARGIN, 0, 0, 0, 0, 0, TERMFONT_NONE);
		p->flags |= TERMP_NOBUF | TERMP_NOSPACE;
		term_word_node(p, p->mc, NULL);
		p->flags &= ~(TERMP_NOBUF | TERMP_NEWMC);
		collect_emit(p, TERM_COLLECT_OUTPUT, TERM_COLLECT_LEAVE,
		    TERM_COLLECT_MARGIN, 0, 0, 0, 0, 0, TERMFONT_NONE);
	}
	collect_emit(p, TERM_COLLECT_ENDLINE, TERM_COLLECT_ENTER,
	    reason, 0, 0, p->viscol, 0, 0, TERMFONT_NONE);
	(*p->endline)(p);
}

/*
 * A newline only breaks an existing line; it won't assert vertical
 * space.  All data in the output buffer is flushed prior to the newline
 * assertion.
 */
void
term_newln(struct termp *p)
{
	p->flags |= TERMP_NOSPACE;
	if (p->tcol->lastcol || p->viscol)
		term_flushln(p);
	p->tcol->taboff = 0;
}

/*
 * Asserts a vertical space (a full, empty line-break between lines).
 * Note that if used twice, this will cause two blank spaces and so on.
 * All data in the output buffer is flushed prior to the newline
 * assertion.
 */
void
term_vspace(struct termp *p)
{

	term_newln(p);
	if (0 < p->skipvsp)
		p->skipvsp--;
	else {
		collect_emit(p, TERM_COLLECT_ENDLINE, TERM_COLLECT_ENTER,
		    TERM_COLLECT_FINAL, 0, 0, p->viscol, 0, 0,
		    TERMFONT_NONE);
		(*p->endline)(p);
	}
}

/* Swap current and previous font; for \fP and .ft P */
void
term_fontlast(struct termp *p)
{
	enum termfont	 f;

	f = p->fontl;
	p->fontl = p->fontq[p->fonti];
	p->fontq[p->fonti] = f;
}

/* Set font, save current, discard previous; for \f, .ft, and man(7). */
void
term_fontrepl(struct termp *p, enum termfont f)
{
	p->fontl = p->fontq[p->fonti];
	if (p->fontibi && f == TERMFONT_UNDER)
		f = TERMFONT_BI;
	p->fontq[p->fonti] = f;
}

/* Set font, save previous; for mdoc(7), eqn(7), and tbl(7). */
void
term_fontpush(struct termp *p, enum termfont f)
{
	enum termfont	 fl;

	fl = p->fontq[p->fonti];
	if (++p->fonti == p->fontsz) {
		p->fontsz += 8;
		p->fontq = mandoc_reallocarray(p->fontq,
		    p->fontsz, sizeof(*p->fontq));
	}
	p->fontq[p->fonti] = fl;
	term_fontrepl(p, f);
}

/* Flush to make the saved pointer current again. */
void
term_fontpopq(struct termp *p, int i)
{
	assert(i >= 0);
	if (p->fonti > i)
		p->fonti = i;
}

/* Pop one font off the stack. */
void
term_fontpop(struct termp *p)
{
	assert(p->fonti > 0);
	p->fonti--;
}

/*
 * Handle pwords, partial words, which may be either a single word or a
 * phrase that cannot be broken down (such as a literal string).  This
 * handles word styling.
 */
void
term_word(struct termp *p, const char *word)
{
	struct roffsu	 su;
	const char	 nbrsp[2] = { ASCII_NBRSP, 0 };
	const char	*seq;		/* Escape sequence argument. */
	const char	*cp;		/* String to be printed. */
	size_t		 csz;		/* String length in basic units. */
	size_t		 lsz;		/* Line width in basic units. */
	size_t		 ssz = 0;	/* Substring length in bytes. */
	int		 sz;		/* Argument length in bytes. */
	int		 uc;		/* Unicode codepoint number. */
	int		 bu;		/* Width in basic units. */
	enum mandoc_esc	 esc;

	if ((p->flags & TERMP_NOBUF) == 0) {
		if ((p->flags & TERMP_NOSPACE) == 0) {
			if ((p->flags & TERMP_KEEP) == 0) {
				bufferc(p, ' ', TERM_COLLECT_AUTO_SPACE);
				if (p->flags & TERMP_SENTENCE)
					bufferc(p, ' ', TERM_COLLECT_AUTO_SPACE);
			} else
				bufferc(p, ASCII_NBRSP, TERM_COLLECT_KEEP_SPACE);
		}
		if (p->flags & TERMP_PREKEEP)
			p->flags |= TERMP_KEEP;
		if (p->flags & TERMP_NONOSPACE)
			p->flags |= TERMP_NOSPACE;
		else
			p->flags &= ~TERMP_NOSPACE;
		p->flags &= ~(TERMP_SENTENCE | TERMP_NONEWLINE);
		p->skipvsp = 0;
	}

	while ('\0' != *word) {
		if ('\\' != *word) {
			if (TERMP_NBRWORD & p->flags) {
				if (' ' == *word) {
					encode(p, nbrsp, 1, TERM_COLLECT_KEEP_SPACE);
					word++;
					continue;
				}
				ssz = strcspn(word, "\\ ");
			} else
				ssz = strcspn(word, "\\");
			encode(p, word, ssz, TERM_COLLECT_TEXT);
			word += (int)ssz;
			continue;
		}

		word++;
		esc = mandoc_escape(&word, &seq, &sz);
		switch (esc) {
		case ESCAPE_UNICODE:
			uc = mchars_num2uc(seq + 1, sz - 1);
			break;
		case ESCAPE_NUMBERED:
			uc = mchars_num2char(seq, sz);
			if (uc >= 0)
				break;
			bufferc(p, ASCII_NBRZW, TERM_COLLECT_ESCAPE);
			continue;
		case ESCAPE_SPECIAL:
			if (p->enc == TERMENC_ASCII) {
				cp = mchars_spec2str(seq, sz, &ssz);
				uc = mchars_spec2cp(seq, sz);
				if (cp != NULL) {
					if (uc > 0) {
						logical_emit(p, uc,
						    TERM_COLLECT_ESCAPE);
						encode(p, cp, ssz,
						    TERM_COLLECT_PROJECTION);
					} else
						encode(p, cp, ssz,
						    TERM_COLLECT_ESCAPE);
				}
				else
					bufferc(p, ASCII_NBRZW, TERM_COLLECT_ESCAPE);
			} else {
				uc = mchars_spec2cp(seq, sz);
				if (uc > 0)
					encode1(p, uc, TERM_COLLECT_ESCAPE);
				else
					bufferc(p, ASCII_NBRZW, TERM_COLLECT_ESCAPE);
			}
			continue;
		case ESCAPE_UNDEF:
			uc = *seq;
			break;
		case ESCAPE_FONTBOLD:
		case ESCAPE_FONTCB:
			term_fontrepl(p, TERMFONT_BOLD);
			continue;
		case ESCAPE_FONTITALIC:
		case ESCAPE_FONTCI:
			term_fontrepl(p, TERMFONT_UNDER);
			continue;
		case ESCAPE_FONTBI:
			term_fontrepl(p, TERMFONT_BI);
			continue;
		case ESCAPE_FONT:
		case ESCAPE_FONTCR:
		case ESCAPE_FONTROMAN:
			term_fontrepl(p, TERMFONT_NONE);
			continue;
		case ESCAPE_FONTPREV:
			term_fontlast(p);
			continue;
		case ESCAPE_BREAK:
			bufferc(p, '\n', TERM_COLLECT_ESCAPE);
			continue;
		case ESCAPE_NOSPACE:
			if (p->flags & TERMP_BACKAFTER)
				p->flags &= ~TERMP_BACKAFTER;
			else if (*word == '\0')
				p->flags |= (TERMP_NOSPACE | TERMP_NONEWLINE);
			continue;
		case ESCAPE_DEVICE:
			if (p->type == TERMTYPE_PDF)
				encode(p, "pdf", 3, TERM_COLLECT_ESCAPE);
			else if (p->type == TERMTYPE_PS)
				encode(p, "ps", 2, TERM_COLLECT_ESCAPE);
			else if (p->enc == TERMENC_ASCII)
				encode(p, "ascii", 5, TERM_COLLECT_ESCAPE);
			else
				encode(p, "utf8", 4, TERM_COLLECT_ESCAPE);
			continue;
		case ESCAPE_HORIZ:
			if (p->flags & TERMP_BACKAFTER) {
				p->flags &= ~TERMP_BACKAFTER;
				continue;
			}
			if (*seq == '|') {
				seq++;
				bu = -term_len(p, p->col);
			} else
				bu = 0;
			if (a2roffsu(seq, &su, SCALE_EM) == NULL)
				continue;
			bu += term_hspan(p, &su);
			if (bu >= 0) {
				while (bu > 0) {
					bu -= term_len(p, 1);
					if (p->flags & TERMP_BACKBEFORE)
						p->flags &= ~TERMP_BACKBEFORE;
					else
						bufferc(p, ASCII_NBRSP, TERM_COLLECT_HORIZ);
				}
				continue;
			}
			if (p->flags & TERMP_BACKBEFORE) {
				p->flags &= ~TERMP_BACKBEFORE;
				assert(p->col > 1);
				collect_emit(p, TERM_COLLECT_BUFFER_CURSOR,
				    TERM_COLLECT_ENTER, TERM_COLLECT_HORIZ,
				    p->col, p->col - 1, 0, 0, 0,
				    TERMFONT_NONE);
				p->col--;
			}
			if (term_len(p, p->col) >= (size_t)(-bu)) {
				collect_emit(p, TERM_COLLECT_BUFFER_CURSOR,
				    TERM_COLLECT_ENTER, TERM_COLLECT_HORIZ,
				    p->col, p->col - -bu / term_len(p, 1),
				    0, 0, 0, TERMFONT_NONE);
				p->col -= -bu / term_len(p, 1);
			} else {
				bu += term_len(p, p->col);
				collect_emit(p, TERM_COLLECT_BUFFER_CURSOR,
				    TERM_COLLECT_ENTER, TERM_COLLECT_HORIZ,
				    p->col, 0, 0, 0, 0, TERMFONT_NONE);
				p->col = 0;
				if (p->tcol->offset > (size_t)(-bu)) {
					p->ti += bu;
					p->tcol->offset += bu;
				} else {
					p->ti -= p->tcol->offset;
					p->tcol->offset = 0;
				}
			}
			continue;
		case ESCAPE_HLINE:
			if ((cp = a2roffsu(seq, &su, SCALE_EM)) == NULL)
				continue;
			bu = term_hspan(p, &su);
			if (bu <= 0) {
				if (p->tcol->rmargin <= p->tcol->offset)
					continue;
				lsz = p->tcol->rmargin - p->tcol->offset;
			} else
				lsz = bu;
			if (*cp == seq[-1])
				uc = -1;
			else if (*cp == '\\') {
				seq = cp + 1;
				esc = mandoc_escape(&seq, &cp, &sz);
				switch (esc) {
				case ESCAPE_UNICODE:
					uc = mchars_num2uc(cp + 1, sz - 1);
					break;
				case ESCAPE_NUMBERED:
					uc = mchars_num2char(cp, sz);
					break;
				case ESCAPE_SPECIAL:
					uc = mchars_spec2cp(cp, sz);
					break;
				case ESCAPE_UNDEF:
					uc = *seq;
					break;
				default:
					uc = -1;
					break;
				}
			} else
				uc = *cp;
			if (uc < 0x20 || (uc > 0x7E && uc < 0xA0))
				uc = '_';
			if (p->enc == TERMENC_ASCII) {
				cp = ascii_uc2str(uc);
				csz = term_strlen(p, cp);
				ssz = strlen(cp);
			} else
				csz = (*p->getwidth)(p, uc);
			while (lsz > 0) {
				if (p->enc == TERMENC_ASCII) {
					logical_emit(p, uc, TERM_COLLECT_ESCAPE);
					encode(p, cp, ssz,
					    TERM_COLLECT_PROJECTION);
				}
				else
					encode1(p, uc, TERM_COLLECT_ESCAPE);
				if (lsz > csz)
					lsz -= csz;
				else
					lsz = 0;
			}
			continue;
		case ESCAPE_SKIPCHAR:
			p->flags |= TERMP_BACKAFTER;
			continue;
		case ESCAPE_OVERSTRIKE:
			cp = seq + sz;
			while (seq < cp) {
				if (*seq == '\\') {
					mandoc_escape(&seq, NULL, NULL);
					continue;
				}
				encode1(p, *seq++, TERM_COLLECT_OVERSTRIKE);
				if (seq < cp) {
					if (p->flags & TERMP_BACKBEFORE)
						p->flags |= TERMP_BACKAFTER;
					else
						p->flags |= TERMP_BACKBEFORE;
				}
			}
			/* Trim trailing backspace/blank pair. */
			if (p->tcol->lastcol > 2 &&
			    (p->tcol->buf[p->tcol->lastcol - 1] == ' ' ||
			     p->tcol->buf[p->tcol->lastcol - 1] == '\t')) {
				collect_emit(p, TERM_COLLECT_BUFFER_TRUNCATE,
				    TERM_COLLECT_ENTER, TERM_COLLECT_OVERSTRIKE,
				    p->tcol->lastcol - 2, p->tcol->lastcol,
				    0, 0, 0, TERMFONT_NONE);
				p->tcol->lastcol -= 2;
			}
			if (p->col > p->tcol->lastcol) {
				collect_emit(p, TERM_COLLECT_BUFFER_CURSOR,
				    TERM_COLLECT_ENTER, TERM_COLLECT_OVERSTRIKE,
				    p->col, p->tcol->lastcol, 0, 0, 0,
				    TERMFONT_NONE);
				p->col = p->tcol->lastcol;
			}
			continue;
		case ESCAPE_IGNORE:
			bufferc(p, ASCII_NBRZW, TERM_COLLECT_ESCAPE);
			continue;
		default:
			continue;
		}

		/*
		 * Common handling for Unicode and numbered
		 * character escape sequences.
		 */

		if (p->enc == TERMENC_ASCII) {
			cp = ascii_uc2str(uc);
			logical_emit(p, uc, TERM_COLLECT_ESCAPE);
			encode(p, cp, strlen(cp), TERM_COLLECT_PROJECTION);
		} else {
			if ((uc < 0x20 && uc != 0x09) ||
			    (uc > 0x7E && uc < 0xA0))
				uc = 0xFFFD;
			encode1(p, uc, TERM_COLLECT_ESCAPE);
		}
	}
	p->flags &= ~TERMP_NBRWORD;
}

void
term_word_node(struct termp *p, const char *word, const struct roff_node *n)
{
	const struct roff_node *saved;

	saved = p->collector_node;
	p->collector_node = n;
	term_word(p, word);
	p->collector_node = saved;
}

static void
adjbuf(struct termp *p, size_t sz)
{
	struct termp_col *c;
	size_t newmax;

	c = p->tcol;
	newmax = c->maxcols == 0 ? 1024 : c->maxcols;
	while (newmax <= sz)
		newmax <<= 2;
	collect_emit(p, TERM_COLLECT_BUFFER_GROW, TERM_COLLECT_ENTER,
	    TERM_COLLECT_NONE, c->maxcols, newmax, 0, 0, 0,
	    TERMFONT_NONE);
	c->maxcols = newmax;
	c->buf = mandoc_reallocarray(c->buf, c->maxcols, sizeof(*c->buf));
}

/*
 * Without the line buffer, layout sentinels still have their usual visible
 * meaning.  In particular, unknown margin characters are zero-width, not
 * literal ASCII_NBRZW bytes.  Tab-reference state is handled by term_field();
 * term_tab_ref() never inserts it while TERMP_NOBUF is set.
 */
static void
directc(struct termp *p, int c, enum term_collector_reason reason)
{
	collect_emit(p, TERM_COLLECT_DIRECT, TERM_COLLECT_ENTER,
	    reason, 0, 0, 0, c, 0, p->fontq[p->fonti]);
	switch (c) {
	case ASCII_BREAK:
	case ASCII_NBRZW:
		return;
	case ASCII_NBRSP:
		c = ' ';
		break;
	case ASCII_HYPH:
		c = '-';
		break;
	default:
		break;
	}
	(*p->letter)(p, c);
}

static void
bufferc(struct termp *p, char c, enum term_collector_reason reason)
{
	if (reason != TERM_COLLECT_HORIZ && reason != TERM_COLLECT_FIELD)
		logical_emit(p, (unsigned char)c, reason);
	if (p->flags & TERMP_NOBUF) {
		directc(p, c, reason);
		return;
	}
	if (p->col + 1 >= p->tcol->maxcols)
		adjbuf(p, p->col + 1);
	if (p->tcol->lastcol <= p->col || (c != ' ' && c != ASCII_NBRSP))
		buffer_write(p, p->col, c, reason, p->fontq[p->fonti]);
	collect_emit(p, TERM_COLLECT_BUFFER_CURSOR, TERM_COLLECT_ENTER,
	    reason, p->col, p->col + 1, 0, 0, 0, TERMFONT_NONE);
	if (p->tcol->lastcol < ++p->col)
		p->tcol->lastcol = p->col;
}

void
term_tab_ref(struct termp *p)
{
	if (p->tcol->lastcol && p->tcol->lastcol <= p->col &&
	    (p->flags & TERMP_NOBUF) == 0)
		bufferc(p, ASCII_TABREF, TERM_COLLECT_FIELD);
}

/*
 * See encode().
 * Do this for a single (probably unicode) value.
 * Does not check for non-decorated glyphs.
 */
static void
encode1(struct termp *p, int c, enum term_collector_reason reason)
{
	enum termfont	  f;

	if (reason != TERM_COLLECT_PROJECTION)
		logical_emit(p, c, reason);

	if (p->flags & TERMP_NOBUF) {
		directc(p, c, reason);
		return;
	}

	if (p->col + 7 >= p->tcol->maxcols)
		adjbuf(p, p->col + 7);

	f = (c == ASCII_HYPH || c > 127 || isgraph(c)) ?
	    p->fontq[p->fonti] : TERMFONT_NONE;

	if (p->flags & TERMP_BACKBEFORE) {
		if (p->tcol->buf[p->col - 1] == ' ' ||
		    p->tcol->buf[p->col - 1] == '\t') {
			collect_emit(p, TERM_COLLECT_BUFFER_CURSOR,
			    TERM_COLLECT_ENTER, TERM_COLLECT_OVERSTRIKE,
			    p->col, p->col - 1, 0, 0, 0,
			    TERMFONT_NONE);
			p->col--;
		} else {
			buffer_write(p, p->col, '\b',
			    TERM_COLLECT_OVERSTRIKE, TERMFONT_NONE);
			collect_emit(p, TERM_COLLECT_BUFFER_CURSOR,
			    TERM_COLLECT_ENTER, TERM_COLLECT_OVERSTRIKE,
			    p->col, p->col + 1, 0, 0, 0,
			    TERMFONT_NONE);
			p->col++;
		}
		p->flags &= ~TERMP_BACKBEFORE;
	}
	if (f == TERMFONT_UNDER || f == TERMFONT_BI) {
		buffer_write(p, p->col, '_', TERM_COLLECT_FONT, f);
		collect_emit(p, TERM_COLLECT_BUFFER_CURSOR,
		    TERM_COLLECT_ENTER, TERM_COLLECT_FONT,
		    p->col, p->col + 1, 0, 0, 0, TERMFONT_NONE);
		p->col++;
		buffer_write(p, p->col, '\b', TERM_COLLECT_FONT, f);
		collect_emit(p, TERM_COLLECT_BUFFER_CURSOR,
		    TERM_COLLECT_ENTER, TERM_COLLECT_FONT,
		    p->col, p->col + 1, 0, 0, 0, TERMFONT_NONE);
		p->col++;
	}
	if (f == TERMFONT_BOLD || f == TERMFONT_BI) {
		if (c == ASCII_HYPH)
			buffer_write(p, p->col, '-', TERM_COLLECT_FONT, f);
		else
			buffer_write(p, p->col, c, TERM_COLLECT_FONT, f);
		collect_emit(p, TERM_COLLECT_BUFFER_CURSOR,
		    TERM_COLLECT_ENTER, TERM_COLLECT_FONT,
		    p->col, p->col + 1, 0, 0, 0, TERMFONT_NONE);
		p->col++;
		buffer_write(p, p->col, '\b', TERM_COLLECT_FONT, f);
		collect_emit(p, TERM_COLLECT_BUFFER_CURSOR,
		    TERM_COLLECT_ENTER, TERM_COLLECT_FONT,
		    p->col, p->col + 1, 0, 0, 0, TERMFONT_NONE);
		p->col++;
	}
	if (p->tcol->lastcol <= p->col || (c != ' ' && c != ASCII_NBRSP))
		buffer_write(p, p->col, c, reason, f);
	collect_emit(p, TERM_COLLECT_BUFFER_CURSOR, TERM_COLLECT_ENTER,
	    reason, p->col, p->col + 1, 0, 0, 0, TERMFONT_NONE);
	if (p->tcol->lastcol < ++p->col)
		p->tcol->lastcol = p->col;
	if (p->flags & TERMP_BACKAFTER) {
		p->flags |= TERMP_BACKBEFORE;
		p->flags &= ~TERMP_BACKAFTER;
	}
}

static void
encode(struct termp *p, const char *word, size_t sz,
		enum term_collector_reason reason)
{
	size_t		  i;

	if (p->flags & TERMP_NOBUF) {
		for (i = 0; i < sz; i++)
			directc(p, word[i], reason);
		return;
	}

	if (p->col + 2 + (sz * 5) >= p->tcol->maxcols)
		adjbuf(p, p->col + 2 + (sz * 5));

	for (i = 0; i < sz; i++) {
		if (ASCII_HYPH == word[i] ||
		    isgraph((unsigned char)word[i]))
			encode1(p, word[i], reason);
		else {
			if (reason != TERM_COLLECT_PROJECTION)
				logical_emit(p, (unsigned char)word[i], reason);
			if (p->tcol->lastcol <= p->col ||
			    (word[i] != ' ' && word[i] != ASCII_NBRSP))
				buffer_write(p, p->col, word[i], reason,
				    TERMFONT_NONE);
			collect_emit(p, TERM_COLLECT_BUFFER_CURSOR,
			    TERM_COLLECT_ENTER, reason, p->col, p->col + 1,
			    0, 0, 0, TERMFONT_NONE);
			p->col++;

			/*
			 * Postpone the effect of \z while handling
			 * an overstrike sequence from ascii_uc2str().
			 */

			if (word[i] == '\b' &&
			    (p->flags & TERMP_BACKBEFORE)) {
				p->flags &= ~TERMP_BACKBEFORE;
				p->flags |= TERMP_BACKAFTER;
			}
		}
	}
	if (p->tcol->lastcol < p->col)
		p->tcol->lastcol = p->col;
}

void
term_setwidth(struct termp *p, const char *wstr)
{
	struct roffsu	 su;
	int		 iop, width;

	iop = 0;
	width = 0;
	if (wstr != NULL) {
		if (*wstr == '+' || *wstr == '-') {
			for (iop = 1;; wstr++) {
				if (*wstr == '-')
					iop = -iop;
				else if (*wstr != '+')
					break;
			}
		}
		if (a2roffsu(wstr, &su, SCALE_MAX) != NULL)
			width = term_hspan(p, &su);
		else
			iop = 0;
	}
	(*p->setwidth)(p, iop, width);
}

size_t
term_len(const struct termp *p, size_t sz)
{
	return (*p->getwidth)(p, ' ') * sz;
}

static size_t
cond_width(const struct termp *p, int c, int *skip)
{
	if (*skip) {
		(*skip) = 0;
		return 0;
	} else
		return (*p->getwidth)(p, c);
}

size_t
term_strlen(const struct termp *p, const char *cp)
{
	const char	*seq;		/* Escape sequence argument. */
	const char	*rhs;		/* String to be printed. */

	/* Widths in basic units. */
	size_t		 sz;		/* Return value. */
	size_t		 this_sz;	/* Individual char for overstrike. */
	size_t		 max_sz;	/* Result of overstrike. */

	/* Numbers of bytes. */
	size_t		 rsz;		/* Substring length in bytes. */
	size_t		 i;		/* Byte index in substring. */
	int		 ssz;		/* Argument length in bytes. */
	int		 skip;		/* Number of bytes to skip. */

	int		 uc;		/* Unicode codepoint number. */
	enum mandoc_esc	 esc;

	static const char rej[] = { '\\', ASCII_NBRSP, ASCII_NBRZW,
		ASCII_BREAK, ASCII_HYPH, ASCII_TABREF, '\0' };

	/*
	 * Account for escaped sequences within string length
	 * calculations.  This follows the logic in term_word() as we
	 * must calculate the width of produced strings.
	 */

	sz = 0;
	skip = 0;
	while ('\0' != *cp) {
		rsz = strcspn(cp, rej);
		for (i = 0; i < rsz; i++)
			sz += cond_width(p, *cp++, &skip);

		switch (*cp) {
		case '\\':
			cp++;
			rhs = NULL;
			uc = 0;
			esc = mandoc_escape(&cp, &seq, &ssz);
			switch (esc) {
			case ESCAPE_UNICODE:
				uc = mchars_num2uc(seq + 1, ssz - 1);
				break;
			case ESCAPE_NUMBERED:
				uc = mchars_num2char(seq, ssz);
				if (uc < 0)
					continue;
				break;
			case ESCAPE_SPECIAL:
				if (p->enc == TERMENC_ASCII) {
					rhs = mchars_spec2str(seq, ssz, &rsz);
					if (rhs != NULL)
						break;
				} else {
					uc = mchars_spec2cp(seq, ssz);
					if (uc > 0)
						sz += cond_width(p, uc, &skip);
				}
				continue;
			case ESCAPE_UNDEF:
				uc = *seq;
				break;
			case ESCAPE_DEVICE:
				if (p->type == TERMTYPE_PDF) {
					rhs = "pdf";
					rsz = 3;
				} else if (p->type == TERMTYPE_PS) {
					rhs = "ps";
					rsz = 2;
				} else if (p->enc == TERMENC_ASCII) {
					rhs = "ascii";
					rsz = 5;
				} else {
					rhs = "utf8";
					rsz = 4;
				}
				break;
			case ESCAPE_SKIPCHAR:
				skip = 1;
				continue;
			case ESCAPE_OVERSTRIKE:
				max_sz = 0;
				rhs = seq + ssz;
				while (seq < rhs) {
					if (*seq == '\\') {
						mandoc_escape(&seq, NULL, NULL);
						continue;
					}
					this_sz = (*p->getwidth)(p, *seq++);
					if (max_sz < this_sz)
						max_sz = this_sz;
				}
				sz += max_sz;
				continue;
			default:
				continue;
			}

			/*
			 * Common handling for Unicode and numbered
			 * character escape sequences.
			 */

			if (rhs == NULL) {
				if (p->enc == TERMENC_ASCII) {
					rhs = ascii_uc2str(uc);
					rsz = strlen(rhs);
				} else {
					if ((uc < 0x20 && uc != 0x09) ||
					    (uc > 0x7E && uc < 0xA0))
						uc = 0xFFFD;
					sz += cond_width(p, uc, &skip);
					continue;
				}
			}

			if (skip) {
				skip = 0;
				break;
			}

			/*
			 * Common handling for all escape sequences
			 * printing more than one character.
			 */

			for (i = 0; i < rsz; i++)
				sz += (*p->getwidth)(p, *rhs++);
			break;
		case ASCII_NBRSP:
			sz += cond_width(p, ' ', &skip);
			cp++;
			break;
		case ASCII_HYPH:
			sz += cond_width(p, '-', &skip);
			cp++;
			break;
		default:
			break;
		}
	}

	return sz;
}

int
term_vspan(const struct termp *p, const struct roffsu *su)
{
	double		 r;
	int		 ri;

	switch (su->unit) {
	case SCALE_BU:
		r = su->scale / 40.0;
		break;
	case SCALE_CM:
		r = su->scale * 6.0 / 2.54;
		break;
	case SCALE_FS:
		r = su->scale * 65536.0 / 40.0;
		break;
	case SCALE_IN:
		r = su->scale * 6.0;
		break;
	case SCALE_MM:
		r = su->scale * 0.006;
		break;
	case SCALE_PC:
		r = su->scale;
		break;
	case SCALE_PT:
		r = su->scale / 12.0;
		break;
	case SCALE_EN:
	case SCALE_EM:
		r = su->scale * 0.6;
		break;
	case SCALE_VS:
		r = su->scale;
		break;
	default:
		abort();
	}
	ri = r > 0.0 ? r + 0.4995 : r - 0.4995;
	return ri < 66 ? ri : 1;
}

/*
 * Convert a scaling width to basic units.
 */
int
term_hspan(const struct termp *p, const struct roffsu *su)
{
	return (*p->hspan)(p, su);
}
