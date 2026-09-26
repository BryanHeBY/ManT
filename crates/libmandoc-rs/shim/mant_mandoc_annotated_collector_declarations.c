/* Native man/mdoc declaration and reading-neighbor evidence only.
 * Final entry names are classified from surviving display glyphs. */
#include "mant_mandoc_annotated_collector_private.h"

/* roff.c::roff_node_prev() skips a wider set of transparent nodes, including
 * layout controls. For reading-context evidence, only these non-content
 * siblings can stand between direct man definition blocks. A real flow
 * boundary also changes roff_node::flow_epoch at allocation. */
int
mant_annotated_decl_reading_sibling_gap(const struct roff_node *node)
{
	return node->type == ROFFT_COMMENT || node->tok == MAN_PD ||
	    node->tok == MDOC_Sm || node->tok == MDOC_Tg ||
	    node->tok == ROFF_ft;
}

int
mant_annotated_decl_reading_family(int token)
{
	if (token == MAN_IP)
		return 1;
	return token == MAN_TP || token == MAN_TQ ? 2 : 0;
}

int
mant_annotated_decl_paragraph_token(enum roff_tok token)
{
	return token == MAN_PP || token == MAN_P || token == MAN_LP;
}

static int head_text_has_glyph(const char *);

static int
hanging_text_has_glyph(struct mant_annotated_collector *collector,
    const struct roff_node *node, int *has_glyph)
{
	*has_glyph = 0;
	if (node->string == NULL)
		return 1;
	if (!mant_annotated_charge_work(collector, strlen(node->string)))
		return 0;
	*has_glyph = head_text_has_glyph(node->string);
	return 1;
}

/* man_term.c::pre_B()/pre_I() only change the font; pre_alternate()
 * executes each TEXT operand without inventing glyphs. An empty instance
 * may precede a real declaration, but cannot itself supply its source. */
static int
hanging_node_has_glyph(struct mant_annotated_collector *collector,
    const struct roff_node *node, int *has_glyph)
{
	const struct roff_node *child;
	int child_has_glyph;

	if (node->type == ROFFT_TEXT)
		return hanging_text_has_glyph(collector, node, has_glyph);
	*has_glyph = 1;
	if (node->type != ROFFT_ELEM)
		return 1;
	switch (node->tok) {
	case MAN_SM:
	case MAN_SB:
	case MAN_BI:
	case MAN_IB:
	case MAN_BR:
	case MAN_RB:
	case MAN_R:
	case MAN_B:
	case MAN_I:
	case MAN_IR:
	case MAN_RI:
		break;
	default:
		return 1;
	}
	*has_glyph = 0;
	for (child = node->child; child != NULL; child = child->next) {
		if (!mant_annotated_charge_work(collector, 1))
			return 0;
		if (child->type != ROFFT_TEXT) {
			*has_glyph = 1;
			return 1;
		}
		if (!hanging_text_has_glyph(collector, child, &child_has_glyph))
			return 0;
		if (child_has_glyph) {
			*has_glyph = 1;
			return 1;
		}
	}
	return 1;
}

/* roff.c::roff_node_alloc() advances flow_epoch for a real .sp before the
 * following TEXT is allocated. roff_term.c::roff_term_pre_sp() then ends the
 * prior line and emits vertical space. Only the first visible content sibling
 * of the section body or a retained .sp can start an implicit presentation
 * head; .br, deleted controls and preceding prose lack that authority. */
