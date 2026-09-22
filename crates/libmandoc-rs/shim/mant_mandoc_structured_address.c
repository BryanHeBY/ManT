/* Zero-width native targets, content points, and authored heading evidence. */
#include "config.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "mandoc.h"
#include "roff.h"
#include "mdoc.h"
#include "tag.h"

#include "mant_mandoc_structured_address.h"
#include "mant_mandoc_structured_builder.h"
#include "mant_mandoc_structured_structure.h"

struct structured_anchor_state {
	uint64_t threshold;
	uint32_t owner;
	uint32_t root;
	uint32_t next;
	uint8_t queue_kind;
	uint8_t resolved;
};

enum structured_anchor_queue_kind {
	ANCHOR_QUEUE_NONE = 0,
	ANCHOR_QUEUE_UNOWNED,
	ANCHOR_QUEUE_LIST,
	ANCHOR_QUEUE_OWNER,
	ANCHOR_QUEUE_ROOT
};

static int
queue_push(struct structured_session *session,
    struct structured_anchor_queue *queue, uint32_t key, uint8_t kind)
{
	struct structured_anchor_state *state;

	if (key == 0 || key > session->result->anchor_count)
		return 0;
	state = session->anchor_states + key - 1;
	if (state->queue_kind != ANCHOR_QUEUE_NONE || state->resolved) {
		mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
		    MANT_STRUCTURED_STAGE_RENDER, 0, key, state->queue_kind);
		return 0;
	}
	if (!mant_structured_charge(session, &session->builder_operations, 1,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_RENDER))
		return 0;
	state->next = 0;
	state->queue_kind = kind;
	if (queue->tail == 0)
		queue->head = key;
	else
		session->anchor_states[queue->tail - 1].next = key;
	queue->tail = key;
	return 1;
}

static uint32_t
queue_pop(struct structured_session *session,
    struct structured_anchor_queue *queue, uint8_t kind)
{
	struct structured_anchor_state *state;
	uint32_t key;

	key = queue->head;
	if (key == 0)
		return 0;
	state = session->anchor_states + key - 1;
	if (state->queue_kind != kind) {
		mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
		    MANT_STRUCTURED_STAGE_RENDER, 0, key, state->queue_kind);
		return 0;
	}
	if (!mant_structured_charge(session, &session->builder_operations, 1,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_RENDER))
		return 0;
	queue->head = state->next;
	if (queue->head == 0)
		queue->tail = 0;
	state->next = 0;
	state->queue_kind = ANCHOR_QUEUE_NONE;
	return key;
}

