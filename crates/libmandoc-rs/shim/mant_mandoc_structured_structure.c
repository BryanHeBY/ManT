/* Native list/item ownership derived from balanced formatter node phases. */
#include "config.h"

#include <stdio.h>
#include <string.h>

#include "mandoc.h"
#include "roff.h"
#include "mdoc.h"

#include "mant_mandoc_structured_builder.h"
#include "mant_mandoc_structured_structure.h"

enum man_marker_style {
	MAN_MARKER_NONE,
	MAN_MARKER_BULLET,
	MAN_MARKER_DOT,
	MAN_MARKER_PAREN_SUFFIX,
	MAN_MARKER_PAREN_PAIR
};

static uint32_t
context_parent(const struct structured_session *session,
    const struct structured_node_context *context)
{
	if (context != NULL && context->container_block != 0)
		return context->container_block;
	return session->section_heading_block;
}

static uint32_t
context_owner(struct structured_session *session,
    const struct structured_node_context *context, uint32_t provenance)
{
	if (context != NULL && context->item != 0)
		return context->owner;
	if (session->section_owner == 0)
		session->section_owner = mant_structured_append_owner(session,
		    MANT_OWNER_DOCUMENT, provenance);
	return session->section_owner;
}

static int
mdoc_list_kind(const struct roff_node *node, uint32_t *kind,
    uint32_t *block_kind)
{
	if (node->norm == NULL)
		return 0;
	switch (node->norm->Bl.type) {
	case LIST_bullet:
	case LIST_dash:
	case LIST_hyphen:
		*kind = MANT_LIST_BULLET;
		*block_kind = MANT_BLOCK_LIST;
		return 1;
	case LIST_enum:
		*kind = MANT_LIST_ORDERED;
		*block_kind = MANT_BLOCK_LIST;
		return 1;
	case LIST_item:
		*kind = MANT_LIST_PLAIN;
		*block_kind = MANT_BLOCK_LIST;
		return 1;
	case LIST_diag:
	case LIST_hang:
	case LIST_inset:
	case LIST_ohang:
	case LIST_tag:
		*kind = MANT_LIST_DEFINITION;
		*block_kind = MANT_BLOCK_DEFINITION_LIST;
		return 1;
	default:
		return 0;
	}
}

static const struct roff_node *
first_text(const struct roff_node *node)
{
	const struct roff_node *found;

	for (; node != NULL; node = node->next) {
		if (node->type == ROFFT_TEXT && node->string != NULL)
			return node;
		if ((found = first_text(node->child)) != NULL)
			return found;
	}
	return NULL;
}

/*
 * TP accepts an optional width on the macro line, but pre_TP() deliberately
 * starts rendering its tag at the first NODE_LINE child.  Classification has
 * to use the same boundary or a numeric width can masquerade as a marker.
 */
static const struct roff_node *
man_marker_text(const struct roff_node *node)
{
	const struct roff_node *head;

	head = node == NULL ? NULL : node->head;
	if (node != NULL && node->tok == MAN_TP && head != NULL) {
		head = head->child;
		while (head != NULL && (head->flags & NODE_LINE) == 0)
			head = head->next;
		return first_text(head);
	}
	return first_text(head);
}

/*
 * man_macro.c::blk_exp closes an implicit IP/TP before opening RS, leaving
 * the relative-indent block as an AST sibling.  The formatter nevertheless
 * renders consecutive RS siblings as content of the preceding visible item.
 */
static int
man_marker_reaches(const struct roff_node *marker,
    const struct roff_node *node)
{
	if (marker == NULL || node == NULL)
		return 0;
	for (marker = marker->next; marker != NULL && marker != node;
	    marker = marker->next)
		if (marker->type != ROFFT_BLOCK || marker->tok != MAN_RS)
			return 0;
	return marker == node;
}