static int
man_hanging_after_boundary(struct mant_annotated_collector *collector,
    const struct roff_node *node)
{
	const struct roff_node *previous;
	int has_glyph;

	/* term.c::term_word() emits no glyph for \& or font-only text. Such a
	 * node cannot be the declaration origin; defer to the first visible
	 * sibling without losing the retained .sp boundary. No-fill examples
	 * and transparent controls remain outside implicit paragraph inference. */
	if ((node->flags & (NODE_NOFILL | NODE_NOPRT)) != 0 ||
	    (node->type == ROFFT_ELEM &&
	    (node->tok < MAN_TH || node->tok >= MAN_MAX ||
	    roff_tok_transparent(node->tok))))
		return 0;
	if (!hanging_node_has_glyph(collector, node, &has_glyph))
		return 0;
	if (!has_glyph)
		return 0;
	for (previous = node->prev; previous != NULL &&
	    previous->flow_epoch == node->flow_epoch; previous = previous->prev) {
		if (!mant_annotated_charge_work(collector, 1))
			return 0;
		if (previous->type == ROFFT_COMMENT ||
		    (previous->flags & NODE_NOPRT) != 0 ||
		    roff_tok_transparent(previous->tok))
			continue;
		if (previous->type == ROFFT_TEXT ||
		    previous->type == ROFFT_ELEM) {
			if (!hanging_node_has_glyph(collector, previous,
			    &has_glyph))
				return 0;
			if (!has_glyph)
				continue;
		}
		break;
	}
	if (!mant_annotated_charge_work(collector, 1))
		return 0;
	if (previous == NULL)
		return node->flow_epoch == node->parent->flow_epoch;
	return previous->type == ROFFT_ELEM &&
	    previous->tok == ROFF_sp && previous->parent == node->parent &&
	    previous->flow_epoch == node->flow_epoch;
}

/* man_validate.c::post_SH unwraps the first PP/P/LP after SH/SS but retains
 * the executed token on each moved child.  Other paragraphs retain their actual
 * BODY parent.  An implicit paragraph starts either at the SH/SS BODY's own
 * epoch or immediately after a preserved .sp in a new epoch.  The whole
 * paragraph, not its first style macro, must directly precede RS; Rust
 * applies complete-head grammar. */
const struct roff_node *
mant_annotated_decl_hanging_successor(struct mant_annotated_collector *collector,
    const struct roff_node *node, int *mode)
{
	const struct roff_node *rs;

	*mode = 0;
	if (node->parent == NULL || collector->active_owner == 0 ||
	    collector->marks[collector->active_owner - 1].kind !=
	    MANT_ANNOTATED_MARK_REGION ||
	    collector->marks[collector->active_owner - 1].region_kind !=
	    MANT_ANNOTATED_REGION_HEADING_BODY)
		return NULL;
	if (node->type == ROFFT_BLOCK && mant_annotated_decl_paragraph_token(node->tok) &&
	    node->body != NULL && node->body->child != NULL)
		rs = node->next;
	else if (mant_annotated_decl_paragraph_token(node->mant_elided_par_tok) &&
	    node->parent->type == ROFFT_BODY &&
	    (node->parent->tok == MAN_SH || node->parent->tok == MAN_SS) &&
	    node->parent->child == node) {
		*mode = 1;
		rs = node;
		while (rs != NULL &&
		    rs->mant_elided_par_tok == node->mant_elided_par_tok) {
			if (!mant_annotated_charge_work(collector, 1))
				return NULL;
			rs = rs->next;
		}
	} else if (node->parent->type == ROFFT_BODY &&
	    (node->parent->tok == MAN_SH || node->parent->tok == MAN_SS) &&
	    (node->type == ROFFT_TEXT || node->type == ROFFT_ELEM) &&
	    man_hanging_after_boundary(collector, node)) {
		/* A deleted .br/.sp still advances flow_epoch, so neither the
		 * section-start nor .sp branch can borrow that hidden boundary. */
		*mode = 2;
		rs = node;
		while (rs != NULL && rs->flow_epoch == node->flow_epoch &&
		    (rs->type == ROFFT_TEXT || rs->type == ROFFT_ELEM ||
		    (rs->type == ROFFT_BLOCK &&
		    (rs->tok == MAN_UR || rs->tok == MAN_MT ||
		    rs->tok == MAN_MR)))) {
			if (!mant_annotated_charge_work(collector, 1))
				return NULL;
			rs = rs->next;
		}
	} else
		return NULL;
	return rs != NULL && rs->type == ROFFT_BLOCK &&
	    rs->tok == MAN_RS && rs->parent == node->parent &&
	    rs->body != NULL && rs->body->child != NULL ? rs : NULL;
}

