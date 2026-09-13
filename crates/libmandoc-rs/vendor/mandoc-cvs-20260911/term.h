/* $Id: term.h,v 1.140 2026/01/07 08:23:23 schwarze Exp $ */
/*
 * Copyright (c) 2011-2015, 2017, 2019, 2021, 2022, 2025, 2026
 *               Ingo Schwarze <schwarze@openbsd.org>
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

enum	termenc {
	TERMENC_ASCII,
	TERMENC_LOCALE,
	TERMENC_UTF8
};

enum	termtype {
	TERMTYPE_CHAR,
	TERMTYPE_PS,
	TERMTYPE_PDF
};

enum	termfont {
	TERMFONT_NONE = 0,
	TERMFONT_BOLD,
	TERMFONT_UNDER,
	TERMFONT_BI,
	TERMFONT__MAX
};

/* ManT observer-only output purpose, orthogonal to atom provenance. */
enum term_exec_fragment_role {
	TERM_EXEC_FRAGMENT_CONTENT = 1,
	TERM_EXEC_FRAGMENT_MARGIN = 3,
	TERM_EXEC_FRAGMENT_PAGE = 4
};

struct	eqn_box;
struct	roff_meta;
struct	roff_node;
struct	tbl_span;
struct	termp;

enum term_exec_reference_kind {
	TERM_EXEC_REFERENCE_EXTERNAL_URI,
	TERM_EXEC_REFERENCE_EMAIL,
	TERM_EXEC_REFERENCE_MANUAL,
	TERM_EXEC_REFERENCE_SECTION
};

enum term_exec_affinity {
	TERM_EXEC_AFFINITY_INLINE,
	TERM_EXEC_AFFINITY_BEFORE_OUTPUT
};

struct term_exec_ops {
	int (*node_enter)(void *, const struct termp *,
	    const struct roff_node *);
	int (*node_leave)(void *, const struct termp *,
	    const struct roff_node *);
	int (*word_begin)(void *, const struct termp *,
	    const struct roff_node *, const char *, size_t, int *);
	int (*word_end)(void *, const struct termp *,
	    const struct roff_node *);
	int (*buffer_write)(void *, const struct termp *,
	    const struct roff_node *, size_t, int, int, int);
	int (*buffer_reserve)(void *, const struct termp *,
	    const struct roff_node *, size_t, size_t);
	int (*buffer_rewrite)(void *, const struct termp *,
	    const struct roff_node *, size_t, int);
	int (*buffer_discard)(void *, const struct termp *,
	    const struct roff_node *, size_t, size_t, int);
	int (*buffer_reset)(void *, const struct termp *,
	    const struct roff_node *);
	int (*flush_begin)(void *, const struct termp *,
	    const struct roff_node *);
	int (*fill_scan)(void *, const struct termp *,
	    const struct roff_node *, size_t);
	int (*fill_decision)(void *, const struct termp *,
	    const struct roff_node *, size_t, size_t, size_t, size_t);
	int (*fill_outcome)(void *, const struct termp *,
	    const struct roff_node *, int);
	int (*field_begin)(void *, const struct termp *,
	    const struct roff_node *, size_t, size_t, size_t);
	int (*field_atom)(void *, const struct termp *,
	    const struct roff_node *, size_t, int);
	int (*field_end)(void *, const struct termp *,
	    const struct roff_node *, size_t, size_t, size_t);
	int (*flush_end)(void *, const struct termp *,
	    const struct roff_node *);
	int (*boundary_enter)(void *, const struct termp *,
	    const struct roff_node *, int);
	int (*boundary_leave)(void *, const struct termp *,
	    const struct roff_node *, int);
	int (*device_advance)(void *, const struct termp *,
	    const struct roff_node *, size_t, size_t, size_t);
	int (*device_letter)(void *, const struct termp *,
	    const struct roff_node *, size_t, int, size_t, size_t);
	int (*device_endline)(void *, const struct termp *,
	    const struct roff_node *, size_t, size_t, size_t, size_t);
	int (*font)(void *, const struct termp *, const struct roff_node *,
	    int, int, size_t, size_t);
	int (*reference_begin)(void *, const struct termp *,
	    const struct roff_node *, const struct roff_node *,
	    const struct roff_node *, int, const char *, size_t,
	    const char *, size_t, int);
	int (*reference_end)(void *, const struct termp *,
	    const struct roff_node *);
	int (*anchor)(void *, const struct termp *, const struct roff_node *,
	    const char *, size_t, size_t, int);
};

