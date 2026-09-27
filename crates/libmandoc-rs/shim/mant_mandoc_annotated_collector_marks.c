/* Native AST/owner observation and result-local mark construction. */
#include "mant_mandoc_annotated_collector_private.h"

static int hanging_point(struct mant_annotated_collector *, uint32_t);

/* Iterate the HEAD in the same child/sibling order as roff.c::deroff(),
 * without using the C stack for arbitrarily deep inline macro trees. */
static const struct roff_node *
heading_next(struct mant_annotated_collector *collector,
    const struct roff_node *root, const struct roff_node *node)
{
	if (node->string == NULL && node->child != NULL)
		return node->child;
	while (node != root) {
		if (node->next != NULL)
			return node->next;
		node = node->parent;
		if (!mant_annotated_charge_work(collector, 1))
			return NULL;
		if (node == NULL) {
			mant_annotated_fail_relation(collector, 0, 1);
			return NULL;
		}
	}
	return NULL;
}

/* Preserve deroff()'s leading escape/whitespace and trailing rules exactly.
 * The first nonempty leaf uses strndup(cp, sz); later leaves use %*s, which
 * is a minimum width and therefore retains their original trailing bytes. */
static int
heading_leaf(struct mant_annotated_collector *collector, const char *string,
    size_t *start, size_t *trimmed_end, size_t *full_end)
{
	size_t length, offset, end;

	for (length = 0; string[length] != '\0'; length++)
		if (!mant_annotated_charge_work(collector, 1))
			return 0;
	for (offset = 0; offset < length; offset++) {
		if (!mant_annotated_charge_work(collector, 1))
			return 0;
		if (string[offset] == '\\' && offset + 1 < length &&
		    strchr(" %&0^|~", string[offset + 1]) != NULL)
			offset++;
		else if (!isspace((unsigned char)string[offset]))
			break;
	}
	end = length;
	if (end > offset && string[end - 1] == '\\')
		end--;
	while (end > offset) {
		if (!mant_annotated_charge_work(collector, 1))
			return 0;
		if (!isspace((unsigned char)string[end - 1]))
			break;
		end--;
	}
	*start = offset;
	*trimmed_end = end;
	*full_end = length;
	return 1;
}

static int
copy_heading_phrase(struct mant_annotated_collector *collector,
    struct mant_annotated_mark *mark, const struct roff_node *head)
{
	const struct roff_node *node;
	struct structured_session *session = collector->session;
	uint8_t *phrase;
	uint64_t length = 0, part;
	size_t start, trimmed_end, full_end, used = 0;
	int first = 1;

	if (head == NULL)
		return 1;
	for (node = head; node != NULL;
	    node = heading_next(collector, head, node)) {
		if (!mant_annotated_charge_work(collector, 1))
			return 0;
		if (node->string == NULL)
			continue;
		if (!heading_leaf(collector, node->string, &start,
		    &trimmed_end, &full_end))
			return 0;
		if (trimmed_end == start)
			continue;
		part = first ? trimmed_end - start : full_end - start + 1;
		if (part > session->limits->max_content_bytes ||
		    length > session->limits->max_content_bytes - part) {
			mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET,
			    MANT_STRUCTURED_STAGE_RENDER, 10,
			    part > UINT64_MAX - length ? UINT64_MAX : length + part,
			    session->limits->max_content_bytes);
			return 0;
		}
		length += part;
		first = 0;
	}
	if (session->status != MANT_STRUCTURED_OK || length == 0)
		return session->status == MANT_STRUCTURED_OK;
	phrase = mant_structured_allocate(session, length, 0,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (phrase == NULL)
		return 0;
	first = 1;
	for (node = head; node != NULL;
	    node = heading_next(collector, head, node)) {
		if (!mant_annotated_charge_work(collector, 1))
			goto failure;
		if (node->string == NULL)
			continue;
		if (!heading_leaf(collector, node->string, &start,
		    &trimmed_end, &full_end))
			goto failure;
		if (trimmed_end == start)
			continue;
		if (!first)
			phrase[used++] = ' ';
		part = first ? trimmed_end - start : full_end - start;
		if (!mant_annotated_charge_work(collector, part))
			goto failure;
		memcpy(phrase + used, node->string + start, (size_t)part);
		used += part;
		first = 0;
	}
	if (session->status != MANT_STRUCTURED_OK)
		goto failure;
	if (!mant_annotated_charge_work(collector, length))
		goto failure;
	if (!mant_structured_valid_utf8(phrase, (size_t)length)) {
		free(phrase);
		return 1;
	}
	if (!mant_structured_charge(session, &session->content_bytes,
	    length, session->limits->max_content_bytes, 10,
	    MANT_STRUCTURED_STAGE_RENDER))
		goto failure;
	mark->name = phrase;
	mark->name_length = length;
	return 1;
failure:
	free(phrase);
	return 0;
}

uint32_t
mant_annotated_marks_source_key(const struct roff_node *node)
{
	return node == NULL || (node->flags & NODE_NOSRC) != 0 ? 0 :
	    node->mant_source_key;
}

static int
within_node(const struct roff_node *node, const struct roff_node *ancestor)
{
	for (; node != NULL; node = node->parent)
		if (node == ancestor)
			return 1;
	return 0;
}

/* The link macro identifies the occurrence, but only selected terminal
 * characters belong to its clickable label.  These operand boundaries are
 * the same ones used by pinned man_html.c::man_MR_pre/man_UR_pre and
 * mdoc_html.c::mdoc_lk_pre; formatter-generated decoration is not inferred
 * from a source span or from a contiguous final display interval. */
uint32_t
mant_annotated_marks_visible_link(const struct mant_annotated_collector *collector,
    const struct roff_node *node, enum term_collector_reason reason)
{
	const struct roff_node *macro = collector->active_link_node;
	const struct roff_node *first, *second, *punct, *body, *head;

	if (macro == NULL || collector->active_link == 0 ||
	    collector->active_link_epoch != collector->phrase_epoch)
		return 0;
	/* mdoc_html.c::mdoc_mt_pre opens a separate anchor for every text
	 * operand.  The terminal's automatic space before each operand is
	 * outside both adjacent anchors, even though term_word_node() reports
	 * that space with the operand as its current node. */
	if (macro->type == ROFFT_TEXT && macro->parent != NULL &&
	    macro->parent->tok == MDOC_Mt)
		return reason != TERM_COLLECT_AUTO_SPACE &&
		    within_node(node, macro) ? collector->active_link : 0;
	/* term.c::term_word() inserts an AUTO_SPACE before the first operand.
	 * man_html.c::man_MR_pre and mdoc_html.c::mdoc_xr_pre open the anchor
	 * after that separator.  Their terminal pre handlers keep NOSPACE
	 * between name and section, so no interior auto-space belongs here. */
	if ((macro->tok == MAN_MR || macro->tok == MDOC_Xr) &&
	    reason == TERM_COLLECT_AUTO_SPACE)
		return 0;
	/* man_term.c::pre_MR and mdoc_term.c::termp_xr_pre emit parentheses
	 * with term_word(), unlike operands emitted via term_word_node().
	 * Their logical event has no node, yet HTML keeps them in the anchor. */
	if (node == NULL)
		return macro->tok == MAN_MR || macro->tok == MDOC_Xr ?
		    collector->active_link : 0;
	switch (macro->tok) {
	case MAN_BR:
	case MAN_IR:
		/* man_term.c::pre_alternate() executes these two operands with
		 * no inserted space.  The styled candidate does not claim any
		 * following punctuation or an enclosing explicit link. */
		first = macro->child;
		second = first == NULL ? NULL : first->next;
		return reason != TERM_COLLECT_AUTO_SPACE &&
		    (within_node(node, first) || within_node(node, second)) ?
		    collector->active_link : 0;
	case MAN_MR:
		/* man_term.c::pre_MR prints name(section), then a separate suffix. */
		first = macro->child;
		second = first == NULL ? NULL : first->next;
		return node == macro || within_node(node, first) ||
		    within_node(node, second) ? collector->active_link : 0;
	case MAN_UR:
	case MAN_MT:
		/* man_term.c::post_UR prints the target in angle brackets after
		 * the link body.  An empty body replays the head as the label. */
		head = macro->head;
		body = macro->body;
		if (body != NULL && body->child != NULL)
			return node != body && within_node(node, body) ?
			    collector->active_link : 0;
		return head != NULL && node != head &&
		    within_node(node, head) ? collector->active_link : 0;
	case MDOC_Lk:
		first = macro->child;
		if (first == NULL)
			return 0;
		punct = macro->last;
		while (punct != first && (punct->flags & NODE_DELIMC) != 0)
			punct = punct->prev;
		punct = punct->next;
		/* mdoc_html.c::mdoc_lk_pre uses the destination as label only
		 * when there is no description; trailing delimiters are outside. */
		if (first->next == punct)
			return within_node(node, first) ? collector->active_link : 0;
		for (second = first->next; second != punct;
		    second = second->next)
			if (within_node(node, second))
				return collector->active_link;
		return 0;
	default:
		return within_node(node, macro) ? collector->active_link : 0;
	}
}