int
mant_annotated_decl_direct_predecessor(struct mant_annotated_collector *collector,
    const struct roff_node *node, const struct roff_node **candidate)
{
	const struct roff_node *previous;
	int family = mant_annotated_decl_reading_family(node->tok);

	*candidate = NULL;
	for (previous = node->prev; previous != NULL &&
	    mant_annotated_decl_reading_sibling_gap(previous); previous = previous->prev)
		if (!mant_annotated_charge_work(collector, 1))
			return 0;
	if (!mant_annotated_charge_work(collector, 1))
		return 0;
	/* man_html.c::list_continues() keeps TP/TQ in one definition-list
	 * family, but never merges that family with IP. Both remain separate
	 * declaration owners and need a completed, direct AST sibling. */
	if (family != 0 && previous != NULL &&
	    previous->type == ROFFT_BLOCK &&
	    mant_annotated_decl_reading_family(previous->tok) == family &&
	    previous->parent == node->parent &&
	    previous->flow_epoch == node->flow_epoch)
		*candidate = previous;
	return 1;
}

/* term.c::term_word() emits no glyph for IGNORE, NOSPACE and font escapes.
 * All other escapes remain significant here, including skipchar/overstrike
 * state and potentially visible special or Unicode characters. */
static int
head_text_has_glyph(const char *text)
{
	enum mandoc_esc esc;
	const unsigned char *plain;

	while (*text != '\0') {
		if (*text != '\\') {
			plain = (const unsigned char *)text;
			if (!isspace(*plain))
				return 1;
			text++;
			continue;
		}
		text++;
		esc = mandoc_escape(&text, NULL, NULL);
		switch (esc) {
		case ESCAPE_IGNORE:
		case ESCAPE_NOSPACE:
		case ESCAPE_FONT:
		case ESCAPE_FONTROMAN:
		case ESCAPE_FONTITALIC:
		case ESCAPE_FONTBOLD:
		case ESCAPE_FONTBI:
		case ESCAPE_FONTCR:
		case ESCAPE_FONTCB:
		case ESCAPE_FONTCI:
		case ESCAPE_FONTPREV:
			break;
		default:
			return 1;
		}
	}
	return 0;
}

/* A man HEAD text node may carry inline font escapes, whereas the formatter
 * only presents the final glyphs. This is a broad declaration candidate, not
 * a name or font parser: term.c::term_word() can switch fonts before the first
 * visible glyph, so final display evidence must make the style decision.
 * A one-letter IP label remains commonly a list marker. */
static int
man_text_declaration_candidate(struct mant_annotated_collector *collector,
    const char *text, int reject_single_letter, int *recognized)
{
	const char *cursor, *next, *sequence;
	enum mandoc_esc escape;
	int size, first_glyph = 0, glyph_count = 0;
	int glyph;

	*recognized = 0;
	if (text == NULL)
		return 1;
	if (!mant_annotated_charge_work(collector, strlen(text)))
		return 0;
	for (cursor = text; *cursor != '\0' && glyph_count < 2; ) {
		if (*cursor != '\\')
			glyph = (unsigned char)*cursor++;
		else {
			next = cursor + 1;
			escape = mandoc_escape(&next, &sequence, &size);
			cursor = next;
			switch (escape) {
			case ESCAPE_IGNORE:
			case ESCAPE_NOSPACE:
			case ESCAPE_FONTBOLD:
			case ESCAPE_FONTCB:
			case ESCAPE_FONTBI:
			case ESCAPE_FONT:
			case ESCAPE_FONTROMAN:
			case ESCAPE_FONTITALIC:
			case ESCAPE_FONTCI:
			case ESCAPE_FONTPREV:
				continue;
			case ESCAPE_SPECIAL:
				if (size != 1 || sequence[0] != '-')
					return 1;
				glyph = '-';
				break;
			default:
				return 1;
			}
		}
		if (isspace((unsigned char)glyph))
			continue;
		if (first_glyph == 0)
			first_glyph = glyph;
		glyph_count++;
	}
	if (reject_single_letter && glyph_count < 2)
		return 1;
	if ((first_glyph >= 'a' && first_glyph <= 'z') ||
	    (first_glyph >= 'A' && first_glyph <= 'Z') ||
	    first_glyph == '-' || first_glyph == '_')
		*recognized = 1;
	return 1;
}