typedef void	(*term_margin)(struct termp *, const struct roff_meta *);

struct	termp_col {
	int		 *buf;		/* Output buffer. */
	size_t		  maxcols;	/* Allocated bytes in buf. */
	size_t		  lastcol;	/* Last byte in buf. */
	size_t		  col;		/* Byte in buf to be written. */
	size_t		  rmargin;	/* Current right margin [BU]. */
	size_t		  offset;	/* Current left margin [BU]. */
	size_t		  taboff;	/* Offset for literal tabs [BU]. */
};

struct	termp {
	struct rofftbl	  tbl;		/* Table configuration. */
	const int	 *tbl_borders;	/* Table borders for this encoding. */
	size_t		  tbl_offset;	/* Left offset of the current table [BU]. */
	struct termp_col *tcols;	/* Array of table columns. */
	struct termp_col *tcol;		/* Current table column. */
	size_t		  maxtcol;	/* Allocated table columns. */
	size_t		  lasttcol;	/* Last column currently used. */
	size_t		  line;		/* Current output line number. */
	size_t		  defindent;	/* Default indent for text [EN]. */
					/* Line lengths in basic units: */
	size_t		  defrmargin;	/* ... as set by -O width / paper */
	size_t		  lastrmargin;	/* ... before the last .ll request */
	size_t		  maxrmargin;	/* ... as set by .ll / setwidth() */
	size_t		  col;		/* Byte position in buf. */
	size_t		  viscol;	/* Width of the current line [BU]. */
	size_t		  trailspace;	/* Whitespace after field [EN]. */
	size_t		  minbl;	/* Whitespace before field [EN]. */
	int		  synopsisonly; /* Print the synopsis only. */
	int		  ti;		/* Temporary indent for line [BU]. */
	int		  skipvsp;	/* Vertical space to skip. */
	int		  roff_po;	/* Requested page offset [BU]. */
	int		  roff_pouse;	/* Applied page offset [BU]. */
	int		  roff_polast;	/* Previous page offset [BU]. */
	int		  flags;
#define	TERMP_SENTENCE	 (1 << 0)	/* Space before a sentence. */
#define	TERMP_NOSPACE	 (1 << 1)	/* No space before words. */
#define	TERMP_NONOSPACE	 (1 << 2)	/* No space (no autounset). */
#define	TERMP_NBRWORD	 (1 << 3)	/* Make next word nonbreaking. */
#define	TERMP_KEEP	 (1 << 4)	/* Keep words together. */
#define	TERMP_PREKEEP	 (1 << 5)	/* ...starting with the next one. */
#define	TERMP_BACKAFTER	 (1 << 6)	/* Back up after next character. */
#define	TERMP_BACKBEFORE (1 << 7)	/* Back up before next character. */
#define	TERMP_NOBREAK	 (1 << 8)	/* See term_flushln(). */
#define	TERMP_BRTRSP	 (1 << 9)	/* See term_flushln(). */
#define	TERMP_BRIND	 (1 << 10)	/* See term_flushln(). */
#define	TERMP_HANG	 (1 << 11)	/* See term_flushln(). */
#define	TERMP_NOPAD	 (1 << 12)	/* See term_flushln(). */
#define	TERMP_NOSPLIT	 (1 << 13)	/* Do not break line before .An. */
#define	TERMP_SPLIT	 (1 << 14)	/* Break line before .An. */
#define	TERMP_NONEWLINE	 (1 << 15)	/* No line break in nofill mode. */
#define	TERMP_BRNEVER	 (1 << 16)	/* Don't even break at maxrmargin. */
#define	TERMP_NOBUF	 (1 << 17)	/* Bypass output buffer. */
#define	TERMP_NEWMC	 (1 << 18)	/* No .mc printed yet. */
#define	TERMP_ENDMC	 (1 << 19)	/* Next break ends .mc mode. */
#define	TERMP_MULTICOL	 (1 << 20)	/* Multiple column mode. */
#define	TERMP_CENTER	 (1 << 21)	/* Center output lines. */
#define	TERMP_RIGHT	 (1 << 22)	/* Adjust to the right margin. */
	enum termtype	  type;		/* Terminal, PS, or PDF. */
	enum termenc	  enc;		/* Type of encoding. */
	enum termfont	  fontl;	/* Last font set. */
	enum termfont	 *fontq;	/* Symmetric fonts. */
	int		  fontsz;	/* Allocated size of font stack */
	int		  fonti;	/* Index of font stack. */
	int		  fontibi;	/* Map font I to BI. */
	term_margin	  headf;	/* invoked to print head */
	term_margin	  footf;	/* invoked to print foot */
	void		(*letter)(struct termp *, int);
	void		(*begin)(struct termp *);
	void		(*end)(struct termp *);
	void		(*endline)(struct termp *);
	void		(*advance)(struct termp *, size_t);
	void		(*setwidth)(struct termp *, int, size_t);
	size_t		(*getwidth)(const struct termp *, int);
	int		(*hspan)(const struct termp *,
				const struct roffsu *);
	const void	 *argf;		/* arg for headf/footf */
	const char	 *mc;		/* Margin character. */
	struct termp_ps	 *ps;
	const struct term_exec_ops *exec_ops; /* Optional execution observer. */
	void		 *exec_arg;
	const struct roff_node *exec_node;
	int		  exec_failed;
	size_t		  exec_field_slot;
	int		  exec_field_active;
	int		  exec_write_role;
	int		  exec_fragment_role;
};