static const char *
skip_marker_decoration(const char *text)
{
	for (;;) {
		while (*text == ' ' || *text == '\t')
			text++;
		if (text[0] == '\\' && text[1] == ' ') {
			text += 2;
			continue;
		}
		if (text[0] == '\\' && text[1] == 'f' && text[2] != '\0') {
			if (text[2] == '[') {
				const char *end = strchr(text + 3, ']');
				if (end == NULL)
					return text;
				text = end + 1;
			} else
				text += 3;
			continue;
		}
		return text;
	}
}

static int
man_named_bullet(const struct roff_node *node)
{
	const struct roff_node *text_node;
	const char *text;

	text_node = man_marker_text(node);
	if (text_node == NULL)
		return 0;
	text = skip_marker_decoration(text_node->string);
	if (strncmp(text, "\\(bu", 4) == 0)
		text += 4;
	else if (strncmp(text, "\\[bu]", 5) == 0)
		text += 5;
	else
		return 0;
	return *skip_marker_decoration(text) == '\0';
}

static uint32_t
man_ordinal_start(const struct roff_node *node, uint32_t *style)
{
	const struct roff_node *text_node;
	const char *text;
	uint64_t value;
	int parenthesized;

	text_node = man_marker_text(node);
	if (text_node == NULL)
		return 0;
	text = skip_marker_decoration(text_node->string);
	parenthesized = *text == '(';
	if (parenthesized)
		text++;
	if (*text < '0' || *text > '9')
		return 0;
	value = 0;
	while (*text >= '0' && *text <= '9') {
		value = value * 10 + (unsigned int)(*text++ - '0');
		if (value > UINT32_MAX)
			return 0;
	}
	if (parenthesized) {
		if (*text++ != ')')
			return 0;
		*style = MAN_MARKER_PAREN_PAIR;
	} else if (*text == '.') {
		*style = MAN_MARKER_DOT;
		text++;
	} else if (*text == ')') {
		*style = MAN_MARKER_PAREN_SUFFIX;
		text++;
	} else
		return 0;
	return *skip_marker_decoration(text) == '\0' && value != 0 ?
	    (uint32_t)value : 0;
}