/* mdoc_macro.c::blk_full() closes the It HEAD before terminal traversal.
 * Freeze the first significant authored head macro while that native tree
 * remains alive; final font alone cannot distinguish Fl/Ev/Va/Dv/Ic/Cm
 * from unrelated typography.  This is evidence, not classification. */
uint32_t
mant_annotated_decl_owner_head_role(const struct roff_node *owner,
    const struct roff_node **role_node)
{
	const struct roff_node *head, *node;
	int skip_children;

	head = owner->head;
	*role_node = NULL;
	if (head == NULL)
		return 0;
	for (node = head->child; node != NULL; ) {
		/* mdoc_term.c::print_mdoc_node() does not descend into NOPRT;
		 * generated NODE_NOSRC macros have no authored role identity. Their
		 * visible children still prevent a later macro from borrowing the
		 * beginning of this HEAD. */
		skip_children = (node->flags & NODE_NOPRT) != 0 ||
		    node->tok == MDOC_Tg || node->tok == MDOC_Ns ||
		    node->tok == MDOC_Sm;
		if (!skip_children &&
		    mant_annotated_marks_source_key(node) != 0) {
			switch (node->tok) {
			case MDOC_Fl:
				*role_node = node;
				return MANT_ANNOTATED_MARK_HEAD_OPTION;
			case MDOC_Ev:
				*role_node = node;
				return MANT_ANNOTATED_MARK_HEAD_ENVIRONMENT;
			case MDOC_Va:
				*role_node = node;
				return MANT_ANNOTATED_MARK_HEAD_VARIABLE;
			case MDOC_Dv:
				*role_node = node;
				return MANT_ANNOTATED_MARK_HEAD_DEFINED_VARIABLE;
			case MDOC_Ic:
			case MDOC_Cm:
				*role_node = node;
				return MANT_ANNOTATED_MARK_HEAD_LITERAL;
			case MDOC_Ar:
			case MDOC_Em:
			case MDOC_Sy:
				return 0;
			default:
				break;
			}
		}
		if (!skip_children && node->type == ROFFT_TEXT &&
		    node->string != NULL &&
		    head_text_has_glyph(node->string))
			return 0;
		if (!skip_children && node->child != NULL) {
			node = node->child;
			continue;
		}
		while (node != head && node->next == NULL)
			node = node->parent;
		if (node == head)
			break;
		node = node->next;
	}
	return 0;
}

/* man_macro.c::blk_imp keeps TP/TQ HEAD distinct; man_term.c::pre_B and
 * pre_alternate establish a candidate role.  term.c::term_word() executes
 * escapes and font changes later.  Raw operand spelling must neither freeze
 * a prefix nor reject an equivalent final-display declaration. */
