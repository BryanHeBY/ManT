/* Native list/item ownership derived from balanced formatter node phases. */
#include "config.h"

#include <stdio.h>
#include <string.h>

#include "mandoc.h"
#include "roff.h"
#include "mdoc.h"
#include "tag.h"

#include "mant_mandoc_structured_address.h"
#include "mant_mandoc_structured_builder.h"
#include "mant_mandoc_structured_marker.h"
#include "mant_mandoc_structured_structure.h"

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
	if (context != NULL && context->owner != 0)
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

static uint32_t
append_list(struct structured_session *session,
    struct structured_node_context *context, const struct roff_node *node,
    uint32_t kind, uint32_t block_kind, uint32_t start, int compact)
{
	struct mant_structured_list_view *lists, *list;
	struct structured_list_state *states, *state;
	uint32_t owner, parent, provenance, block;

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
	states = mant_structured_grow_array(session, session->list_states,
	    session->result->list_count, &session->list_state_capacity,
	    mant_structured_limit_u32(session->limits->max_blocks), sizeof(*states),
	    session->limits->max_builder_allocated_bytes, 12,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (states == NULL)
		return 0;
	session->list_states = states;
	state = states + session->result->list_count;
	memset(state, 0, sizeof(*state));
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
	context->owner = owner;
	context->list = list->key;
	/* The new list owns its active item and pending target scope. */
	context->item = 0;
	context->container_block = block;
	return list->key;
}

static uint32_t
append_item(struct structured_session *session,
    struct structured_node_context *context, const struct roff_node *node,
    uint32_t list_key)
{
	struct mant_structured_item_view *items, *item;
	const struct mant_structured_list_view *list;
	struct structured_list_state *state;
	uint32_t kind, owner, provenance;

	if (list_key == 0 || list_key > session->result->list_count)
		return 0;
	list = session->result->lists + list_key - 1;
	state = session->list_states + list_key - 1;
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
	item->ordinal = state->item_count++;
	item->provenance = provenance;
	mant_structured_address_bind_item(session, list_key, owner);
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
		    mant_structured_man_marker_reaches(
		    session->last_man_marker_node, node)) {
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
		marker_style = MANT_STRUCTURED_MAN_MARKER_NONE;
		start = 0;
		if (node->tok != MAN_TQ &&
		    mant_structured_man_named_bullet(node)) {
			kind = MANT_LIST_BULLET;
			block_kind = MANT_BLOCK_LIST;
			marker_style = MANT_STRUCTURED_MAN_MARKER_BULLET;
			marker = 1;
		} else if (node->tok != MAN_TQ &&
		    (start = mant_structured_man_ordinal_start(node,
		    &marker_style)) != 0) {
			kind = MANT_LIST_ORDERED;
			block_kind = MANT_BLOCK_LIST;
			marker = 1;
		} else {
			kind = MANT_LIST_DEFINITION;
			block_kind = MANT_BLOCK_DEFINITION_LIST;
		}
		if (marker && session->last_man_marker_node != NULL &&
		    mant_structured_man_marker_reaches(
		    session->last_man_marker_node, node) &&
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
	}
	return 1;
}

void
mant_structured_leave_node(struct structured_session *session,
    const struct roff_node *node)
{
	struct structured_node_context *context;

	context = mant_structured_current_context(session);
	if (context != NULL && node != NULL && node->type == ROFFT_BLOCK &&
	    node->tok == MDOC_It && context->item != 0 &&
	    mant_structured_address_owner_needs_root(session, context->owner)) {
		uint32_t provenance;

		provenance = mant_structured_append_provenance(session, node, 1);
		if (provenance != 0) {
			int has_root = 0;
			uint32_t index;

			for (index = 0; index < session->result->content_root_count;
			    index++)
				if (session->result->content_roots[index].owner ==
				    context->owner) {
					has_root = 1;
					break;
				}
			if (!has_root && mant_structured_open_content_root(session, 0,
			    provenance))
				mant_structured_address_root_opened(session, node, 0);
			mant_structured_address_finish_owner(session, context->owner);
		}
	}
	if (context != NULL && node != NULL && node->type == ROFFT_HEAD &&
	    context->part == STRUCTURED_PART_TERM && context->term_root != 0)
		mant_structured_close_term_root(session, context->term_root,
		    context->item);
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
	}
}
