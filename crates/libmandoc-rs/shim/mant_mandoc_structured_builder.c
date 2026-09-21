/* Final structured content and relation construction. */
#include "config.h"

#include <stdlib.h>
#include <stdio.h>
#include <string.h>

#include "mandoc.h"
#include "roff.h"
#include "tbl.h"
#include "out.h"
#include "term.h"

#include "mant_mandoc_structured_builder.h"
#include "mant_mandoc_structured_structure.h"

int
mant_structured_copy_metadata(struct structured_session *session,
    const struct roff_meta *meta)
{
	struct mant_structured_metadata_view *out = &session->result->metadata;

	if (!mant_structured_charge(session, &session->builder_operations, 9,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_FINALIZE))
		return 0;
	memset(out, 0, sizeof(*out));
	if (meta->macroset == MACROSET_MAN)
		out->macroset = MANT_FORMAT_MAN;
	else if (meta->macroset == MACROSET_MDOC)
		out->macroset = MANT_FORMAT_MDOC;
	if (meta->title != NULL)
		out->presence_flags |= MANT_METADATA_TITLE_PRESENT;
	if (meta->msec != NULL)
		out->presence_flags |= MANT_METADATA_SECTION_PRESENT;
	if (meta->vol != NULL)
		out->presence_flags |= MANT_METADATA_VOLUME_PRESENT;
	if (meta->os != NULL)
		out->presence_flags |= MANT_METADATA_OS_PRESENT;
	if (meta->arch != NULL)
		out->presence_flags |= MANT_METADATA_ARCH_PRESENT;
	if (meta->name != NULL)
		out->presence_flags |= MANT_METADATA_NAME_PRESENT;
	if (meta->date != NULL)
		out->presence_flags |= MANT_METADATA_DATE_PRESENT;
	if (meta->sodest != NULL)
		out->presence_flags |= MANT_METADATA_ALIAS_PRESENT;
	out->title = mant_structured_copy_cstring(session, meta->title);
	out->section = mant_structured_copy_cstring(session, meta->msec);
	out->volume = mant_structured_copy_cstring(session, meta->vol);
	out->operating_system = mant_structured_copy_cstring(session, meta->os);
	out->architecture = mant_structured_copy_cstring(session, meta->arch);
	out->name = mant_structured_copy_cstring(session, meta->name);
	out->date = mant_structured_copy_cstring(session, meta->date);
	out->alias_target = mant_structured_copy_cstring(session, meta->sodest);
	out->has_body = meta->hasbody != 0 ||
	    (meta->macroset == MACROSET_MDOC && meta->first != NULL &&
	    meta->first->child != NULL);
	return session->status == MANT_STRUCTURED_OK;
}

static const struct roff_node *
source_node(const struct roff_node *node)
{
	while (node != NULL && ((node->flags & NODE_NOSRC) != 0 ||
	    node->mant_source_key == 0 || node->line <= 0 || node->pos < 0))
		node = node->parent;
	return node;
}