int
mant_annotated_decl_copy_tp_lexical_head(struct mant_annotated_collector *collector,
    const struct roff_node *owner, int *recognized, int *direct_text)
{
	const struct roff_node *head, *first;

	*recognized = 0;
	*direct_text = 0;
	if (owner->tok != MAN_TP && owner->tok != MAN_TQ)
		return 1;
	head = owner->head;
	first = head == NULL ? NULL : head->child;
	/* man_macro.c::blk_imp retains the TP/TQ same-line width operands in
	 * HEAD, but man_term.c::pre_TP prints only from the first NODE_LINE
	 * child. Do not let layout arguments become declaration evidence. */
	while (first != NULL && (first->flags & NODE_LINE) == 0) {
		if (!mant_annotated_charge_work(collector, 1))
			return 0;
		first = first->next;
	}
	/* A next-line PD changes paragraph distance without printing a label;
	 * the following next-line macro remains the actual HEAD candidate. */
	while (first != NULL && first->tok == MAN_PD) {
		if (!mant_annotated_charge_work(collector, 1))
			return 0;
		first = first->next;
	}
	if (first == NULL)
		return 1;
	/* pre_B and pre_alternate provide a lexical candidate boundary. I/R and
	 * italic/roman-first alternate macros do not. The final displayed glyphs
	 * and fonts, not the first authored operand, decide whether it names an
	 * option. */
	if (first->type == ROFFT_TEXT) {
		if (!man_text_declaration_candidate(collector, first->string,
		    0, recognized))
			return 0;
		*direct_text = *recognized;
		return 1;
	}
	if (first->tok != MAN_B && first->tok != MAN_BI &&
	    first->tok != MAN_BR && first->tok != MAN_SB)
		return 1;
	if (first->tok == MAN_BR || first->tok == MAN_BI) {
		/* man_term.c::pre_alternate() traverses every operand, including
		 * empty ones. A later bold operand can supply the whole visible
		 * name; an italic-only BI head remains merely a candidate and is
		 * rejected by the final-display name check. */
		*recognized = 1;
		return 1;
	}
	*recognized = first->tok == MAN_B || first->tok == MAN_SB;
	return 1;
}

/* man_macro.c::blk_imp retains the first .IP argument as one HEAD text node;
 * man_term.c::pre_IP prints it and uses the next argument only for width.
 * A leading bold font is structural candidate evidence, not a parsed option
 * prefix: term.c::term_word() executes escapes before Rust sees the glyphs. */
int
mant_annotated_decl_ip_bold_candidate(struct mant_annotated_collector *collector,
    const struct roff_node *owner, int *recognized)
{
	const struct roff_node *first;
	const char *cursor, *next;
	enum mandoc_esc escape;

	*recognized = 0;
	first = owner->head == NULL ? NULL : owner->head->child;
	if (first == NULL || first->type != ROFFT_TEXT ||
	    first->string == NULL || first->string[0] != '\\')
		return 1;
	if (!mant_annotated_charge_work(collector, strlen(first->string)))
		return 0;
	cursor = first->string;
	while (*cursor != '\0') {
		if (*cursor != '\\')
			break;
		next = cursor + 1;
		escape = mandoc_escape(&next, NULL, NULL);
		if (escape == ESCAPE_FONTBOLD || escape == ESCAPE_FONTCB)
			*recognized = 1;
		else if (escape != ESCAPE_IGNORE && escape != ESCAPE_NOSPACE &&
		    escape != ESCAPE_FONT && escape != ESCAPE_FONTITALIC &&
		    escape != ESCAPE_FONTBI && escape != ESCAPE_FONTROMAN &&
		    escape != ESCAPE_FONTCR && escape != ESCAPE_FONTCI &&
		    escape != ESCAPE_FONTPREV)
			break;
		cursor = next;
	}
	return 1;
}

/* man_term.c::pre_IP prints only the first HEAD text node; the second is
 * layout width. A plain first operand can be a declaration candidate, but
 * italic-only, numbered, bullet, and one-letter list labels are not names. */
int
mant_annotated_decl_ip_head_candidate(struct mant_annotated_collector *collector,
    const struct roff_node *owner, int *recognized)
{
	const struct roff_node *first;
	const char *cursor;
	enum mandoc_esc escape;

	*recognized = 0;
	first = owner->head == NULL ? NULL : owner->head->child;
	if (first == NULL || first->type != ROFFT_TEXT || first->string == NULL)
		return 1;
	/* Leading bold font evidence is handled above. An inline-font head
	 * must not re-enter through a plain-text spelling guess. */
	if (first->string[0] == '\\') {
		cursor = first->string + 1;
		escape = mandoc_escape(&cursor, NULL, NULL);
		if (escape == ESCAPE_FONTBOLD || escape == ESCAPE_FONTCB)
			return 1;
	}
	return man_text_declaration_candidate(collector, first->string, 1,
	    recognized);
}