static uint32_t
add_mark(struct mant_annotated_collector *collector,
    const struct roff_node *node, const struct roff_node *origin,
    uint32_t kind, uint32_t parent, uint32_t region_kind,
    const struct roff_node *link_operand)
{
	struct mant_annotated_mark *marks, *mark;
	const struct roff_node *first, *second;
	enum mant_link_target_copy_status target_status;
	size_t name_length;
	uint32_t maximum;

	maximum = collector->session->limits->max_transfer_objects >
	    UINT32_MAX ? UINT32_MAX :
	    (uint32_t)collector->session->limits->max_transfer_objects;
	marks = mant_structured_grow_array(collector->session,
	    collector->marks, collector->mark_count,
	    &collector->mark_capacity, maximum, sizeof(*marks),
	    collector->session->limits->max_builder_allocated_bytes, 32,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (marks == NULL)
		return 0;
	collector->marks = marks;
	if (!mant_annotated_buffer_reserve_point(collector,
	    collector->mark_count, maximum))
		return 0;
	mark = marks + collector->mark_count;
	memset(mark, 0, sizeof(*mark));
	mark->key = ++collector->mark_count;
	mark->kind = kind;
	mark->parent = parent;
	mark->owner = collector->active_owner;
	mark->region_kind = region_kind;
	mark->token = node == NULL ? 0 : node->tok;
	mark->source = mant_annotated_marks_source_key(origin);
	/* read.c reparses user macros at the invocation source key, but their
	 * node positions address expanded text.  Keep source identity without
	 * claiming those positions are authored coordinates. */
	if (mark->source != 0 && origin != NULL &&
	    origin->mant_coordinate_origin == MANDOC_COORDINATE_AUTHORED &&
	    origin->line > 0 && origin->pos >= 0 &&
	    (uint64_t)origin->line <= UINT32_MAX &&
	    (uint64_t)origin->pos < UINT32_MAX) {
		mark->line = origin->line;
		mark->column = origin->pos + 1;
	}
	if (mark->source != 0 && mark->line != 0)
		mark->flags |= MANT_ANNOTATED_MARK_AUTHORED;
	if (mark->source > collector->session->result->source_count) {
		mant_annotated_fail_relation(collector, mark->source,
		    collector->session->result->source_count);
		return 0;
	}
	if (kind == MANT_ANNOTATED_MARK_HEADING) {
		/* roff.c::deroff() is also the pinned structured heading-evidence
		 * rule.  Read the HEAD, never the BLOCK's subsequent BODY. */
		if (!copy_heading_phrase(collector, mark,
		    node == NULL ? NULL : node->head))
			return 0;
	} else if (kind == MANT_ANNOTATED_MARK_ANCHOR) {
		mark->flags |= MANT_ANNOTATED_MARK_FINAL_POINT_UNVERIFIED;
		/* tag.c::tag_put()/tag_move_id() own the final NUL-terminated
		 * spelling; a moved .Tg keeps its authored source separately. */
		if (node == NULL || node->tag == NULL ||
		    (name_length = strlen(node->tag)) == 0 ||
		    !mant_structured_valid_utf8(
		    (const uint8_t *)node->tag, name_length)) {
			mant_structured_set_failure(collector->session,
			    MANT_STRUCTURED_UNSUPPORTED,
			    MANT_STRUCTURED_STAGE_RENDER, 0, 0, 0);
			return 0;
		}
		mark->name = mant_structured_copy_bytes(collector->session,
		    (const uint8_t *)node->tag, name_length, 1,
		    MANT_STRUCTURED_STAGE_RENDER);
		if (mark->name == NULL)
			return 0;
		mark->name_length = name_length;
		if (mant_tag_is_manual(node->tag))
			mark->flags |= MANT_ANNOTATED_MARK_MANUAL_TARGET;
	} else if (kind == MANT_ANNOTATED_MARK_LINK) {
		/* The macro identifies the kind; .Mt uses each operand as its
		 * authored origin.  Copy the destination while the parsed tree is
		 * alive, using the existing structured link decoder. */
		first = second = NULL;
		switch (node->tok) {
		case MAN_UR:
		case MAN_MT:
			mark->target_kind = node->tok == MAN_UR ?
			    MANT_LINK_EXTERNAL : MANT_LINK_EMAIL;
			first = node->head == NULL ? NULL : node->head->child;
			break;
		case MAN_MR:
		case MDOC_Xr:
			first = node->child;
			second = first == NULL ? NULL : first->next;
			mark->target_kind = second == NULL ?
			    MANT_LINK_DOCUMENT : MANT_LINK_MANUAL;
			break;
		case MDOC_Lk:
			mark->target_kind = MANT_LINK_EXTERNAL;
			first = node->child;
			break;
		case MDOC_Sx:
			mark->target_kind = MANT_LINK_SECTION;
			/* roff.c::deroff() can legitimately return no text for a
			 * zero-width operand.  html.c::html_make_id() then returns
			 * NULL; the macro is not a failed render or a clickable href. */
			if (!mant_structured_copy_deroff_target_allow_empty(
			    collector->session, &mark->target_a, node))
				goto unsupported_target;
			if (mark->target_a.len == 0)
				mark->target_kind = 0;
			break;
		case MDOC_Mt:
			/* Each child is one native link instance, not one address
			 * shared by the enclosing .Mt macro.  The parser and HTML
			 * formatter both require direct text children here. */
			if (link_operand == NULL || link_operand->parent != node ||
			    link_operand->type != ROFFT_TEXT) {
				mant_annotated_fail_relation(collector, mark->key, 0);
				return 0;
			}
			mark->target_kind = MANT_LINK_EMAIL;
			first = link_operand;
			break;
		default:
			break;
		}
		/* A missing operand is not an empty decoded destination.  The
		 * former yields no .UR/.MT/.Lk/.Xr/.Sx occurrence at all (filtered
		 * by push_node), except .MR's valid no-href () anchor. */
		if (node->tok != MDOC_Sx && first == NULL) {
			mark->target_kind = 0;
		}
		if (mark->target_kind != 0 && first != NULL) {
			target_status =
			    mant_structured_copy_link_target_allow_empty_classified(
			    collector->session, &mark->target_a, first);
			if (target_status == MANT_LINK_TARGET_UNSUPPORTED)
				goto rejected_target;
			if (target_status != MANT_LINK_TARGET_OK)
				goto unsupported_target;
		}
		if (second != NULL) {
			mark->target_b_present = 1;
			target_status =
			    mant_structured_copy_link_target_allow_empty_classified(
			    collector->session, &mark->target_b, second);
			if (target_status == MANT_LINK_TARGET_UNSUPPORTED)
				goto rejected_target;
			if (target_status != MANT_LINK_TARGET_OK)
				goto unsupported_target;
		}
		/* man_html.c::man_MR_pre and mdoc_html.c::mdoc_xr_pre only
		 * create a destination with nonempty name and section.  A raw
		 * operand such as \& can decode to empty while remaining a valid
		 * visible no-href instance.  Lk/UR/MT instead retain href="". */
		if ((mark->target_kind == MANT_LINK_DOCUMENT ||
		    mark->target_kind == MANT_LINK_MANUAL) &&
		    (mark->target_a.len == 0 ||
		    (mark->target_b_present && mark->target_b.len == 0))) {
			free((void *)mark->target_a.ptr);
			free((void *)mark->target_b.ptr);
			memset(&mark->target_a, 0, sizeof(mark->target_a));
			memset(&mark->target_b, 0, sizeof(mark->target_b));
			mark->target_kind = mark->target_b_present = 0;
		}
	}
	collector->metrics.mark_count = collector->mark_count;
	return mark->key;

rejected_target:
	/* An unhandled *semantic* destination escape must not stop term.c's
	 * already safe byte stream.  Retain the native macro occurrence and its
	 * visible label, but make it a no-href link and report incomplete link
	 * coverage.  Invalid UTF-8, structural data, allocation and budget
	 * failures take the distinct hard-failure branch below. */
	free((void *)mark->target_a.ptr);
	free((void *)mark->target_b.ptr);
	memset(&mark->target_a, 0, sizeof(mark->target_a));
	memset(&mark->target_b, 0, sizeof(mark->target_b));
	mark->target_kind = mark->target_b_present = 0;
	mark->flags |= MANT_ANNOTATED_MARK_LINK_REJECTED;
	collector->link_annotation_rejected = 1;
	collector->metrics.mark_count = collector->mark_count;
	return mark->key;

unsupported_target:
	if (collector->session->status == MANT_STRUCTURED_OK)
		mant_structured_set_failure(collector->session,
		    MANT_STRUCTURED_UNSUPPORTED, MANT_STRUCTURED_STAGE_RENDER,
		    0, 0, 0);
	return 0;
}

uint32_t
mant_annotated_marks_add_compatible(struct mant_annotated_collector *collector,
    const struct roff_node *node, const char *name, size_t name_length,
    const char *section, size_t section_length)
{
	struct mant_annotated_mark *mark;
	uint32_t key;

	if (node == NULL || name == NULL || section == NULL ||
	    name_length == 0 || section_length == 0) {
		mant_annotated_fail_relation(collector, name_length, section_length);
		return 0;
	}
	key = add_mark(collector, node, node,
	    MANT_ANNOTATED_MARK_LINK, 0, 0, NULL);
	if (key == 0)
		return 0;
	mark = collector->marks + key - 1;
	mark->flags |= MANT_ANNOTATED_MARK_COMPATIBLE_LINK;
	/* A text node's source position begins at the node, not necessarily at
	 * this candidate's byte range after roff expansion. Keep SourceKey, but
	 * never fabricate an authored SourceSpan for a later substring. The BR/IR
	 * macro's own authored location, in contrast, is an exact declaration. */
	if (node->type == ROFFT_TEXT) {
		mark->line = mark->column = 0;
		mark->flags &= ~MANT_ANNOTATED_MARK_AUTHORED;
	}
	mark->target_kind = MANT_LINK_MANUAL;
	mark->target_b_present = 1;
	mark->target_a.len = name_length;
	mark->target_a.ptr = mant_structured_copy_bytes(collector->session,
	    (const uint8_t *)name, name_length, 1,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (mark->target_a.ptr == NULL)
		return 0;
	mark->target_b.len = section_length;
	mark->target_b.ptr = mant_structured_copy_bytes(collector->session,
	    (const uint8_t *)section, section_length, 1,
	    MANT_STRUCTURED_STAGE_RENDER);
	return mark->target_b.ptr == NULL ? 0 : key;
}

static int
observe_html_phrase_boundary(struct mant_annotated_collector *collector,
    const struct roff_node *node)
{
	int nofill, closes;

	/* man_html.c::print_man_node() skips comments and NOPRT before fillmode
	 * or a macro handler can close an in-phrase anchor. NODE_ENTER
	 * precedes output from this node and observes the same normalized tree. */
	if (node->type == ROFFT_COMMENT || (node->flags & NODE_NOPRT) != 0)
		return 1;
	nofill = (node->flags & NODE_NOFILL) != 0;
	closes = nofill != collector->html_nofill;
	collector->html_nofill = nofill;
	/* html.c::html_fillmode() closes phrase tags on both fi->nf and
	 * nf->fi (the latter closes PRE with any nested A); tbl_html.c::
	 * html_tblopen(),
	 * roff_html.c::roff_html_pre_sp(), and the paragraph/list/section
	 * handlers in man_html.c close all active phrase tags, including A.
	 * A later nested link may open, but it cannot revive an old ancestor. */
	if (node->type == ROFFT_TBL ||
	    (node->tok == ROFF_sp && !nofill) ||
	    (node->type == ROFFT_BLOCK &&
	    (node->tok == MAN_PP || node->tok == MAN_LP ||
	    node->tok == MAN_P || node->tok == MAN_HP ||
	    node->tok == MAN_IP || node->tok == MAN_TP ||
	    node->tok == MAN_TQ || node->tok == MAN_SH ||
	    node->tok == MAN_SS || node->tok == MAN_RS ||
	    node->tok == MAN_SY)))
		closes = 1;
	if (!closes || collector->active_link == 0 ||
	    collector->active_link_epoch != collector->phrase_epoch)
		return 1;
	if (collector->phrase_epoch == UINT64_MAX) {
		mant_structured_set_failure(collector->session,
		    MANT_STRUCTURED_BUDGET, MANT_STRUCTURED_STAGE_RENDER, 29,
		    UINT64_MAX, UINT64_MAX - 1);
		return 0;
	}
	collector->phrase_epoch++;
	return 1;
}

/* mdoc_term.c::termp_fl_pre emits its generated dash before traversing the
 * child text; Va and Dv use different initial fonts, but term.c::term_word()
 * can override either with an escape. Native syntax is evidence only: C
 * never splits declaration forms or infers a name from raw spelling. */
static uint32_t
head_component_role(const struct roff_node *node)
{
	if (node->type != ROFFT_ELEM ||
	    (node->flags & NODE_NOPRT) != 0 ||
	    mant_annotated_marks_source_key(node) == 0)
		return 0;
	switch (node->tok) {
	case MDOC_Fl:
		return MANT_ANNOTATED_MARK_HEAD_OPTION;
	case MDOC_Ev:
		return MANT_ANNOTATED_MARK_HEAD_ENVIRONMENT;
	case MDOC_Va:
		return MANT_ANNOTATED_MARK_HEAD_VARIABLE;
	case MDOC_Dv:
		return MANT_ANNOTATED_MARK_HEAD_DEFINED_VARIABLE;
	case MDOC_Ic:
	case MDOC_Cm:
		return MANT_ANNOTATED_MARK_HEAD_LITERAL;
	case MDOC_Ar:
		return MANT_ANNOTATED_MARK_HEAD_ARGUMENT;
	case MAN_B:
	case MAN_SB:
		return MANT_ANNOTATED_MARK_HEAD_LEXICAL;
	default:
		return 0;
	}
}

/* man_term.c::pre_alternate() emits each TEXT child directly with
 * term_word_node(), without a child NODE_ENTER event. Keep every operand
 * interval of a lexical BI/BR HEAD: term.c::term_word() may change the font
 * inside either slot, so the initial alternating font cannot decide whether
 * a later visible name is evidence. The checked final name range decides.
 * Preserve the existing bold-slot witness for IB/RB. */

static int
alternate_component_slot(int token, uint32_t index)
{
	switch (token) {
	case MAN_BI:
	case MAN_BR:
		return 1;
	case MAN_IB:
	case MAN_RB:
		return index % 2 != 0;
	default:
		return 0;
	}
}

int
mant_annotated_marks_select_component(struct mant_annotated_collector *collector,
    const struct roff_node *word)
{
	struct annotated_frame *frame;
	struct mant_annotated_mark *parent_mark;
	const struct roff_node *child;
	uint32_t index, parent, key;

	if (word == NULL || collector->frame_count == 0)
		return 1;
	frame = collector->frames + collector->frame_count - 1;
	if (frame->node->type != ROFFT_ELEM ||
	    (frame->node->tok != MAN_BI && frame->node->tok != MAN_BR &&
	    frame->node->tok != MAN_IB && frame->node->tok != MAN_RB) ||
	    word->parent != frame->node || word == frame->alternate_child)
		return 1;
	child = frame->alternate_next_child;
	index = frame->alternate_child_index;
	while (child != NULL && child != word) {
		if (!mant_annotated_charge_work(collector, 1))
			return 0;
		child = child->next;
		index++;
	}
	if (child == NULL || index == UINT32_MAX) {
		mant_annotated_fail_relation(collector, index, 0);
		return 0;
	}
	frame->alternate_child = child;
	frame->alternate_next_child = child->next;
	frame->alternate_child_index = index + 1;
	collector->active_head_component = 0;
	if (!alternate_component_slot(frame->node->tok, index) ||
	    collector->active_owner == 0)
		return 1;
	parent = collector->active_owner;
	parent_mark = collector->marks + parent - 1;
	if (parent_mark->kind != MANT_ANNOTATED_MARK_REGION ||
	    parent_mark->region_kind != MANT_ANNOTATED_REGION_OWNER_TERM ||
	    parent_mark->parent == 0 ||
	    (collector->marks[parent_mark->parent - 1].flags &
	    (MANT_ANNOTATED_MARK_DEFINITION |
	    MANT_ANNOTATED_MARK_HANGING_CANDIDATE)) == 0)
		return 1;
	key = add_mark(collector, word, word,
	    MANT_ANNOTATED_MARK_HEAD_COMPONENT, parent, 0, NULL);
	if (key == 0)
		return 0;
	collector->marks[key - 1].flags |= MANT_ANNOTATED_MARK_HEAD_LEXICAL;
	collector->active_head_component = key;
	return 1;
}

/* mdoc_term.c::termp_fl_pre emits its own dash before the Fl operand;
 * termp_ns_pre may then glue a different macro's glyphs to it.  Preserve a
 * deliberately simple authored operand while the AST lives.  Rust must still
 * match it against final surviving output before binding any semantic name. */
static int
copy_owner_head_operand(struct mant_annotated_collector *collector,
    struct mant_annotated_mark *mark, const struct roff_node *role_node)
{
	const struct roff_node *operand;
	struct structured_session *session = collector->session;
	const char *value;
	size_t size, total;
	uint8_t *copy;

	if (role_node == NULL || (role_node->tok != MDOC_Fl &&
	    role_node->tok != MDOC_Ev))
		return 1;
	operand = role_node->child;
	/* mdoc_macro.c::macro_or_word() permits an empty Fl element before
	 * closing/middle punctuation.  The next HEAD text node is the exact
	 * delimiter printed against termp_fl_pre()'s generated dash. */
	if (operand == NULL && role_node->tok == MDOC_Fl &&
	    role_node->next != NULL &&
	    role_node->next->type == ROFFT_TEXT &&
	    role_node->next->string != NULL &&
	    (mdoc_isdelim(role_node->next->string) == DELIM_CLOSE ||
	    mdoc_isdelim(role_node->next->string) == DELIM_MIDDLE))
		operand = role_node->next;
	if (operand == NULL || operand->next != NULL ||
	    operand->type != ROFFT_TEXT || operand->string == NULL ||
	    operand->string[0] == '\0' ||
	    strpbrk(operand->string, "\\ \t\r\n") != NULL)
		return 1;
	value = operand->string;
	size = strlen(value);
	if (size == SIZE_MAX || !mant_annotated_charge_work(collector, size))
		return 0;
	if (!mant_structured_valid_utf8(
	    (const uint8_t *)value, size))
		return 1;
	total = size + (role_node->tok == MDOC_Fl);
	if (!mant_structured_charge(session, &session->content_bytes,
	    total, session->limits->max_content_bytes, 10,
	    MANT_STRUCTURED_STAGE_RENDER))
		return 0;
	copy = mant_structured_allocate(session, total, 0,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (copy == NULL)
		return 0;
	if (role_node->tok == MDOC_Fl)
		copy[0] = '-';
	memcpy(copy + total - size, value, size);
	mark->name = copy;
	mark->name_length = total;
	return 1;
}

/* Pinned man_term.c::pre_alternate() executes each direct text operand in
 * order, with no auto-space between them.  This is a narrow source-backed
 * candidate, not a scan of bold glyphs or final terminal text.  The section
 * and topic bounds use the same literal grammar as Sphinx candidates;
 * more complex escaped spellings require checked word-range mapping. */
static int
styled_manual_candidate(struct mant_annotated_collector *collector,
    const struct roff_node *node, uint8_t *decoded, size_t *decoded_length,
    const char **section_value, size_t *section_value_length)
{
	const struct roff_node *name, *section, *punct;
	enum mant_link_target_copy_status status;
	size_t name_length, section_length;

	if (node->type != ROFFT_ELEM ||
	    (node->tok != MAN_BR && node->tok != MAN_IR) ||
	    (node->flags & (NODE_NOPRT | NODE_NOFILL)) != 0 ||
	    collector->html_nofill ||
	    (collector->active_link != 0 &&
	    collector->active_link_epoch == collector->phrase_epoch))
		return 0;
	name = node->child;
	section = name == NULL ? NULL : name->next;
	punct = section == NULL ? NULL : section->next;
	if (name == NULL || section == NULL ||
	    name->type != ROFFT_TEXT || section->type != ROFFT_TEXT ||
	    ((name->flags | section->flags) & NODE_NOPRT) != 0 ||
	    name->string == NULL || section->string == NULL ||
	    (punct != NULL && (punct->next != NULL ||
	    punct->type != ROFFT_TEXT || punct->string == NULL ||
	    punct->string[0] == '\0' || punct->string[1] != '\0' ||
	    strchr(".,;:", punct->string[0]) == NULL)))
		return 0;
	name_length = 0;
	while (name_length <= 1024 && name->string[name_length] != '\0')
		name_length++;
	section_length = 0;
	while (section_length <= 18 &&
	    section->string[section_length] != '\0')
		section_length++;
	if (!mant_annotated_charge_work(collector,
	    name_length + section_length + 1))
		return -1;
	if (name_length == 0 || name_length > 1024 ||
	    section_length < 3 || section_length > 18 ||
	    section->string[0] != '(' ||
	    section->string[section_length - 1] != ')')
		return 0;
	if (!mant_annotated_refs_literal_section(section->string + 1,
	    section_length - 2))
		return 0;
	status = mant_structured_decode_link_target_slice(
	    collector->session, name->string, name_length, decoded, 256,
	    decoded_length);
	if (status == MANT_LINK_TARGET_FAILED)
		return -1;
	if (status != MANT_LINK_TARGET_OK ||
	    !mant_annotated_refs_literal_topic((const char *)decoded,
	    *decoded_length))
		return 0;
	*section_value = section->string + 1;
	*section_value_length = section_length - 2;
	return 1;
}

static int
push_node(struct mant_annotated_collector *collector,
    const struct roff_node *node)
{
	struct annotated_frame *frames, *frame;
	struct mant_annotated_mark *parent_mark;
	const struct roff_node *hanging_rs;
	const char *styled_section = NULL;
	uint8_t styled_name[256];
	size_t styled_name_length = 0, styled_section_length = 0;
	uint32_t key, region_kind, parent;
	int hanging_mode, styled_candidate;

	if (node == NULL || collector->frame_count >=
	    collector->session->limits->max_nesting_depth ||
	    collector->frame_count == UINT32_MAX) {
		mant_structured_set_failure(collector->session,
		    MANT_STRUCTURED_BUDGET, MANT_STRUCTURED_STAGE_RENDER,
		    0, (uint64_t)collector->frame_count + 1,
		    collector->session->limits->max_nesting_depth);
		return 0;
	}
	if (!observe_html_phrase_boundary(collector, node))
		return 0;
	/* The deleted first PP/P/LP leaves direct SH/SS BODY siblings.  Route
	 * exactly its stamped children through one presentation HEAD; RS itself
	 * restores the surrounding section region before its own pre handler. */
	if (collector->pending_hanging_rs == node)
		collector->active_owner =
		    collector->marks[collector->pending_hanging_owner - 1].parent;
	else if (collector->pending_hanging_scope == node->parent &&
	    ((collector->pending_hanging_mode == 1 &&
	    node->mant_elided_par_tok == collector->pending_hanging_par_tok) ||
	    (collector->pending_hanging_mode == 2 &&
	    node->flow_epoch == collector->pending_hanging_epoch)) &&
	    collector->pending_hanging_head_region != 0)
		collector->active_owner = collector->pending_hanging_head_region;
	/* man_term.c::print_man_nodelist() and mdoc_term.c::
	 * print_mdoc_nodelist() traverse the first ROOT child, not ROOT itself.
	 * The first unsectioned text therefore opens a native direct region;
	 * subsequent style nodes retain that owner across their frame leaves. */
	if (node->type == ROFFT_TEXT && collector->active_owner == 0 &&
	    !collector->in_header && !collector->in_footer) {
		if (collector->unsectioned_region == 0) {
			key = add_mark(collector, NULL, NULL,
			    MANT_ANNOTATED_MARK_REGION, 0,
			    MANT_ANNOTATED_REGION_UNSECTIONED, NULL);
			if (key == 0)
				return 0;
			if (!mant_annotated_buffer_point_now(collector, key,
			    collector->advance_count))
				return 0;
			collector->unsectioned_region = key;
		}
		collector->active_owner = collector->unsectioned_region;
	}
	frames = mant_structured_grow_array(collector->session,
	    collector->frames, collector->frame_count,
	    &collector->frame_capacity, UINT32_MAX, sizeof(*frames),
	    collector->session->limits->max_builder_allocated_bytes, 9,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (frames == NULL)
		return 0;
	collector->frames = frames;
	frame = frames + collector->frame_count++;
	frame->node = node;
	frame->saved_owner = collector->active_owner;
	frame->saved_head_component = collector->active_head_component;
	frame->saved_link = collector->active_link;
	frame->saved_link_node = collector->active_link_node;
	frame->saved_heading = collector->active_heading;
	frame->saved_link_epoch = collector->active_link_epoch;
	frame->owner_mark = frame->anchor_mark = frame->region_mark = 0;
	frame->last_direct_man_node = NULL;
	frame->last_direct_man_owner = 0;
	frame->alternate_child = NULL;
	frame->alternate_next_child = node->child;
	frame->alternate_child_index = 0;

	/* man_macro.c::blk_imp and mdoc_macro.c::blk_full produce a block
	 * with distinct HEAD/BODY scopes.  Their terminal traversal emits
	 * ENTER/LEAVE on every scope, without requiring a formatter flush. */
	if (node->type == ROFFT_BLOCK &&
	    (node->tok == MAN_SH || node->tok == MAN_SS ||
	    node->tok == MDOC_Sh || node->tok == MDOC_Ss)) {
		parent = node->tok == MAN_SS || node->tok == MDOC_Ss ?
		    collector->last_top_heading : 0;
		key = add_mark(collector, node, node,
		    MANT_ANNOTATED_MARK_HEADING, parent, 0, NULL);
		if (key == 0)
			return 0;
		if (node->tok == MAN_SS || node->tok == MDOC_Ss)
			collector->marks[key - 1].flags |=
			    MANT_ANNOTATED_MARK_SUBSECTION;
		collector->active_heading = key;
		collector->active_owner = key;
		if (node->tok == MAN_SH || node->tok == MDOC_Sh)
			collector->last_top_heading = key;
	} else if ((hanging_rs = mant_annotated_decl_hanging_successor(collector, node,
	    &hanging_mode)) != NULL) {
		const struct roff_node *origin;
		uint32_t owner_key;

		if (collector->pending_hanging_rs != NULL ||
		    collector->pending_hanging_owner != 0) {
			mant_annotated_fail_relation(collector, collector->pending_hanging_owner, 0);
			return 0;
		}
		/* The paragraph is a presentation candidate, never a C-parsed
		 * semantic name.  A rejected candidate still belongs to the
		 * section reading surface.  The declaration source is its first
		 * surviving child, not the potentially deleted paragraph boundary. */
		origin = node->type == ROFFT_BLOCK ? node->body->child : node;
		owner_key = add_mark(collector, node, origin,
		    MANT_ANNOTATED_MARK_OWNER, collector->active_owner, 0, NULL);
		if (owner_key == 0)
			return 0;
		collector->marks[owner_key - 1].flags |=
		    MANT_ANNOTATED_MARK_HANGING_CANDIDATE;
		collector->active_owner = owner_key;
		frame->owner_mark = owner_key;
		collector->pending_hanging_rs = hanging_rs;
		collector->pending_hanging_owner = owner_key;
		if (node->type == ROFFT_BLOCK) {
			collector->pending_hanging_scope = NULL;
			collector->pending_hanging_head_region = 0;
			collector->pending_hanging_mode = 0;
			collector->pending_hanging_par_tok = 0;
			collector->pending_hanging_epoch = 0;
		} else {
			key = add_mark(collector, node, origin,
			    MANT_ANNOTATED_MARK_REGION, owner_key,
			    MANT_ANNOTATED_REGION_OWNER_TERM, NULL);
			if (key == 0)
				return 0;
			collector->marks[owner_key - 1].title_region = key;
			collector->active_owner = key;
			frame->region_mark = key;
			collector->pending_hanging_scope = node->parent;
			collector->pending_hanging_head_region = key;
			collector->pending_hanging_mode = hanging_mode;
			collector->pending_hanging_par_tok =
			    hanging_mode == 1 ? node->mant_elided_par_tok : 0;
			collector->pending_hanging_epoch = node->flow_epoch;
			if (!hanging_point(collector, owner_key) ||
			    !hanging_point(collector, key))
				return 0;
		}
	} else if (node->type == ROFFT_BODY && mant_annotated_decl_paragraph_token(node->tok) &&
	    collector->frame_count > 1 &&
	    collector->frames[collector->frame_count - 2].node == node->parent &&
	    collector->frames[collector->frame_count - 2].owner_mark ==
	    collector->pending_hanging_owner &&
	    collector->pending_hanging_rs != NULL) {
		parent = collector->pending_hanging_owner;
		key = add_mark(collector, node, node->child,
		    MANT_ANNOTATED_MARK_REGION, parent,
		    MANT_ANNOTATED_REGION_OWNER_TERM, NULL);
		if (key == 0)
			return 0;
		collector->marks[parent - 1].title_region = key;
		collector->pending_hanging_head_region = key;
		collector->active_owner = key;
		frame->region_mark = key;
	} else if (node->type == ROFFT_BLOCK &&
	    (node->tok == MAN_TP || node->tok == MAN_IP ||
	    node->tok == MAN_TQ ||
	    node->tok == MDOC_It)) {
		const struct roff_node *bl;
		const struct roff_node *preceding_node, *completed_node;
		const struct annotated_frame *parent_frame;
		const struct roff_node *role_node = NULL;
		int definition, lexical_head, direct_tp_text = 0;
		uint32_t head_role = 0, preceding_key;

		/* man_term.c::pre_TP/post_TP present HEAD as a named term.
		 * mdoc_term.c::termp_it_pre uses Bl's validated list type;
		 * bullet/enum/column heads are not definition declarations. */
		definition = node->tok == MAN_TP || node->tok == MAN_TQ;
		if (node->tok == MDOC_It) {
			bl = node->parent == NULL ? NULL : node->parent->parent;
			if (bl != NULL && bl->tok == MDOC_Bl && bl->norm != NULL)
				switch (bl->norm->Bl.type) {
				case LIST_tag:
				case LIST_hang:
				case LIST_diag:
				case LIST_inset:
				case LIST_ohang:
					definition = 1;
					break;
				default:
					break;
				}
		}
		key = add_mark(collector, node, node,
		    MANT_ANNOTATED_MARK_OWNER, collector->active_owner, 0, NULL);
		if (key == 0)
			return 0;
		if (node->tok == MAN_IP &&
		    !mant_annotated_decl_ip_bold_candidate(collector, node, &definition))
			return 0;
		if (node->tok == MAN_IP && !definition &&
		    !mant_annotated_decl_ip_head_candidate(collector, node, &definition))
			return 0;
		if (definition) {
			if (node->tok == MDOC_It)
				head_role = mant_annotated_decl_owner_head_role(node, &role_node);
			else if (node->tok == MAN_IP)
				head_role = MANT_ANNOTATED_MARK_HEAD_LEXICAL;
			else if (node->tok == MAN_TP || node->tok == MAN_TQ) {
				if (!mant_annotated_decl_copy_tp_lexical_head(collector,
				    node, &lexical_head, &direct_tp_text))
					return 0;
				if (lexical_head)
					head_role = MANT_ANNOTATED_MARK_HEAD_LEXICAL;
			}
			collector->marks[key - 1].flags |=
			    MANT_ANNOTATED_MARK_DEFINITION |
			    head_role;
			if (direct_tp_text)
				collector->marks[key - 1].flags |=
				    MANT_ANNOTATED_MARK_DIRECT_TP_TEXT;
			if (!copy_owner_head_operand(collector,
			    collector->marks + key - 1, role_node))
				return 0;
			if (mant_annotated_decl_reading_family(node->tok) != 0 &&
			    head_role == MANT_ANNOTATED_MARK_HEAD_LEXICAL) {
				if (!mant_annotated_decl_direct_predecessor(collector, node,
				    &preceding_node))
					return 0;
				parent_frame = collector->frame_count > 1 ?
				    collector->frames + collector->frame_count - 2 : NULL;
				if (parent_frame == NULL) {
					completed_node = collector->last_root_man_node;
					preceding_key = collector->last_root_man_owner;
				} else if (parent_frame->node == node->parent) {
					completed_node = parent_frame->last_direct_man_node;
					preceding_key = parent_frame->last_direct_man_owner;
				} else {
					completed_node = NULL;
					preceding_key = 0;
				}
				/* A completed direct sibling and the AST's nearest eligible
				 * predecessor must identify the same owner. The prior HEAD
				 * must itself be a checked lexical definition candidate. */
				if (preceding_node != NULL &&
				    preceding_node == completed_node &&
				    preceding_key != 0 &&
				    (collector->marks[preceding_key - 1].flags &
				    (MANT_ANNOTATED_MARK_DEFINITION |
				    MANT_ANNOTATED_MARK_HEAD_LEXICAL)) ==
				    (MANT_ANNOTATED_MARK_DEFINITION |
				    MANT_ANNOTATED_MARK_HEAD_LEXICAL) &&
				    collector->marks[preceding_key - 1].parent ==
				    collector->marks[key - 1].parent)
					collector->marks[key - 1].preceding_owner =
					    preceding_key;
			}
		}
		collector->active_owner = key;
		frame->owner_mark = key;
	} else if (node->type == ROFFT_BLOCK &&
	    (node->tok == MDOC_Bl || node->tok == MDOC_Bd)) {
		region_kind = node->tok == MDOC_Bl ?
		    MANT_ANNOTATED_REGION_LIST : MANT_ANNOTATED_REGION_LITERAL;
		key = add_mark(collector, node, node,
		    MANT_ANNOTATED_MARK_REGION, collector->active_owner,
		    region_kind, NULL);
		if (key == 0)
			return 0;
		collector->active_owner = key;
		frame->region_mark = key;
	} else if (node->type == ROFFT_TBL ||
	    node->type == ROFFT_EQN) {
		if (node->type == ROFFT_TBL && collector->table_node != NULL) {
			mant_annotated_fail_relation(collector, collector->frame_count, 0);
			return 0;
		}
		region_kind = node->type == ROFFT_TBL ?
		    MANT_ANNOTATED_REGION_TABLE_SPAN :
		    MANT_ANNOTATED_REGION_EQUATION;
		key = add_mark(collector, node, node,
		    MANT_ANNOTATED_MARK_REGION, collector->active_owner,
		    region_kind, NULL);
		if (key == 0)
			return 0;
		collector->active_owner = key;
		frame->region_mark = key;
		if (node->type == ROFFT_TBL) {
			collector->table_node = node;
			collector->cell_count = 0;
			collector->table_prepared = 0;
		}
	} else if ((node->type == ROFFT_HEAD ||
	    node->type == ROFFT_BODY) &&
	    (node->tok == MAN_SH || node->tok == MAN_SS ||
	    node->tok == MDOC_Sh || node->tok == MDOC_Ss ||
	    node->tok == MAN_TP || node->tok == MAN_IP ||
	    node->tok == MAN_TQ ||
	    node->tok == MDOC_It)) {
		parent = collector->active_owner;
		parent_mark = parent == 0 ? NULL :
		    collector->marks + parent - 1;
		if (parent_mark == NULL) {
			mant_annotated_fail_relation(collector, node->tok, parent);
			return 0;
		}
		if (parent_mark->kind == MANT_ANNOTATED_MARK_HEADING)
			region_kind = node->type == ROFFT_HEAD ?
			    MANT_ANNOTATED_REGION_HEADING_TITLE :
			    MANT_ANNOTATED_REGION_HEADING_BODY;
		else if (parent_mark->kind == MANT_ANNOTATED_MARK_OWNER)
			region_kind = node->type == ROFFT_HEAD ?
			    MANT_ANNOTATED_REGION_OWNER_TERM :
			    MANT_ANNOTATED_REGION_OWNER_BODY;
		else {
			mant_annotated_fail_relation(collector, parent_mark->kind,
			    MANT_ANNOTATED_MARK_OWNER);
			return 0;
		}
		key = add_mark(collector, node, node,
		    MANT_ANNOTATED_MARK_REGION, parent, region_kind, NULL);
		if (key == 0)
			return 0;
		frame->region_mark = key;
		/* add_mark() may reallocate the array, so reselect parent. */
		parent_mark = collector->marks + parent - 1;
		if (node->type == ROFFT_HEAD)
			parent_mark->title_region = key;
		else
			parent_mark->body_region = key;
		collector->active_owner = key;
	}
	/* A component is only a definition HEAD macro instance, never a
	 * style-looking body glyph or a list label without a declaration owner. */
	if (collector->active_owner != 0 &&
	    (region_kind = head_component_role(node)) != 0) {
		parent = collector->active_owner;
		parent_mark = collector->marks + parent - 1;
		if (parent_mark->kind == MANT_ANNOTATED_MARK_REGION &&
		    parent_mark->region_kind ==
		    MANT_ANNOTATED_REGION_OWNER_TERM &&
		    parent_mark->parent != 0 &&
		    (collector->marks[parent_mark->parent - 1].flags &
	    (MANT_ANNOTATED_MARK_DEFINITION |
	    MANT_ANNOTATED_MARK_HANGING_CANDIDATE)) != 0) {
			key = add_mark(collector, node, node,
			    MANT_ANNOTATED_MARK_HEAD_COMPONENT, parent, 0, NULL);
			if (key == 0)
				return 0;
			collector->marks[key - 1].flags |= region_kind;
			collector->active_head_component = key;
		}
	}

	styled_candidate = styled_manual_candidate(collector, node,
	    styled_name, &styled_name_length, &styled_section,
	    &styled_section_length);
	if (styled_candidate < 0)
		return 0;
	if ((node->type == ROFFT_BLOCK || node->type == ROFFT_ELEM) &&
	    (node->tok == MAN_UR || node->tok == MAN_MT ||
	    node->tok == MAN_MR || node->tok == MDOC_Lk ||
	    node->tok == MDOC_Xr || node->tok == MDOC_Sx) &&
	    /* man_html.c::man_UR_pre has no anchor without a head operand;
	     * mdoc_html.c's Lk/Xr/Sx handlers emit no visible link without a
	     * child.  .MR differs: its generated () still forms an anchor. */
	    (node->tok == MAN_MR ||
	    ((node->tok == MAN_UR || node->tok == MAN_MT) ?
	    node->head != NULL && node->head->child != NULL :
	    node->child != NULL))) {
		key = add_mark(collector, node, node,
		    MANT_ANNOTATED_MARK_LINK, collector->active_link, 0, NULL);
		if (key == 0)
			return 0;
		collector->active_link = key;
		collector->active_link_node = node;
		collector->active_link_epoch = collector->phrase_epoch;
	} else if (styled_candidate) {
		key = mant_annotated_marks_add_compatible(collector, node,
		    (const char *)styled_name, styled_name_length,
		    styled_section, styled_section_length);
		if (key == 0)
			return 0;
		collector->active_link = key;
		collector->active_link_node = node;
		collector->active_link_epoch = collector->phrase_epoch;
	} else if (node->type == ROFFT_TEXT && node->parent != NULL &&
	    node->parent->tok == MDOC_Mt) {
		/* mdoc_html.c::mdoc_mt_pre gives each direct operand its own
		 * mailto anchor.  The occurrence belongs to the exact operand;
		 * post_defaults() marks a generated ~ as NODE_NOSRC. */
		key = add_mark(collector, node->parent, node,
		    MANT_ANNOTATED_MARK_LINK, collector->active_link, 0, node);
		if (key == 0)
			return 0;
		collector->active_link = key;
		collector->active_link_node = node;
		collector->active_link_epoch = collector->phrase_epoch;
	}
	if ((node->flags & NODE_ID) != 0 && node->tag != NULL &&
	    node->tag[0] != '\0') {
		const struct roff_node *origin =
		    node->mant_manual_target_source == NULL ? node :
		    node->mant_manual_target_source;
		key = add_mark(collector, node, origin,
		    MANT_ANNOTATED_MARK_ANCHOR,
		    collector->active_owner, 0, NULL);
		if (key == 0)
			return 0;
		frame->anchor_mark = key;
	}
	return 1;
}

static int
hanging_point(struct mant_annotated_collector *collector, uint32_t key)
{
	return mant_annotated_buffer_point_now(collector, key,
	    collector->advance_count);
}

/* man_term.c::print_man_node emits CHILD after each macro's pre handler.
 * The B checkpoint precedes its surviving glyphs; RS BLOCK has already
 * flushed that head, and RS BODY has computed the effective indentation.
 * No source width is reinterpreted by this collector. */
static int
observe_hanging_child(struct mant_annotated_collector *collector,
    const struct roff_node *node)
{
	struct annotated_frame *frame;
	struct mant_annotated_mark *owner, *continuation;
	uint32_t key, owner_key, saved_owner;

	if (collector->frame_count == 0)
		return 1;
	frame = collector->frames + collector->frame_count - 1;
	if (frame->node != node) {
		mant_annotated_fail_relation(collector, collector->frame_count, 0);
		return 0;
	}
	if (frame->owner_mark != 0 && node->type == ROFFT_BLOCK &&
	    mant_annotated_decl_paragraph_token(node->tok) &&
	    (collector->marks[frame->owner_mark - 1].flags &
	    MANT_ANNOTATED_MARK_HANGING_CANDIDATE) != 0)
		return hanging_point(collector, frame->owner_mark);
	if (frame->region_mark != 0 && node->type == ROFFT_BODY &&
	    mant_annotated_decl_paragraph_token(node->tok) &&
	    collector->marks[frame->region_mark - 1].region_kind ==
	    MANT_ANNOTATED_REGION_OWNER_TERM)
		return hanging_point(collector, frame->region_mark);
	if (collector->pending_hanging_rs != node &&
	    (node->type != ROFFT_BODY || node->parent !=
	    collector->pending_hanging_rs))
		return 1;
	owner_key = collector->pending_hanging_owner;
	if (owner_key == 0 || owner_key > collector->mark_count ||
	    node->tok != MAN_RS) {
		mant_annotated_fail_relation(collector, owner_key, collector->mark_count);
		return 0;
	}
	owner = collector->marks + owner_key - 1;
	if (node->type == ROFFT_BLOCK) {
		if (owner->body_region != 0 ||
		    owner->parent != collector->active_owner) {
			mant_annotated_fail_relation(collector, owner->body_region,
			    collector->active_owner);
			return 0;
		}
		/* A real, empty OwnerBody slot lets a failed candidate remain a
		 * readable presentation owner without claiming RS as its body. */
		saved_owner = collector->active_owner;
		collector->active_owner = owner_key;
		key = add_mark(collector, node, node,
		    MANT_ANNOTATED_MARK_REGION, owner_key,
		    MANT_ANNOTATED_REGION_OWNER_BODY, NULL);
		collector->active_owner = saved_owner;
		if (key == 0)
			return 0;
		collector->marks[owner_key - 1].body_region = key;
		return hanging_point(collector, key);
	}
	if (node->type != ROFFT_BODY || node->parent == NULL ||
	    node->parent->head == NULL || owner->body_region == 0 ||
	    owner->parent != collector->active_owner) {
		mant_annotated_fail_relation(collector, owner_key, collector->active_owner);
		return 0;
	}
	/* pre_RS BODY stores the device's effective offset in HEAD.  A
	 * nonpositive increment is not an indented continuation. */
	if (node->parent->head->aux > 0) {
		owner->flags |= MANT_ANNOTATED_MARK_DEFINITION |
		    MANT_ANNOTATED_MARK_HEAD_LEXICAL;
		key = add_mark(collector, node, node,
		    MANT_ANNOTATED_MARK_REGION, owner->parent,
		    MANT_ANNOTATED_REGION_HANGING_CONTINUATION, NULL);
		if (key == 0)
			return 0;
		continuation = collector->marks + key - 1;
		continuation->owner = 0;
		continuation->preceding_owner = owner_key;
		collector->active_owner = key;
		frame->region_mark = key;
		if (!hanging_point(collector, key))
			return 0;
	}
	collector->pending_hanging_rs = NULL;
	collector->pending_hanging_scope = NULL;
	collector->pending_hanging_owner = 0;
	collector->pending_hanging_head_region = 0;
	collector->pending_hanging_mode = 0;
	collector->pending_hanging_par_tok = 0;
	collector->pending_hanging_epoch = 0;
	return 1;
}

static int
text_cell(const struct tbl_cell *layout, const struct tbl_dat *data)
{
	if (layout->pos != TBL_CELL_LONG &&
	    layout->pos != TBL_CELL_CENTRE &&
	    layout->pos != TBL_CELL_LEFT &&
	    layout->pos != TBL_CELL_RIGHT &&
	    layout->pos != TBL_CELL_NUMBER)
		return 0;
	return data == NULL || data->pos == TBL_DATA_DATA ||
	    data->pos == TBL_DATA_NONE;
}

static int
prepare_table_span(struct mant_annotated_collector *collector,
    const struct roff_node *node)
{
	const struct tbl_span *span;
	const struct tbl_cell *layout;
	const struct tbl_dat *data, *current;
	struct annotated_cell *grown;
	struct mant_annotated_mark *mark;
	uint32_t columns, index, previous = UINT32_MAX, maximum, key;

	if (collector->table_node != node || collector->table_prepared ||
	    node->span == NULL || node->span->opts == NULL) {
		mant_annotated_fail_relation(collector, collector->frame_count, 0);
		return 0;
	}
	collector->table_prepared = 1;
	span = node->span;
	collector->cell_count = 0;
	if (span->pos != TBL_SPAN_DATA)
		return 1;
	if (span->opts->cols < 0 || span->layout == NULL) {
		mant_annotated_fail_relation(collector, span->opts->cols, 0);
		return 0;
	}
	columns = (uint32_t)span->opts->cols;
	if (columns == 0)
		return 1;
	maximum = collector->session->limits->max_table_cells > UINT32_MAX ?
	    UINT32_MAX :
	    (uint32_t)collector->session->limits->max_table_cells;
	if (columns > maximum) {
		mant_structured_set_failure(collector->session,
		    MANT_STRUCTURED_BUDGET, MANT_STRUCTURED_STAGE_RENDER,
		    19, columns, maximum);
		return 0;
	}
	if (!mant_annotated_charge_work(collector, columns))
		return 0;
	grown = mant_structured_grow_array(collector->session,
	    collector->cells, columns - 1, &collector->cell_capacity,
	    maximum, sizeof(*grown),
	    collector->session->limits->max_builder_allocated_bytes, 9,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (grown == NULL)
		return 0;
	collector->cells = grown;
	memset(collector->cells, 0, columns * sizeof(*collector->cells));
	collector->cell_count = columns;
	layout = span->layout->first;
	data = span->first;
	for (index = 0; layout != NULL; layout = layout->next) {
		if (layout->col < 0 || (uint32_t)layout->col >= columns ||
		    (previous != UINT32_MAX &&
		    (uint32_t)layout->col <= previous)) {
			mant_annotated_fail_relation(collector, layout->col, columns);
			return 0;
		}
		index = (uint32_t)layout->col;
		previous = index;
		current = data != NULL && data->layout == layout ? data : NULL;
		if (current != NULL)
			data = data->next;
		if (text_cell(layout, current)) {
			/* tbl_term.c::tbl_word() is the only authored text
			 * emitter.  Even when it is skipped for an empty cell,
			 * term_tbl() later reports that cell's native column. */
			key = add_mark(collector, node, NULL,
			    MANT_ANNOTATED_MARK_REGION,
			    collector->active_owner,
			    MANT_ANNOTATED_REGION_TABLE_CELL, NULL);
			if (key == 0)
				return 0;
			mark = collector->marks + key - 1;
			mark->source = mant_annotated_marks_source_key(node);
			mark->table_column = index;
			collector->cells[index].mark = key;
			collector->cells[index].data = current;
		}
	}
	return 1;
}

static void
finish_table_span(struct mant_annotated_collector *collector,
    const struct roff_node *node)
{
	uint32_t index, key;

	if (collector->table_node != node || !collector->table_prepared ||
	    collector->active_cell != NULL) {
		mant_annotated_fail_relation(collector, collector->frame_count, 0);
		return;
	}
	for (index = 0; index < collector->cell_count; index++) {
		key = collector->cells[index].mark;
		if (key != 0 &&
		    collector->marks[key - 1].table_position_present == 0) {
			mant_annotated_fail_relation(collector, key, 0);
			return;
		}
	}
	collector->table_node = NULL;
	collector->table_prepared = 0;
	collector->cell_count = 0;
}

static void
observe_table_cell(struct mant_annotated_collector *collector,
    const struct term_collector_event *event)
{
	const struct tbl_dat *data = event->cell;
	struct annotated_cell *cell;
	uint32_t column;

	if (collector->table_node == NULL || !collector->table_prepared ||
	    data == NULL || data->layout == NULL ||
	    data->layout->col < 0) {
		mant_annotated_fail_relation(collector, collector->cell_count, 0);
		return;
	}
	column = (uint32_t)data->layout->col;
	if (column >= collector->cell_count) {
		mant_annotated_fail_relation(collector, column, collector->cell_count);
		return;
	}
	cell = collector->cells + column;
	if (cell->mark == 0 || cell->data != data) {
		mant_annotated_fail_relation(collector, column, cell->mark);
		return;
	}
	if (event->phase == TERM_COLLECT_ENTER) {
		if (collector->active_cell != NULL) {
			mant_annotated_fail_relation(collector, column, 0);
			return;
		}
		collector->active_cell = data;
		collector->cell_saved_owner = collector->active_owner;
		collector->active_owner = cell->mark;
	} else if (event->phase == TERM_COLLECT_LEAVE) {
		if (collector->active_cell != data ||
		    collector->active_owner != cell->mark) {
			mant_annotated_fail_relation(collector, column, 0);
			return;
		}
		collector->active_owner = collector->cell_saved_owner;
		collector->cell_saved_owner = 0;
		collector->active_cell = NULL;
	} else
		mant_annotated_fail_relation(collector, event->phase, TERM_COLLECT_LEAVE);
}

static void
observe_table_cell_position(struct mant_annotated_collector *collector,
    const struct term_collector_event *event)
{
	struct mant_annotated_mark *mark;
	uint32_t key;

	if (collector->table_node == NULL || !collector->table_prepared ||
	    collector->active_cell != NULL || event->column >=
	    collector->cell_count) {
		mant_annotated_fail_relation(collector, event->column, collector->cell_count);
		return;
	}
	key = collector->cells[event->column].mark;
	if (key == 0)
		return; /* Native rule or span column, not a text cell. */
	mark = collector->marks + key - 1;
	if (mark->table_position_present != 0 ||
	    !mant_annotated_buffer_point_is_unused(collector, key)) {
		mant_annotated_fail_relation(collector, key, 0);
		return;
	}
	/* tbl_term.c::term_tbl reports every native text column on its first
	 * physical line, even when tbl_data() emitted no word.  The current
	 * display cursor is a real device boundary; the table offset remains
	 * a separate BU hint and is never reinterpreted as a display column. */
	if (!mant_annotated_buffer_point_now(collector, key,
	    collector->advance_count))
		return;
	mark->table_position_present = 1;
	mark->table_offset = event->pos;
}

static void
pop_node(struct mant_annotated_collector *collector,
    const struct roff_node *node)
{
	struct annotated_frame *frame, *parent_frame;
	const struct mant_annotated_mark *owner;
	const struct roff_node **last_node;
	uint32_t *last_owner;

	if (collector->frame_count == 0) {
		mant_annotated_fail_relation(collector, 0, 1);
		return;
	}
	frame = collector->frames + collector->frame_count - 1;
	if (frame->node != node) {
		mant_annotated_fail_relation(collector, collector->frame_count, 0);
		return;
	}
	if (node->type == ROFFT_TBL) {
		finish_table_span(collector, node);
		if (collector->session->status != MANT_STRUCTURED_OK)
			return;
	}
	collector->active_owner = frame->saved_owner == 0 ?
	    collector->unsectioned_region : frame->saved_owner;
	collector->active_head_component = frame->saved_head_component;
	collector->active_link = frame->saved_link;
	collector->active_link_node = frame->saved_link_node;
	collector->active_link_epoch = frame->saved_link_epoch;
	collector->active_heading = frame->saved_heading;
	collector->frame_count--;
	parent_frame = collector->frame_count == 0 ? NULL :
	    collector->frames + collector->frame_count - 1;
	if (parent_frame != NULL && parent_frame->node != node->parent)
		return; /* A skipped AST wrapper cannot prove direct adjacency. */
	last_node = parent_frame == NULL ? &collector->last_root_man_node :
	    &parent_frame->last_direct_man_node;
	last_owner = parent_frame == NULL ? &collector->last_root_man_owner :
	    &parent_frame->last_direct_man_owner;
	owner = frame->owner_mark == 0 ? NULL :
	    collector->marks + frame->owner_mark - 1;
	if (node->type == ROFFT_BLOCK &&
	    mant_annotated_decl_reading_family(node->tok) != 0 &&
	    owner != NULL &&
	    (owner->flags & MANT_ANNOTATED_MARK_DEFINITION) != 0) {
		*last_node = node;
		*last_owner = frame->owner_mark;
	} else if (!mant_annotated_decl_reading_sibling_gap(node)) {
		*last_node = NULL;
		*last_owner = 0;
	}
}

/* Structural hooks never mutate an active terminal slot.  The device bridge
 * calls this once in upstream event order; node leave does not force a flush. */
void
mant_annotated_marks_observe(struct mant_annotated_collector *collector,
    const struct term_collector_event *event)
{
	switch (event->op) {
	case TERM_COLLECT_NODE:
		if (event->phase == TERM_COLLECT_ENTER)
			(void)push_node(collector, event->node);
		else if (event->phase == TERM_COLLECT_CHILD &&
		    event->node != NULL) {
			if (!observe_hanging_child(collector, event->node))
				return;
			if (event->node->type == ROFFT_TBL)
				(void)prepare_table_span(collector, event->node);
		} else if (event->phase == TERM_COLLECT_LEAVE)
			pop_node(collector, event->node);
		return;
	case TERM_COLLECT_TABLE_CELL:
		observe_table_cell(collector, event);
		return;
	case TERM_COLLECT_TABLE_CELL_POSITION:
		observe_table_cell_position(collector, event);
		return;
	default:
		return;
	}
}

uint32_t
mant_annotated_marks_margin(struct mant_annotated_collector *collector)
{
	uint32_t key;

	key = add_mark(collector, NULL, NULL, MANT_ANNOTATED_MARK_REGION,
	    collector->margin_owner, MANT_ANNOTATED_REGION_MARGIN, NULL);
	if (key == 0)
		return 0;
	collector->marks[key - 1].owner = collector->margin_owner;
	return mant_annotated_buffer_point_now(collector, key, 0) ? key : 0;
}