static uint32_t
append_span_for_node(struct structured_session *session,
    const struct roff_node *node)
{
	struct mant_structured_span_view *span, *grown;

	node = source_node(node);
	if (node == NULL || node->mant_source_key > session->result->source_count)
		return 0;
	if (node == session->last_span_node && session->last_span != 0)
		return session->last_span;
	if (!mant_structured_charge(session, &session->builder_operations, 1,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_RENDER) ||
	    !mant_structured_charge(session, &session->relation_edges, 1,
	    session->limits->max_relation_edges, 30,
	    MANT_STRUCTURED_STAGE_RENDER))
		return 0;
	grown = mant_structured_grow_array(session, session->result->spans,
	    session->result->span_count, &session->result->span_capacity,
	    UINT32_MAX, sizeof(*grown),
	    session->limits->max_builder_allocated_bytes, 6,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (grown == NULL)
		return 0;
	session->result->spans = grown;
	span = grown + session->result->span_count;
	memset(span, 0, sizeof(*span));
	span->line_column_present = 1;
	span->source = node->mant_source_key;
	span->line_start = (uint32_t)node->line;
	span->column_start = (uint32_t)node->pos + 1;
	session->last_span_node = node;
	session->last_span = ++session->result->span_count;
	return session->last_span;
}

uint32_t
mant_structured_append_provenance(struct structured_session *session,
    const struct roff_node *node, int authored)
{
	struct mant_structured_provenance_view *provenance, *grown;
	uint32_t span;

	node = source_node(node);
	if (node == session->last_provenance_node &&
	    authored == session->last_provenance_authored &&
	    session->last_provenance != 0)
		return session->last_provenance;
	span = append_span_for_node(session, node);
	if (session->status != MANT_STRUCTURED_OK)
		return 0;
	grown = mant_structured_grow_array(session, session->result->provenances,
	    session->result->provenance_count,
	    &session->result->provenance_capacity, UINT32_MAX, sizeof(*grown),
	    session->limits->max_builder_allocated_bytes, 9,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (grown == NULL)
		return 0;
	session->result->provenances = grown;
	provenance = grown + session->result->provenance_count;
	memset(provenance, 0, sizeof(*provenance));
	if (authored && span != 0) {
		provenance->kind = MANT_PROVENANCE_AUTHORED;
		provenance->authored_span = span;
	} else if (span != 0) {
		provenance->kind = MANT_PROVENANCE_GENERATED;
		provenance->generated_trigger_span = span;
	} else
		provenance->kind = MANT_PROVENANCE_UNKNOWN;
	session->last_provenance_node = node;
	session->last_provenance_authored = authored != 0;
	session->last_provenance = ++session->result->provenance_count;
	return session->last_provenance;
}

uint32_t
mant_structured_limit_u32(uint64_t value)
{
	return value > UINT32_MAX ? UINT32_MAX : (uint32_t)value;
}

uint32_t
mant_structured_append_owner(struct structured_session *session, uint32_t kind,
    uint32_t provenance)
{
	struct mant_structured_owner_view *owners, *owner;
	uint32_t *counts;

	owners = mant_structured_grow_array(session, session->result->owners,
	    session->result->owner_count, &session->result->owner_capacity,
	    mant_structured_limit_u32(session->limits->max_owners), sizeof(*owners),
	    session->limits->max_builder_allocated_bytes, 11,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (owners == NULL)
		return 0;
	session->result->owners = owners;
	counts = mant_structured_grow_array(session, session->owner_root_counts,
	    session->result->owner_count, &session->owner_root_capacity,
	    mant_structured_limit_u32(session->limits->max_owners), sizeof(*counts),
	    session->limits->max_builder_allocated_bytes, 11,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (counts == NULL)
		return 0;
	session->owner_root_counts = counts;
	counts[session->result->owner_count] = 0;
	if (!mant_structured_charge(session, &session->builder_operations, 1,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_RENDER) ||
	    !mant_structured_charge(session, &session->relation_edges, 1,
	    session->limits->max_relation_edges, 30,
	    MANT_STRUCTURED_STAGE_RENDER))
		return 0;
	owner = owners + session->result->owner_count;
	memset(owner, 0, sizeof(*owner));
	owner->key = ++session->result->owner_count;
	owner->kind = kind;
	owner->provenance = provenance;
	return owner->key;
}

uint32_t
mant_structured_append_block(struct structured_session *session, uint32_t owner,
    uint32_t kind, uint32_t parent, uint32_t provenance, uint32_t root)
{
	struct mant_structured_block_view *blocks, *block;
	uint32_t *counts, ordinal;

	blocks = mant_structured_grow_array(session, session->result->blocks,
	    session->result->block_count, &session->result->block_capacity,
	    mant_structured_limit_u32(session->limits->max_blocks), sizeof(*blocks),
	    session->limits->max_builder_allocated_bytes, 12,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (blocks == NULL)
		return 0;
	session->result->blocks = blocks;
	counts = mant_structured_grow_array(session, session->block_child_counts,
	    session->result->block_count, &session->block_child_capacity,
	    mant_structured_limit_u32(session->limits->max_blocks), sizeof(*counts),
	    session->limits->max_builder_allocated_bytes, 12,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (counts == NULL)
		return 0;
	session->block_child_counts = counts;
	counts[session->result->block_count] = 0;
	if (parent == 0)
		ordinal = session->top_level_block_count++;
	else if (parent <= session->result->block_count)
		ordinal = counts[parent - 1]++;
	else {
		mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
		    MANT_STRUCTURED_STAGE_RENDER, 0, parent,
		    session->result->block_count);
		return 0;
	}
	if (!mant_structured_charge(session, &session->builder_operations, 1,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_RENDER) ||
	    !mant_structured_charge(session, &session->relation_edges,
	    2 + (parent != 0) + (root != 0),
	    session->limits->max_relation_edges, 30,
	    MANT_STRUCTURED_STAGE_RENDER))
		return 0;
	block = blocks + session->result->block_count;
	memset(block, 0, sizeof(*block));
	block->key = ++session->result->block_count;
	block->owner = owner;
	block->kind = kind;
	block->parent = parent;
	block->ordinal = ordinal;
	block->provenance = provenance;
	block->root = root;
	return block->key;
}

int
mant_structured_open_content_root(struct structured_session *session, int heading,
    uint32_t provenance)
{
	struct mant_structured_content_root_view *roots, *root;
	struct structured_root_atoms *root_atoms;
	struct structured_node_context *context;
	uint32_t owner_key, root_kind, block_kind, parent, block;

	context = mant_structured_current_context(session);
	if (heading) {
		owner_key = mant_structured_append_owner(session, MANT_OWNER_SECTION,
		    provenance);
		root_kind = MANT_ROOT_HEADING;
		block_kind = MANT_BLOCK_HEADING;
		parent = 0;
	} else if (context != NULL && context->item != 0) {
		owner_key = context->owner;
		root_kind = context->part == STRUCTURED_PART_TERM ?
		    MANT_ROOT_TERM : MANT_ROOT_BODY;
		block_kind = MANT_BLOCK_PARAGRAPH;
		parent = context->container_block;
	} else {
		if (session->section_owner == 0)
			session->section_owner = mant_structured_append_owner(session,
			    MANT_OWNER_DOCUMENT, provenance);
		owner_key = session->section_owner;
		root_kind = MANT_ROOT_BODY;
		block_kind = MANT_BLOCK_PARAGRAPH;
		parent = session->section_heading_block;
	}
	if (owner_key == 0)
		return 0;
	roots = mant_structured_grow_array(session, session->result->content_roots,
	    session->result->content_root_count,
	    &session->result->content_root_capacity,
	    mant_structured_limit_u32(session->limits->max_blocks), sizeof(*roots),
	    session->limits->max_builder_allocated_bytes, 12,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (roots == NULL)
		return 0;
	session->result->content_roots = roots;
	root_atoms = mant_structured_grow_array(session, session->root_atoms,
	    session->result->content_root_count, &session->root_atom_capacity,
	    mant_structured_limit_u32(session->limits->max_blocks),
	    sizeof(*root_atoms), session->limits->max_builder_allocated_bytes, 12,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (root_atoms == NULL)
		return 0;
	session->root_atoms = root_atoms;
	root_atoms += session->result->content_root_count;
	memset(root_atoms, 0, sizeof(*root_atoms));
	root_atoms->first = UINT32_MAX;
	if (!mant_structured_charge(session, &session->builder_operations, 1,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_RENDER) ||
	    !mant_structured_charge(session, &session->relation_edges, 2,
	    session->limits->max_relation_edges, 30,
	    MANT_STRUCTURED_STAGE_RENDER))
		return 0;
	root = roots + session->result->content_root_count;
	memset(root, 0, sizeof(*root));
	root->key = ++session->result->content_root_count;
	root->owner = owner_key;
	root->ordinal = session->owner_root_counts[owner_key - 1]++;
	root->kind = root_kind;
	root->provenance = provenance;
	block = 0;
	if (root_kind != MANT_ROOT_TERM) {
		block = mant_structured_append_block(session, owner_key, block_kind,
		    parent, provenance, root->key);
		if (block == 0)
			return 0;
	}
	session->current_owner = owner_key;
	session->current_root = root->key;
	session->current_root_atom_count = 0;
	if (heading) {
		session->section_owner = owner_key;
		session->section_heading_block = block;
	} else if (root_kind == MANT_ROOT_TERM && context != NULL) {
		uint32_t index;

		root_atoms->item = context->item;
		for (index = session->node_depth; index > 0; index--)
			if (session->node_contexts[index - 1].item == context->item &&
			    session->node_contexts[index - 1].part ==
			    STRUCTURED_PART_TERM) {
				session->node_contexts[index - 1].term_root = root->key;
			}
	}
	return 1;
}

static int
form_separator(const struct mant_structured_content_atom_view *atom)
{
	return atom->kind == MANT_ATOM_TEXT && atom->role == 0 &&
	    atom->text.len == 1 &&
	    (atom->text.ptr[0] == ',' || atom->text.ptr[0] == '|');
}

static int
append_name_hint(struct structured_session *session, uint32_t form_key,
    uint32_t first_ref, uint32_t ref_count, uint32_t provenance)
{
	struct mant_structured_name_hint_view *hints, *hint;

	hints = mant_structured_grow_array(session, session->result->name_hints,
	    session->result->name_hint_count,
	    &session->result->name_hint_capacity,
	    mant_structured_limit_u32(session->limits->max_name_hints),
	    sizeof(*hints), session->limits->max_builder_allocated_bytes, 25,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (hints == NULL)
		return 0;
	session->result->name_hints = hints;
	hint = hints + session->result->name_hint_count;
	memset(hint, 0, sizeof(*hint));
	hint->key = ++session->result->name_hint_count;
	hint->form = form_key;
	hint->first_ref = first_ref;
	hint->ref_count = ref_count;
	hint->provenance = provenance;
	return mant_structured_charge(session, &session->builder_operations, 1,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_RENDER) &&
	    mant_structured_charge(session, &session->relation_edges,
	    (uint64_t)ref_count + 2,
	    session->limits->max_relation_edges, 30,
	    MANT_STRUCTURED_STAGE_RENDER);
}

static int
append_term_form(struct structured_session *session, uint32_t root_key,
    uint32_t item_key, uint32_t begin, uint32_t end)
{
	struct mant_structured_content_ref_view *refs, *ref;
	struct mant_structured_form_view *forms, *form;
	struct mant_structured_item_view *item;
	const struct mant_structured_content_atom_view *atom;
	uint32_t first_ref, i, role, run_count, run_first, run_role;

	while (begin < end && session->result->content_atoms[begin].kind ==
	    MANT_ATOM_WHITESPACE)
		begin++;
	while (end > begin && session->result->content_atoms[end - 1].kind ==
	    MANT_ATOM_WHITESPACE)
		end--;
	first_ref = session->result->content_ref_count + 1;
	role = 0;
	for (i = begin; i < end; i++) {
		atom = session->result->content_atoms + i;
		if (atom->kind != MANT_ATOM_TEXT &&
		    atom->kind != MANT_ATOM_WHITESPACE)
			continue;
		if (atom->text.len == 0 || atom->text.len > UINT32_MAX)
			continue;
		refs = mant_structured_grow_array(session,
		    session->result->content_refs,
		    session->result->content_ref_count,
		    &session->result->content_ref_capacity,
		    mant_structured_limit_u32(session->limits->max_content_refs),
		    sizeof(*refs), session->limits->max_builder_allocated_bytes, 14,
		    MANT_STRUCTURED_STAGE_RENDER);
		if (refs == NULL)
			return 0;
		session->result->content_refs = refs;
		ref = refs + session->result->content_ref_count++;
		memset(ref, 0, sizeof(*ref));
		ref->atom = atom->key;
		ref->byte_end = (uint32_t)atom->text.len;
		if (atom->kind == MANT_ATOM_TEXT && atom->role != 0 &&
		    atom->role != MANT_ROLE_ARGUMENT) {
			if (role == 0)
				role = atom->role;
			else if (role != atom->role)
				role = UINT32_MAX;
		}
	}
	if (session->result->content_ref_count < first_ref)
		return 1;
	forms = mant_structured_grow_array(session, session->result->forms,
	    session->result->form_count, &session->result->form_capacity,
	    mant_structured_limit_u32(session->limits->max_forms), sizeof(*forms),
	    session->limits->max_builder_allocated_bytes, 24,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (forms == NULL)
		return 0;
	session->result->forms = forms;
	form = forms + session->result->form_count;
	memset(form, 0, sizeof(*form));
	form->key = ++session->result->form_count;
	form->owner = session->result->content_roots[root_key - 1].owner;
	form->role = role == UINT32_MAX ? 0 : role;
	form->first_ref = first_ref;
	form->ref_count = session->result->content_ref_count - first_ref + 1;
	form->provenance = session->result->content_roots[root_key - 1].provenance;
	item = session->result->items + item_key - 1;
	if (item->form_count == 0)
		item->first_form = form->key;
	item->form_count++;
	if (!mant_structured_charge(session, &session->builder_operations, 1,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_RENDER) ||
	    !mant_structured_charge(session, &session->relation_edges,
	    (uint64_t)form->ref_count + 3,
	    session->limits->max_relation_edges, 30,
	    MANT_STRUCTURED_STAGE_RENDER))
		return 0;
	run_first = run_count = run_role = 0;
	for (i = 0; i < form->ref_count; i++) {
		ref = session->result->content_refs + form->first_ref - 1 + i;
		atom = session->result->content_atoms + ref->atom - 1;
		if (atom->kind == MANT_ATOM_TEXT && atom->role != 0 &&
		    atom->role != MANT_ROLE_ARGUMENT) {
			if (run_count != 0 && run_role != atom->role) {
				if (!append_name_hint(session, form->key, run_first,
				    run_count, form->provenance))
					return 0;
				run_count = 0;
			}
			if (run_count == 0) {
				run_first = form->first_ref + i;
				run_role = atom->role;
			}
			run_count++;
		} else if (run_count != 0) {
			if (!append_name_hint(session, form->key, run_first,
			    run_count, form->provenance))
				return 0;
			run_count = 0;
		}
	}
	return run_count == 0 || append_name_hint(session, form->key,
	    run_first, run_count, form->provenance);
}

static int
finalize_term_root(struct structured_session *session,
    uint32_t root_key, uint32_t item_key)
{
	const struct mant_structured_content_atom_view *atom;
	uint32_t begin, end, i, remaining_content, segment_content;

	if (root_key == 0 || root_key > session->result->content_root_count ||
	    item_key == 0 || item_key > session->result->item_count)
		return 1;
	if (session->root_atoms[root_key - 1].count == 0)
		return 1;
	begin = session->root_atoms[root_key - 1].first;
	if (begin > session->result->content_atom_count ||
	    session->root_atoms[root_key - 1].count >
	    session->result->content_atom_count - begin) {
		mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
		    MANT_STRUCTURED_STAGE_RENDER, 0,
		    begin, session->result->content_atom_count);
		return 0;
	}
	end = begin + session->root_atoms[root_key - 1].count;
	remaining_content = 0;
	for (i = begin; i < end; i++) {
		atom = session->result->content_atoms + i;
		if (atom->root != root_key) {
			mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
			    MANT_STRUCTURED_STAGE_RENDER, 0, atom->root, root_key);
			return 0;
		}
		if (atom->kind != MANT_ATOM_WHITESPACE)
			remaining_content++;
	}
	segment_content = 0;
	for (i = begin; i < end; i++) {
		atom = session->result->content_atoms + i;
		if (atom->kind != MANT_ATOM_WHITESPACE)
			remaining_content--;
		if (!form_separator(atom) || segment_content == 0 ||
		    remaining_content == 0) {
			if (atom->kind != MANT_ATOM_WHITESPACE)
				segment_content++;
			continue;
		}
		if (!append_term_form(session, root_key, item_key, begin, i))
			return 0;
		begin = i + 1;
		segment_content = 0;
	}
	return append_term_form(session, root_key, item_key, begin, end);
}

int
mant_structured_close_term_root(struct structured_session *session,
    uint32_t root_key, uint32_t item_key)
{
	struct structured_root_atoms *state;
	const struct mant_structured_content_root_view *root;
	const struct mant_structured_item_view *item;

	if (root_key == 0 || root_key > session->result->content_root_count ||
	    item_key == 0 || item_key > session->result->item_count) {
		mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
		    MANT_STRUCTURED_STAGE_RENDER, 0, root_key, item_key);
		return 0;
	}
	state = session->root_atoms + root_key - 1;
	root = session->result->content_roots + root_key - 1;
	item = session->result->items + item_key - 1;
	if (root->kind != MANT_ROOT_TERM || state->item != item_key ||
	    root->owner != item->owner || state->closed != 0) {
		mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
		    MANT_STRUCTURED_STAGE_RENDER, 0, root_key, item_key);
		return 0;
	}
	state->closed = 1;
	return 1;
}

int
mant_structured_finish_term_roots(struct structured_session *session)
{
	const struct mant_structured_content_root_view *root;
	struct structured_root_atoms *state;
	uint32_t index, key;

	for (index = 0; index < session->result->content_root_count; index++) {
		key = index + 1;
		root = session->result->content_roots + index;
		state = session->root_atoms + index;
		if (root->kind != MANT_ROOT_TERM) {
			if (state->item != 0 || state->closed != 0 ||
			    state->finalized != 0) {
				mant_structured_set_failure(session,
				    MANT_STRUCTURED_RELATION,
				    MANT_STRUCTURED_STAGE_RENDER, 0, key, 0);
				return 0;
			}
			continue;
		}
		if (state->item == 0 || state->closed == 0 ||
		    state->finalized != 0 ||
		    !finalize_term_root(session, key, state->item)) {
			if (session->status == MANT_STRUCTURED_OK)
				mant_structured_set_failure(session,
				    MANT_STRUCTURED_RELATION,
				    MANT_STRUCTURED_STAGE_RENDER, 0, key,
				    state->item);
			return 0;
		}
		state->finalized = 1;
	}
	return 1;
}

int
mant_structured_append_atom(struct structured_session *session, uint32_t root,
    uint32_t provenance, uint32_t kind, uint32_t style, uint32_t role,
    uint32_t link,
    const uint8_t *bytes, size_t length, const uint8_t *display,
    size_t display_length, int breakable)
{
	struct mant_structured_content_atom_view *atoms, *atom;
	const struct mant_structured_content_root_view *content_root;
	uint8_t *grown_text;
	uint64_t required, new_capacity, added;

	if (root == 0 || root > session->result->content_root_count)
		return 0;
	if (!mant_structured_charge(session, &session->builder_operations, 1,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_RENDER))
		return 0;
	if ((length != 0 && !mant_structured_charge(session, &session->content_bytes, length,
	    session->limits->max_content_bytes, 10,
	    MANT_STRUCTURED_STAGE_RENDER)) ||
	    (display_length != 0 && !mant_structured_charge(session, &session->content_bytes,
	    display_length, session->limits->max_content_bytes, 10,
	    MANT_STRUCTURED_STAGE_RENDER)))
		return 0;
	if (length != 0 && session->result->content_atom_count != 0) {
		atom = session->result->content_atoms +
		    session->result->content_atom_count - 1;
		if (atom->root == root && atom->provenance == provenance &&
		    atom->kind == kind && atom->style_flags == style &&
		    atom->role == role && atom->link == link &&
		    atom->display_override_present == (display_length != 0) &&
		    atom->whitespace_breakable == (breakable != 0) &&
		    (kind == MANT_ATOM_TEXT || kind == MANT_ATOM_WHITESPACE)) {
			if (atom->text.len > UINT64_MAX - length) {
				mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET,
				    MANT_STRUCTURED_STAGE_RENDER, 10,
				    UINT64_MAX, session->limits->max_content_bytes);
				return 0;
			}
			required = atom->text.len + length;
			if (required > session->current_atom_capacity) {
				new_capacity = session->current_atom_capacity == 0 ? 8 :
				    session->current_atom_capacity;
				while (new_capacity < required) {
					if (new_capacity > UINT64_MAX / 2) {
						new_capacity = required;
						break;
					}
					new_capacity *= 2;
				}
				if (new_capacity > SIZE_MAX) {
					mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET,
					    MANT_STRUCTURED_STAGE_RENDER, 9,
					    new_capacity, SIZE_MAX);
					return 0;
				}
				added = new_capacity - session->current_atom_capacity;
				if (!mant_structured_charge(session, &session->allocated_bytes, added,
				    session->limits->max_builder_allocated_bytes, 9,
				    MANT_STRUCTURED_STAGE_RENDER))
					return 0;
				grown_text = mant_structured_injected_allocation_failure() ? NULL :
				    realloc((void *)atom->text.ptr, (size_t)new_capacity);
				if (grown_text == NULL) {
					mant_structured_set_failure(session,
					    MANT_STRUCTURED_BUILDER_ALLOC,
					    MANT_STRUCTURED_STAGE_RENDER, 0,
					    new_capacity,
					    session->limits->max_builder_allocated_bytes);
					return 0;
				}
				atom->text.ptr = grown_text;
				session->current_atom_capacity = new_capacity;
			}
			memcpy((uint8_t *)atom->text.ptr + atom->text.len,
			    bytes, length);
			atom->text.len = required;
			if (display_length != 0) {
				if (atom->display_override.len >
				    UINT64_MAX - display_length) {
					mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET,
					    MANT_STRUCTURED_STAGE_RENDER, 10,
					    UINT64_MAX,
					    session->limits->max_content_bytes);
					return 0;
				}
				required = atom->display_override.len + display_length;
				if (required > session->current_display_capacity) {
					new_capacity = session->current_display_capacity == 0 ?
					    8 : session->current_display_capacity;
					while (new_capacity < required) {
						if (new_capacity > UINT64_MAX / 2) {
							new_capacity = required;
							break;
						}
						new_capacity *= 2;
					}
					added = new_capacity -
					    session->current_display_capacity;
					if (new_capacity > SIZE_MAX) {
						mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET,
						    MANT_STRUCTURED_STAGE_RENDER, 9,
						    new_capacity, SIZE_MAX);
						return 0;
					}
					if (!mant_structured_charge(session, &session->allocated_bytes, added,
					    session->limits->max_builder_allocated_bytes, 9,
					    MANT_STRUCTURED_STAGE_RENDER))
						return 0;
					grown_text = mant_structured_injected_allocation_failure() ? NULL :
					    realloc((void *)atom->display_override.ptr,
					    (size_t)new_capacity);
					if (grown_text == NULL) {
						mant_structured_set_failure(session,
						    MANT_STRUCTURED_BUILDER_ALLOC,
						    MANT_STRUCTURED_STAGE_RENDER, 0,
						    new_capacity,
						    session->limits->max_builder_allocated_bytes);
						return 0;
					}
					atom->display_override.ptr = grown_text;
					session->current_display_capacity = new_capacity;
				}
				memcpy((uint8_t *)atom->display_override.ptr +
				    atom->display_override.len, display, display_length);
				atom->display_override.len = required;
			}
			return 1;
		}
	}
	if (!mant_structured_charge(session, &session->annotation_runs, 1,
	    session->limits->max_annotation_runs, 28,
	    MANT_STRUCTURED_STAGE_RENDER))
		return 0;
	if ((kind == MANT_ATOM_BREAK_OPPORTUNITY ||
	    kind == MANT_ATOM_HARD_BREAK) &&
	    !mant_structured_charge(session, &session->connection_atoms, 1,
	    session->limits->max_connection_atoms, 27,
	    MANT_STRUCTURED_STAGE_RENDER))
		return 0;
	if (!mant_structured_charge(session, &session->relation_edges, link == 0 ? 3 : 4,
	    session->limits->max_relation_edges, 30,
	    MANT_STRUCTURED_STAGE_RENDER))
		return 0;
	atoms = mant_structured_grow_array(session, session->result->content_atoms,
	    session->result->content_atom_count,
	    &session->result->content_atom_capacity,
	    mant_structured_limit_u32(session->limits->max_content_atoms), sizeof(*atoms),
	    session->limits->max_builder_allocated_bytes, 13,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (atoms == NULL)
		return 0;
	session->result->content_atoms = atoms;
	content_root = session->result->content_roots + root - 1;
	if (session->man_continuation_pending != 0 &&
	    session->last_man_item != 0 &&
	    content_root->kind == MANT_ROOT_BODY &&
	    content_root->owner == session->result->items[
	    session->last_man_item - 1].owner)
		session->man_continuation_pending = 0;
	atom = atoms + session->result->content_atom_count;
	memset(atom, 0, sizeof(*atom));
	atom->key = ++session->result->content_atom_count;
	atom->root = root;
	if (session->result->content_atom_count > 1 &&
	    atom[-1].root == root)
		atom->ordinal = atom[-1].ordinal + 1;
	atom->owner = content_root->owner;
	atom->kind = kind;
	atom->style_flags = style;
	atom->role = role;
	atom->link = link;
	atom->whitespace_breakable = breakable != 0;
	atom->provenance = provenance;
	if (length != 0) {
		session->current_atom_capacity = length < 8 ? 8 : length;
		atom->text.ptr = mant_structured_allocate(session, session->current_atom_capacity,
		    0, MANT_STRUCTURED_STAGE_RENDER);
		if (atom->text.ptr == NULL)
			return 0;
		memcpy((void *)atom->text.ptr, bytes, length);
		atom->text.len = length;
	} else
		session->current_atom_capacity = 0;
	if (display_length != 0) {
		session->current_display_capacity = display_length < 8 ? 8 :
		    display_length;
		atom->display_override.ptr = mant_structured_allocate(session,
		    session->current_display_capacity, 0,
		    MANT_STRUCTURED_STAGE_RENDER);
		if (atom->display_override.ptr == NULL)
			return 0;
		memcpy((void *)atom->display_override.ptr, display,
		    display_length);
		atom->display_override.len = display_length;
		atom->display_override_present = 1;
	} else
		session->current_display_capacity = 0;
	if (session->root_atoms[root - 1].count == 0)
		session->root_atoms[root - 1].first = atom->key - 1;
	session->root_atoms[root - 1].count++;
	session->current_root_atom_count++;
	return 1;
}