const char	 *ascii_uc2str(int);

void		  roff_term_pre(struct termp *, const struct roff_node *);

void		  term_eqn(struct termp *, const struct eqn_box *);
void		  term_tbl(struct termp *, const struct tbl_span *);
void		  term_free(struct termp *);
void		  term_setcol(struct termp *, size_t);
void		  term_newln(struct termp *);
void		  term_vspace(struct termp *);
void		  term_word(struct termp *, const char *);
void		  term_flushln(struct termp *);
void		  term_begin(struct termp *, term_margin,
			term_margin, const struct roff_meta *);
void		  term_end(struct termp *);

void		  term_setwidth(struct termp *, const char *);
int		  term_hspan(const struct termp *, const struct roffsu *);
int		  term_vspan(const struct termp *, const struct roffsu *);
size_t		  term_strlen(const struct termp *, const char *);
size_t		  term_len(const struct termp *, size_t);

void		  term_tab_set(const struct termp *, const char *);
void		  term_tab_ref(struct termp *);
size_t		  term_tab_next(size_t);
void		  term_tab_free(void);

void		  term_fontpush(struct termp *, enum termfont);
void		  term_fontpop(struct termp *);
void		  term_fontpopq(struct termp *, int);
void		  term_fontrepl(struct termp *, enum termfont);
void		  term_fontlast(struct termp *);
void		  term_exec_attach(struct termp *,
			const struct term_exec_ops *, void *);
int		  term_exec_failed(const struct termp *);
int		  term_exec_node(struct termp *, const struct roff_node *, int);
int		  term_exec_word(struct termp *, const char *, size_t, int);
int		  term_exec_buffer_write(struct termp *, size_t, int, int, int);
int		  term_exec_buffer_reserve(struct termp *, size_t, size_t);
int		  term_exec_buffer_rewrite(struct termp *, size_t, int);
int		  term_exec_buffer_discard(struct termp *, size_t, size_t, int);
int		  term_exec_buffer_reset(struct termp *);
int		  term_exec_flush(struct termp *, int);
int		  term_exec_fill_scan(struct termp *, size_t);
int		  term_exec_fill_decision(struct termp *, size_t, size_t,
			size_t, size_t);
int		  term_exec_fill_outcome(struct termp *, int);
int		  term_exec_field(struct termp *, int, size_t, size_t, size_t);
int		  term_exec_field_atom(struct termp *, size_t, int);
int		  term_exec_boundary(struct termp *, int, int);
int		  term_exec_device_advance(struct termp *, size_t, size_t,
			size_t);
int		  term_exec_device_letter(struct termp *, size_t, int, size_t,
			size_t);
int		  term_exec_device_endline(struct termp *, size_t, size_t,
			size_t, size_t);
int		  term_exec_font(struct termp *, int, int, size_t, size_t);
int		  term_exec_reference_begin(struct termp *, int,
			const struct roff_node *, const struct roff_node *,
			const char *, const char *, int);
int		  term_exec_reference_end(struct termp *);
int		  term_exec_anchor(struct termp *, const struct roff_node *);