static struct structured_anchor_queue *
owner_queue(struct structured_session *session, uint32_t owner)
{
	struct structured_anchor_queue *queues;
	uint32_t old_capacity;

	if (owner == 0 || owner > session->result->owner_count)
		return NULL;
	old_capacity = session->owner_anchor_queue_capacity;
	queues = mant_structured_grow_array(session,
	    session->owner_anchor_queues, owner - 1,
	    &session->owner_anchor_queue_capacity,
	    mant_structured_limit_u32(session->limits->max_owners),
	    sizeof(*queues), session->limits->max_builder_allocated_bytes, 37,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (queues == NULL)
		return NULL;
	if (session->owner_anchor_queue_capacity > old_capacity)
		memset(queues + old_capacity, 0,
		    (size_t)(session->owner_anchor_queue_capacity - old_capacity) *
		    sizeof(*queues));
	session->owner_anchor_queues = queues;
	return queues + owner - 1;
}

static uint32_t
append_point(struct structured_session *session, uint32_t root_key,
    uint32_t atom_boundary)
{
	const struct mant_structured_content_root_view *root;
	struct mant_structured_content_point_view *points, *point;
	uint64_t scalar_boundary;

	if (root_key == 0 || root_key > session->result->content_root_count ||
	    atom_boundary > session->root_atoms[root_key - 1].count)
		return 0;
	scalar_boundary = session->root_atoms[root_key - 1].scalar_count;
	if (scalar_boundary > UINT32_MAX)
		goto overflow;
	if (!mant_structured_charge(session, &session->builder_operations, 1,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_RENDER) ||
	    !mant_structured_charge(session, &session->relation_edges, 3,
	    session->limits->max_relation_edges, 30,
	    MANT_STRUCTURED_STAGE_RENDER))
		return 0;
	points = mant_structured_grow_array(session, session->result->content_points,
	    session->result->content_point_count,
	    &session->result->content_point_capacity,
	    mant_structured_limit_u32(session->limits->max_content_points),
	    sizeof(*points), session->limits->max_builder_allocated_bytes, 15,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (points == NULL)
		return 0;
	session->result->content_points = points;
	root = session->result->content_roots + root_key - 1;
	point = points + session->result->content_point_count;
	memset(point, 0, sizeof(*point));
	point->key = ++session->result->content_point_count;
	point->root = root_key;
	point->ordinal = session->root_atoms[root_key - 1].point_count++;
	point->owner = root->owner;
	point->boundary_kind = MANT_POINT_BETWEEN_ATOMS;
	point->atom_boundary = atom_boundary;
	point->scalar_boundary = (uint32_t)scalar_boundary;
	point->provenance = root->provenance;
	return point->key;

overflow:
	mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET,
	    MANT_STRUCTURED_STAGE_RENDER, 15, UINT64_MAX, UINT32_MAX);
	return 0;
}

static const char *
node_target(const struct roff_node *node)
{
	if (node == NULL)
		return NULL;
	if (node->tag != NULL && node->tag[0] != '\0')
		return node->tag;
	if (node->child != NULL && node->child->type == ROFFT_TEXT &&
	    node->child->string != NULL && node->child->string[0] != '\0')
		return node->child->string;
	return NULL;
}

static uint32_t
take_authored_from_queue(struct structured_session *session,
    struct structured_anchor_queue *queue, uint8_t queue_kind,
    const char *target)
{
	const struct mant_structured_anchor_view *anchor;
	struct structured_anchor_state *state;
	size_t length;
	uint32_t key, previous;

	length = strlen(target);
	previous = 0;
	for (key = queue->head; key != 0; key = state->next) {
		state = session->anchor_states + key - 1;
		if (state->queue_kind != queue_kind) {
			mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
			    MANT_STRUCTURED_STAGE_RENDER, 0, key,
			    state->queue_kind);
			return 0;
		}
		if (!mant_structured_charge(session, &session->builder_operations, 1,
		    session->limits->max_builder_operations, 8,
		    MANT_STRUCTURED_STAGE_RENDER))
			return 0;
		anchor = session->result->anchors + key - 1;
		if (anchor->origin == MANT_TARGET_ORIGIN_AUTHORED &&
		    anchor->target.len == length &&
		    memcmp(anchor->target.ptr, target, length) == 0) {
			if (!mant_structured_charge(session,
			    &session->builder_operations, 1,
			    session->limits->max_builder_operations, 8,
			    MANT_STRUCTURED_STAGE_RENDER))
				return 0;
			if (previous == 0)
				queue->head = state->next;
			else
				session->anchor_states[previous - 1].next = state->next;
			if (queue->tail == key)
				queue->tail = previous;
			state->next = 0;
			state->queue_kind = ANCHOR_QUEUE_NONE;
			return key;
		}
		previous = key;
	}
	return 0;
}

static uint32_t
take_authored_anchor(struct structured_session *session, const char *target,
    uint32_t owner)
{
	struct structured_anchor_queue *queue;
	uint32_t key;

	if (owner != 0 && owner <= session->owner_anchor_queue_capacity) {
		queue = session->owner_anchor_queues + owner - 1;
		key = take_authored_from_queue(session, queue,
		    ANCHOR_QUEUE_OWNER, target);
		if (key != 0 || session->status != MANT_STRUCTURED_OK)
			return key;
	}
	return take_authored_from_queue(session, &session->unowned_anchors,
	    ANCHOR_QUEUE_UNOWNED, target);
}