static uint32_t
append_list(struct structured_session *session,
    struct structured_node_context *context, const struct roff_node *node,
    uint32_t kind, uint32_t block_kind, uint32_t start, int compact)
{
	struct mant_structured_list_view *lists, *list;
	uint32_t *counts, owner, parent, provenance, block;

	provenance = mant_structured_append_provenance(session, node, 1);
	owner = context_owner(session, context, provenance);
	parent = context_parent(session, context);
	if (provenance == 0 || owner == 0)
		return 0;
	block = mant_structured_append_block(session, owner, block_kind, parent,
	    provenance, 0);
	if (block == 0)
		return 0;
	lists = mant_structured_grow_array(session, session->result->lists,
	    session->result->list_count, &session->result->list_capacity,
	    mant_structured_limit_u32(session->limits->max_blocks), sizeof(*lists),
	    session->limits->max_builder_allocated_bytes, 12,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (lists == NULL)
		return 0;
	session->result->lists = lists;
	counts = mant_structured_grow_array(session, session->list_item_counts,
	    session->result->list_count, &session->list_item_capacity,
	    mant_structured_limit_u32(session->limits->max_blocks), sizeof(*counts),
	    session->limits->max_builder_allocated_bytes, 12,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (counts == NULL)
		return 0;
	session->list_item_counts = counts;
	counts[session->result->list_count] = 0;
	if (!mant_structured_charge(session, &session->builder_operations, 1,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_RENDER) ||
	    !mant_structured_charge(session, &session->relation_edges, 2,
	    session->limits->max_relation_edges, 30,
	    MANT_STRUCTURED_STAGE_RENDER))
		return 0;
	list = lists + session->result->list_count;
	memset(list, 0, sizeof(*list));
	list->key = ++session->result->list_count;
	list->block = block;
	list->kind = kind;
	list->compact = compact != 0;
	list->start = kind == MANT_LIST_ORDERED ? start : 0;
	list->provenance = provenance;
	context->list = list->key;
	context->container_block = block;
	return list->key;
}

static int
set_item_target(struct structured_session *session, uint32_t item_key,
    const struct roff_node *node)
{
	struct mant_structured_item_view *item;
	size_t target_length;

	if (item_key == 0 || item_key > session->result->item_count ||
	    node == NULL || (node->flags & NODE_ID) == 0 || node->tag == NULL ||
	    node->tag[0] == '\0')
		return 1;
	item = session->result->items + item_key - 1;
	if (item->target_present != 0)
		return 1;
	target_length = strlen(node->tag);
	if (!mant_structured_valid_utf8((const uint8_t *)node->tag,
	    target_length))
		return 1;
	item->target.ptr = mant_structured_copy_bytes(session,
	    (const uint8_t *)node->tag, target_length, 1,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (item->target.ptr == NULL)
		return 0;
	item->target.len = target_length;
	item->target_present = 1;
	return 1;
}

static uint32_t
append_item(struct structured_session *session,
    struct structured_node_context *context, const struct roff_node *node,
    uint32_t list_key)
{
	struct mant_structured_item_view *items, *item;
	const struct mant_structured_list_view *list;
	uint32_t kind, owner, provenance;

	if (list_key == 0 || list_key > session->result->list_count)
		return 0;
	list = session->result->lists + list_key - 1;
	provenance = mant_structured_append_provenance(session, node, 1);
	kind = list->kind == MANT_LIST_DEFINITION ||
	    list->kind == MANT_LIST_NATIVE_MARKER ?
	    MANT_OWNER_DEFINITION_ITEM : MANT_OWNER_LIST_ITEM;
	owner = mant_structured_append_owner(session, kind, provenance);
	if (provenance == 0 || owner == 0)
		return 0;
	items = mant_structured_grow_array(session, session->result->items,
	    session->result->item_count, &session->result->item_capacity,
	    mant_structured_limit_u32(session->limits->max_owners), sizeof(*items),
	    session->limits->max_builder_allocated_bytes, 11,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (items == NULL)
		return 0;
	session->result->items = items;
	if (!mant_structured_charge(session, &session->builder_operations, 1,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_RENDER) ||
	    !mant_structured_charge(session, &session->relation_edges, 3,
	    session->limits->max_relation_edges, 30,
	    MANT_STRUCTURED_STAGE_RENDER))
		return 0;
	item = items + session->result->item_count;
	memset(item, 0, sizeof(*item));
	item->key = ++session->result->item_count;
	item->list = list_key;
	item->owner = owner;
	item->ordinal = session->list_item_counts[list_key - 1]++;
	item->provenance = provenance;
	if (!set_item_target(session, item->key, node))
		return 0;
	if (session->pending_item_target != NULL) {
		if (!set_item_target(session, item->key,
		    session->pending_item_target))
			return 0;
		session->pending_item_target = NULL;
	}
	context->owner = owner;
	context->item = item->key;
	context->list = list_key;
	context->container_block = list->block;
	return item->key;
}

struct structured_node_context *
mant_structured_current_context(struct structured_session *session)
{
	return session->node_depth == 0 ? NULL :
	    session->node_contexts + session->node_depth - 1;
}

int
mant_structured_enter_node(struct structured_session *session,
    const struct roff_node *node)
{
	struct structured_node_context inherited, *contexts, *context;
	uint32_t block_kind, kind, list, marker_style, start;
	int marker;

	memset(&inherited, 0, sizeof(inherited));
	if (session->node_depth > 1)
		inherited = session->node_contexts[session->node_depth - 2];
	contexts = mant_structured_grow_array(session, session->node_contexts,
	    session->node_depth - 1, &session->node_context_capacity,
	    mant_structured_limit_u32(session->limits->max_nesting_depth),
	    sizeof(*contexts), session->limits->max_builder_allocated_bytes, 35,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (contexts == NULL)
		return 0;
	session->node_contexts = contexts;
	context = contexts + session->node_depth - 1;
	*context = inherited;
	context->term_root = 0;
	context->restore_man_state = 0;
	if (node == NULL)
		return 1;
	if (node->type == ROFFT_BLOCK && node->tok == MAN_RS) {
		context->saved_man_marker_node = session->last_man_marker_node;
		context->saved_man_item = session->last_man_item;
		context->saved_man_list = session->last_man_list;
		context->saved_man_parent_block = session->last_man_parent_block;
		context->saved_man_marker_list = session->last_man_marker_list;
		context->saved_man_marker_kind = session->last_man_marker_kind;
		context->saved_man_marker_style = session->last_man_marker_style;
		context->saved_man_marker_ordinal = session->next_man_marker_ordinal;
		context->restore_man_state = 1;
		if (session->last_man_item != 0 &&
		    session->last_man_marker_list != 0 &&
		    session->last_man_parent_block == context_parent(session, context) &&
		    man_marker_reaches(session->last_man_marker_node, node)) {
			context->item = session->last_man_item;
			context->list = session->last_man_marker_list;
			context->owner = session->result->items[
			    context->item - 1].owner;
			context->container_block = session->result->lists[
			    context->list - 1].block;
		}
	}
	if (node->type == ROFFT_BLOCK && node->tok == MDOC_Bl) {
		if (!mdoc_list_kind(node, &kind, &block_kind))
			return 0;
		return append_list(session, context, node, kind, block_kind,
		    kind == MANT_LIST_ORDERED ? 1 : 0, node->norm->Bl.comp) != 0;
	}
	if (node->type == ROFFT_BLOCK && node->tok == MDOC_It) {
		list = context->list;
		return append_item(session, context, node, list) != 0;
	}
	if (node->type == ROFFT_BLOCK &&
	    (node->tok == MAN_IP || node->tok == MAN_TP ||
	    node->tok == MAN_TQ)) {
		if (node->tok == MAN_TQ && session->man_continuation_pending != 0 &&
		    session->last_man_item != 0 &&
		    session->last_man_list != 0 &&
		    session->result->lists[session->last_man_list - 1].kind ==
		    MANT_LIST_DEFINITION &&
		    session->last_man_parent_block == context_parent(session, context)) {
			context->item = session->last_man_item;
			context->list = session->last_man_list;
			context->owner = session->result->items[
			    context->item - 1].owner;
			context->container_block = session->result->lists[
			    context->list - 1].block;
			return 1;
		}
		marker = 0;
		marker_style = MAN_MARKER_NONE;
		start = 0;
		if (node->tok != MAN_TQ && man_named_bullet(node)) {
			kind = MANT_LIST_BULLET;
			block_kind = MANT_BLOCK_LIST;
			marker_style = MAN_MARKER_BULLET;
			marker = 1;
		} else if (node->tok != MAN_TQ &&
		    (start = man_ordinal_start(node, &marker_style)) != 0) {
			kind = MANT_LIST_ORDERED;
			block_kind = MANT_BLOCK_LIST;
			marker = 1;
		} else {
			kind = MANT_LIST_DEFINITION;
			block_kind = MANT_BLOCK_DEFINITION_LIST;
		}
		if (marker && session->last_man_marker_node != NULL &&
		    man_marker_reaches(session->last_man_marker_node, node) &&
		    session->last_man_parent_block == context_parent(session, context) &&
		    session->last_man_marker_kind == kind &&
		    session->last_man_marker_style == marker_style &&
		    (kind != MANT_LIST_ORDERED ||
		    session->next_man_marker_ordinal == start))
			list = session->last_man_marker_list;
		else
			list = append_list(session, context, node, kind, block_kind,
			    start, 0);
		if (list == 0 || append_item(session, context, node, list) == 0)
			return 0;
		if (marker) {
			session->last_man_marker_node = node;
			session->last_man_marker_list = list;
			session->last_man_marker_kind = kind;
			session->last_man_marker_style = marker_style;
			session->next_man_marker_ordinal = kind == MANT_LIST_ORDERED &&
			    start != UINT32_MAX ? start + 1 : 0;
		}
		session->last_man_item = context->item;
		session->last_man_list = context->list;
		session->last_man_parent_block = context_parent(session,
		    session->node_depth > 1 ?
		    session->node_contexts + session->node_depth - 2 : NULL);
		session->man_continuation_pending = node->tok != MAN_IP &&
		    session->result->lists[context->list - 1].kind ==
		    MANT_LIST_DEFINITION;
		return 1;
	}
	if (node->tok == MDOC_Tg && (node->flags & NODE_ID) != 0 &&
	    context->list != 0 && context->item == 0)
		session->pending_item_target = node;
	if ((node->tok == MAN_PP || node->tok == MAN_LP || node->tok == MAN_P ||
	    node->tok == MAN_IP || node->tok == MAN_TP) &&
	    node->type != ROFFT_HEAD && node->type != ROFFT_BODY)
		session->man_continuation_pending = 0;
	if ((node->tok == MAN_RS || node->tok == MAN_RE) &&
	    node->type != ROFFT_HEAD && node->type != ROFFT_BODY)
		session->man_continuation_pending = 0;
	if (context->item != 0 && node->type == ROFFT_HEAD &&
	    (node->tok == MDOC_It || node->tok == MAN_IP ||
	    node->tok == MAN_TP || node->tok == MAN_TQ))
		context->part = STRUCTURED_PART_TERM;
	else if (context->item != 0 && node->type == ROFFT_BODY &&
	    (node->tok == MDOC_It || node->tok == MAN_IP ||
	    node->tok == MAN_TP || node->tok == MAN_TQ))
		context->part = STRUCTURED_PART_BODY;
	if (context->item != 0 && (node->type == ROFFT_HEAD ||
	    node->type == ROFFT_BODY) &&
	    (node->tok == MDOC_It || node->tok == MAN_IP ||
	    node->tok == MAN_TP || node->tok == MAN_TQ)) {
		session->current_root = 0;
		session->current_owner = 0;
		session->current_root_atom_count = 0;
	}
	if (!set_item_target(session, context->item, node))
		return 0;
	return 1;
}

void
mant_structured_leave_node(struct structured_session *session,
    const struct roff_node *node)
{
	struct structured_node_context *context;

	context = mant_structured_current_context(session);
	if (context != NULL && node != NULL && node->type == ROFFT_HEAD &&
	    context->part == STRUCTURED_PART_TERM && context->term_root != 0)
		mant_structured_finalize_term_root(session, context->term_root,
		    context->item);
	if (node != NULL && node->type == ROFFT_BLOCK && node->tok == MDOC_Bl)
		session->pending_item_target = NULL;
	if (node != NULL && node->type == ROFFT_BLOCK && node->tok == MAN_RS) {
		session->man_continuation_pending = 0;
		if (context != NULL && context->restore_man_state != 0) {
			session->last_man_marker_node =
			    context->saved_man_marker_node;
			session->last_man_item = context->saved_man_item;
			session->last_man_list = context->saved_man_list;
			session->last_man_parent_block =
			    context->saved_man_parent_block;
			session->last_man_marker_list =
			    context->saved_man_marker_list;
			session->last_man_marker_kind =
			    context->saved_man_marker_kind;
			session->last_man_marker_style =
			    context->saved_man_marker_style;
			session->next_man_marker_ordinal =
			    context->saved_man_marker_ordinal;
		}
	}
	if (node != NULL && (((node->type == ROFFT_HEAD ||
	    node->type == ROFFT_BODY) &&
	    (node->tok == MDOC_It || node->tok == MAN_IP ||
	    node->tok == MAN_TP || node->tok == MAN_TQ)) ||
	    (node->type == ROFFT_BLOCK &&
	    (node->tok == MDOC_Bl || node->tok == MDOC_It ||
	    node->tok == MAN_IP || node->tok == MAN_TP ||
	    node->tok == MAN_TQ)))) {
		session->current_root = 0;
		session->current_owner = 0;
		session->current_root_atom_count = 0;
	}
}