static uint32_t
append_anchor(struct structured_session *session, const char *target,
    uint32_t origin, uint32_t provenance)
{
	struct mant_structured_anchor_view *anchors, *anchor;
	struct structured_anchor_state *states, *state;
	size_t length;

	if (target == NULL || target[0] == '\0')
		return 0;
	length = strlen(target);
	if (!mant_structured_valid_utf8((const uint8_t *)target, length))
		return 0;
	if (!mant_structured_charge(session, &session->builder_operations, 1,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_RENDER) ||
	    !mant_structured_charge(session, &session->relation_edges, 3,
	    session->limits->max_relation_edges, 30,
	    MANT_STRUCTURED_STAGE_RENDER))
		return 0;
	anchors = mant_structured_grow_array(session, session->result->anchors,
	    session->result->anchor_count, &session->result->anchor_capacity,
	    mant_structured_limit_u32(session->limits->max_anchor_evidence),
	    sizeof(*anchors), session->limits->max_builder_allocated_bytes, 37,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (anchors == NULL)
		return 0;
	session->result->anchors = anchors;
	states = mant_structured_grow_array(session, session->anchor_states,
	    session->result->anchor_count, &session->anchor_state_capacity,
	    mant_structured_limit_u32(session->limits->max_anchor_evidence),
	    sizeof(*states), session->limits->max_builder_allocated_bytes, 37,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (states == NULL)
		return 0;
	session->anchor_states = states;
	anchor = anchors + session->result->anchor_count;
	memset(anchor, 0, sizeof(*anchor));
	anchor->target.ptr = mant_structured_copy_bytes(session,
	    (const uint8_t *)target, length, 1, MANT_STRUCTURED_STAGE_RENDER);
	if (anchor->target.ptr == NULL)
		return 0;
	anchor->target.len = length;
	anchor->key = ++session->result->anchor_count;
	anchor->origin = origin;
	anchor->provenance = provenance;
	state = states + anchor->key - 1;
	memset(state, 0, sizeof(*state));
	state->threshold = session->token_total;
	return anchor->key;
}

static int
bind_anchor(struct structured_session *session, uint32_t key, uint32_t owner,
    uint32_t root, int leading)
{
	struct mant_structured_anchor_view *anchor;
	struct structured_anchor_queue *queue;
	struct structured_anchor_state *state;

	if (key == 0 || key > session->result->anchor_count)
		return 0;
	anchor = session->result->anchors + key - 1;
	state = session->anchor_states + key - 1;
	if (anchor->owner != 0 && anchor->owner != owner)
		return 0;
	if (owner == 0)
		return queue_push(session, &session->unowned_anchors, key,
		    ANCHOR_QUEUE_UNOWNED);
	anchor->owner = owner;
	state->owner = owner;
	if (root != 0 && !leading) {
		state->root = root;
		queue = &session->root_atoms[root - 1].pending_anchors;
		if (queue->tail != 0 && session->anchor_states[
		    queue->tail - 1].threshold > state->threshold) {
			mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
			    MANT_STRUCTURED_STAGE_RENDER, 0, state->threshold,
			    session->anchor_states[queue->tail - 1].threshold);
			return 0;
		}
		return queue_push(session, queue, key, ANCHOR_QUEUE_ROOT);
	}
	state->root = 0;
	queue = owner_queue(session, owner);
	return queue != NULL && queue_push(session, queue, key,
	    ANCHOR_QUEUE_OWNER);
}

static void
resolve_queue_at(struct structured_session *session,
    struct structured_anchor_queue *queue, uint8_t queue_kind,
    uint32_t root, uint32_t atom_boundary)
{
	struct mant_structured_anchor_view *anchor;
	struct structured_anchor_state *state;
	uint32_t key, point;

	point = 0;
	while (queue->head != 0 && session->status == MANT_STRUCTURED_OK) {
		if (point == 0)
			point = append_point(session, root, atom_boundary);
		if (point == 0)
			return;
		key = queue_pop(session, queue, queue_kind);
		if (key == 0)
			return;
		anchor = session->result->anchors + key - 1;
		state = session->anchor_states + key - 1;
		anchor->point = point;
		state->root = root;
		state->resolved = 1;
	}
}

static void
drain_unowned_to_owner(struct structured_session *session, uint32_t owner,
    uint32_t root, int leading)
{
	uint32_t key;

	while ((key = queue_pop(session, &session->unowned_anchors,
	    ANCHOR_QUEUE_UNOWNED)) != 0)
		if (!bind_anchor(session, key, owner, root, leading))
			return;
}

static int
node_is_heading(const struct roff_node *node)
{
	return node != NULL && node->type == ROFFT_HEAD &&
	    (node->tok == MAN_SH || node->tok == MDOC_Sh);
}

void
mant_structured_address_enter_node(struct structured_session *session,
    const struct roff_node *node)
{
	struct structured_node_context *context;
	const char *target;
	uint32_t key, owner, origin, provenance, root, scope_list;
	int leading;

	if (node == NULL || session->status != MANT_STRUCTURED_OK)
		return;
	context = mant_structured_current_context(session);
	if (node->tok == MDOC_Tg) {
		target = node->child == NULL || node->child->type != ROFFT_TEXT ?
		    NULL : node->child->string;
		provenance = mant_structured_append_provenance(session, node, 1);
		scope_list = context != NULL && context->list != 0 &&
		    context->item == 0 ? context->list : 0;
		key = provenance == 0 ? 0 : append_anchor(session, target,
		    MANT_TARGET_ORIGIN_AUTHORED, provenance);
		if (key == 0)
			return;
		if (scope_list != 0) {
			queue_push(session,
			    &session->list_states[scope_list - 1].pending_anchors,
			    key, ANCHOR_QUEUE_LIST);
			return;
		}
		if ((node->flags & NODE_NOPRT) != 0 ||
		    (node->next != NULL && (node->next->tok == MAN_SH ||
		    node->next->tok == MDOC_Sh || node->next->tok == MDOC_Pp ||
		    node->next->tok == MDOC_It))) {
			queue_push(session, &session->unowned_anchors, key,
			    ANCHOR_QUEUE_UNOWNED);
			return;
		}
		owner = context != NULL && context->item != 0 ? context->owner :
		    session->current_owner != 0 ? session->current_owner :
		    session->section_owner;
		bind_anchor(session, key, owner, session->current_root,
		    session->current_root == 0);
		return;
	}
	if ((node->flags & NODE_ID) == 0)
		return;
	target = node_target(node);
	if (target == NULL)
		return;
	origin = mant_tag_is_manual(target) ? MANT_TARGET_ORIGIN_AUTHORED :
	    MANT_TARGET_ORIGIN_GENERATED;
	owner = context != NULL && context->item != 0 ? context->owner :
	    session->current_owner != 0 ? session->current_owner :
	    node_is_heading(node) ? 0 : session->section_owner;
	root = session->current_root;
	leading = root == 0 || node->tok == MDOC_Pp || node_is_heading(node) ||
	    (node->tok == MDOC_It && node->type != ROFFT_TEXT);
	key = origin == MANT_TARGET_ORIGIN_AUTHORED ?
	    take_authored_anchor(session, target, owner) : 0;
	if (key == 0) {
		provenance = mant_structured_append_provenance(session, node,
		    origin == MANT_TARGET_ORIGIN_AUTHORED);
		key = provenance == 0 ? 0 : append_anchor(session, target, origin,
		    provenance);
	}
	if (key == 0)
		return;
	bind_anchor(session, key, owner, root, leading);
	if ((node->tok == MDOC_Pp || node_is_heading(node)) && owner != 0)
		drain_unowned_to_owner(session, owner, 0, 1);
}

void
mant_structured_address_bind_item(struct structured_session *session,
    uint32_t list, uint32_t owner)
{
	struct structured_anchor_queue *queue;
	uint32_t key;

	if (list == 0 || list > session->result->list_count)
		return;
	queue = &session->list_states[list - 1].pending_anchors;
	while ((key = queue_pop(session, queue, ANCHOR_QUEUE_LIST)) != 0) {
		if (!bind_anchor(session, key, owner, 0, 1))
			return;
	}
}

static void
append_heading_evidence(struct structured_session *session,
    const struct roff_node *node)
{
	struct mant_structured_heading_evidence_view *headings, *heading;
	const struct roff_node *head;
	char *phrase;
	size_t length;
	uint32_t provenance;

	for (head = node; head != NULL; head = head->parent)
		if (node_is_heading(head))
			break;
	if (head == NULL)
		return;
	provenance = mant_structured_append_provenance(session, head, 1);
	if (provenance == 0)
		return;
	if (!mant_structured_charge(session, &session->builder_operations, 1,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_RENDER) ||
	    !mant_structured_charge(session, &session->relation_edges, 3,
	    session->limits->max_relation_edges, 30,
	    MANT_STRUCTURED_STAGE_RENDER))
		return;
	headings = mant_structured_grow_array(session,
	    session->result->heading_evidence,
	    session->result->heading_evidence_count,
	    &session->result->heading_evidence_capacity,
	    mant_structured_limit_u32(session->limits->max_heading_evidence),
	    sizeof(*headings), session->limits->max_builder_allocated_bytes, 38,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (headings == NULL)
		return;
	session->result->heading_evidence = headings;
	heading = headings + session->result->heading_evidence_count;
	memset(heading, 0, sizeof(*heading));
	heading->key = ++session->result->heading_evidence_count;
	heading->block = session->section_heading_block;
	heading->owner = session->section_owner;
	heading->provenance = provenance;
	phrase = NULL;
	deroff(&phrase, head);
	if (phrase == NULL || phrase[0] == '\0') {
		free(phrase);
		return;
	}
	length = strlen(phrase);
	if (mant_structured_valid_utf8((const uint8_t *)phrase, length)) {
		heading->authored_phrase.ptr = mant_structured_copy_bytes(session,
		    (const uint8_t *)phrase, length, 1,
		    MANT_STRUCTURED_STAGE_RENDER);
		if (heading->authored_phrase.ptr != NULL) {
			heading->authored_phrase.len = length;
			heading->authored_phrase_present = 1;
		}
	}
	free(phrase);
}

void
mant_structured_address_root_opened(struct structured_session *session,
    const struct roff_node *node, int heading)
{
	struct structured_anchor_queue *queue;
	uint32_t owner, root;

	root = session->current_root;
	owner = session->current_owner;
	if (heading) {
		append_heading_evidence(session, node);
		drain_unowned_to_owner(session, owner, 0, 1);
	}
	queue = owner_queue(session, owner);
	if (queue == NULL)
		return;
	if (queue->last_root != root &&
	    !mant_structured_charge(session, &session->builder_operations, 1,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_RENDER))
		return;
	queue->last_root = root;
	resolve_queue_at(session, queue, ANCHOR_QUEUE_OWNER, root, 0);
}

void
mant_structured_address_before_atom(struct structured_session *session,
    uint32_t root, uint64_t sequence)
{
	struct mant_structured_anchor_view *anchor;
	struct structured_anchor_queue *queue;
	struct structured_anchor_state *state;
	uint32_t key, point;

	if (root == 0 || root > session->result->content_root_count)
		return;
	queue = &session->root_atoms[root - 1].pending_anchors;
	point = 0;
	while (queue->head != 0 && session->anchor_states[
	    queue->head - 1].threshold < sequence) {
		if (point == 0)
			point = append_point(session, root,
			    session->root_atoms[root - 1].count);
		if (point == 0)
			return;
		key = queue_pop(session, queue, ANCHOR_QUEUE_ROOT);
		if (key == 0)
			return;
		anchor = session->result->anchors + key - 1;
		state = session->anchor_states + key - 1;
		anchor->point = point;
		state->resolved = 1;
		session->force_atom_split = 1;
	}
}

int
mant_structured_address_owner_needs_root(const struct structured_session *session,
    uint32_t owner)
{
	return owner != 0 && owner <= session->owner_anchor_queue_capacity &&
	    session->owner_anchor_queues[owner - 1].head != 0;
}

void
mant_structured_address_finish_owner(struct structured_session *session,
    uint32_t owner)
{
	struct structured_anchor_queue *queue;
	uint32_t root;

	if (owner == 0 || owner > session->owner_anchor_queue_capacity)
		return;
	queue = session->owner_anchor_queues + owner - 1;
	root = queue->last_root;
	if (root != 0)
		resolve_queue_at(session, queue, ANCHOR_QUEUE_OWNER, root,
		    session->root_atoms[root - 1].count);
}

void
mant_structured_address_finish(struct structured_session *session)
{
	struct structured_anchor_queue *queue;
	uint32_t i;

	for (i = 1; i <= session->result->owner_count; i++)
		mant_structured_address_finish_owner(session, i);
	for (i = 0; i < session->result->content_root_count; i++) {
		queue = &session->root_atoms[i].pending_anchors;
		resolve_queue_at(session, queue, ANCHOR_QUEUE_ROOT, i + 1,
		    session->root_atoms[i].count);
	}
	if (session->unowned_anchors.head != 0 &&
	    session->status == MANT_STRUCTURED_OK)
		mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
		    MANT_STRUCTURED_STAGE_FINALIZE, 0,
		    session->unowned_anchors.head, session->result->anchor_count);
	for (i = 0; i < session->result->list_count &&
	    session->status == MANT_STRUCTURED_OK; i++)
		if (session->list_states[i].pending_anchors.head != 0)
			mant_structured_set_failure(session,
			    MANT_STRUCTURED_RELATION, MANT_STRUCTURED_STAGE_FINALIZE,
			    0, session->list_states[i].pending_anchors.head,
			    session->result->anchor_count);
	for (i = 0; i < session->owner_anchor_queue_capacity &&
	    session->status == MANT_STRUCTURED_OK; i++)
		if (session->owner_anchor_queues[i].head != 0)
			mant_structured_set_failure(session,
			    MANT_STRUCTURED_RELATION, MANT_STRUCTURED_STAGE_FINALIZE,
			    0, session->owner_anchor_queues[i].head,
			    session->result->anchor_count);
	for (i = 0; i < session->result->content_root_count &&
	    session->status == MANT_STRUCTURED_OK; i++)
		if (session->root_atoms[i].pending_anchors.head != 0)
			mant_structured_set_failure(session,
			    MANT_STRUCTURED_RELATION, MANT_STRUCTURED_STAGE_FINALIZE,
			    0, session->root_atoms[i].pending_anchors.head,
			    session->result->anchor_count);
	for (i = 0; i < session->result->anchor_count &&
	    session->status == MANT_STRUCTURED_OK; i++)
		if (session->anchor_states[i].resolved == 0)
			mant_structured_set_failure(session,
			    MANT_STRUCTURED_RELATION, MANT_STRUCTURED_STAGE_FINALIZE,
			    0, i + 1, session->result->anchor_count);
}

void
mant_structured_address_release(struct structured_session *session)
{
	free(session->anchor_states);
	free(session->owner_anchor_queues);
	session->anchor_states = NULL;
	session->anchor_state_capacity = 0;
	session->owner_anchor_queues = NULL;
	session->owner_anchor_queue_capacity = 0;
}
