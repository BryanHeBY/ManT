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

int
mant_structured_open_content_root(struct structured_session *session, int heading,
    uint32_t provenance)
{
	struct mant_structured_owner_view *owners, *owner = NULL;
	struct mant_structured_content_root_view *roots, *root;
	struct mant_structured_block_view *blocks, *block;
	uint32_t owner_key, operations;
	int new_owner;

	new_owner = heading || session->section_owner == 0;
	if (new_owner) {
		owners = mant_structured_grow_array(session, session->result->owners,
		    session->result->owner_count,
		    &session->result->owner_capacity,
		    mant_structured_limit_u32(session->limits->max_owners), sizeof(*owners),
		    session->limits->max_builder_allocated_bytes, 11,
		    MANT_STRUCTURED_STAGE_RENDER);
		if (owners == NULL)
			return 0;
		session->result->owners = owners;
		owner = owners + session->result->owner_count;
		memset(owner, 0, sizeof(*owner));
		owner->key = ++session->result->owner_count;
		owner->kind = heading ? MANT_OWNER_SECTION : MANT_OWNER_DOCUMENT;
		owner->provenance = provenance;
		owner_key = owner->key;
	} else
		owner_key = session->section_owner;
	roots = mant_structured_grow_array(session, session->result->content_roots,
	    session->result->content_root_count,
	    &session->result->content_root_capacity,
	    mant_structured_limit_u32(session->limits->max_blocks), sizeof(*roots),
	    session->limits->max_builder_allocated_bytes, 12,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (roots == NULL)
		return 0;
	session->result->content_roots = roots;
	blocks = mant_structured_grow_array(session, session->result->blocks,
	    session->result->block_count, &session->result->block_capacity,
	    mant_structured_limit_u32(session->limits->max_blocks), sizeof(*blocks),
	    session->limits->max_builder_allocated_bytes, 12,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (blocks == NULL)
		return 0;
	session->result->blocks = blocks;
	operations = new_owner ? 3 : 2;
	if (!mant_structured_charge(session, &session->builder_operations, operations,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_RENDER) ||
	    !mant_structured_charge(session, &session->relation_edges, 6,
	    session->limits->max_relation_edges, 30,
	    MANT_STRUCTURED_STAGE_RENDER))
		return 0;
	root = roots + session->result->content_root_count;
	memset(root, 0, sizeof(*root));
	root->key = ++session->result->content_root_count;
	root->owner = owner_key;
	root->ordinal = heading ? 0 :
	    session->section_owner != 0 ? session->section_root_count : 0;
	root->kind = heading ? MANT_ROOT_HEADING : MANT_ROOT_BODY;
	root->provenance = provenance;
	block = blocks + session->result->block_count;
	memset(block, 0, sizeof(*block));
	block->key = ++session->result->block_count;
	block->owner = owner_key;
	block->kind = heading ? MANT_BLOCK_HEADING : MANT_BLOCK_PARAGRAPH;
	block->parent = heading ? 0 : session->section_heading_block;
	block->ordinal = block->parent == 0 ?
	    session->top_level_block_count : session->section_child_block_count;
	block->provenance = provenance;
	block->root = root->key;
	session->current_owner = owner_key;
	session->current_root = root->key;
	session->current_root_atom_count = 0;
	if (heading) {
		session->top_level_block_count++;
		session->section_owner = owner_key;
		session->section_heading_block = block->key;
		session->section_root_count = 1;
		session->section_child_block_count = 0;
	} else if (session->section_owner != 0) {
		session->section_root_count++;
		session->section_child_block_count++;
	} else
		session->top_level_block_count++;
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
	return 1;
}

const struct roff_node *
mant_structured_link_node(const struct roff_node *node)
{
	for (; node != NULL; node = node->parent)
		switch (node->tok) {
		case MDOC_Lk:
		case MDOC_Mt:
		case MDOC_Xr:
		case MAN_MR:
			if (node->type == ROFFT_ELEM)
				return node;
			break;
		case MAN_UR:
		case MAN_MT:
			if (node->type == ROFFT_BLOCK)
				return node;
			break;
		default:
			break;
		}
	return NULL;
}

static int
copy_link_target(struct structured_session *session,
    struct mant_bytes_view *out, const struct roff_node *node)
{
	size_t length;

	if (node == NULL || node->type != ROFFT_TEXT || node->string == NULL ||
	    node->string[0] == '\0' || strchr(node->string, '\\') != NULL)
		return 0;
	length = strlen(node->string);
	if (!mant_structured_valid_utf8((const uint8_t *)node->string, length))
		return 0;
	out->ptr = mant_structured_copy_bytes(session, (const uint8_t *)node->string,
	    length, 1, MANT_STRUCTURED_STAGE_RENDER);
	if (out->ptr == NULL)
		return 0;
	out->len = length;
	return 1;
}

uint32_t
mant_structured_ensure_link(struct structured_session *session, const struct roff_node *node,
    uint32_t owner, uint32_t provenance)
{
	const struct roff_node *canonical, *first, *second;
	struct mant_structured_link_view *links, *link = NULL;
	uint32_t kind;

	canonical = mant_structured_link_node(node);
	if (canonical == NULL)
		return 0;
	if (canonical == session->last_link_node)
		return session->last_link;
	first = second = NULL;
	switch (canonical->tok) {
	case MDOC_Lk:
		kind = MANT_LINK_EXTERNAL;
		first = canonical->child;
		break;
	case MDOC_Mt:
		kind = MANT_LINK_EMAIL;
		first = canonical->child;
		if (first != NULL && first->next != NULL &&
		    (first->next->flags & NODE_DELIMC) == 0)
			goto unsupported;
		break;
	case MDOC_Xr:
		kind = MANT_LINK_MANUAL;
		first = canonical->child;
		second = first == NULL ? NULL : first->next;
		break;
	case MAN_UR:
		kind = MANT_LINK_EXTERNAL;
		first = canonical->head == NULL ? NULL : canonical->head->child;
		break;
	case MAN_MT:
		kind = MANT_LINK_EMAIL;
		first = canonical->head == NULL ? NULL : canonical->head->child;
		break;
	case MAN_MR:
		kind = MANT_LINK_MANUAL;
		first = canonical->child;
		second = first == NULL ? NULL : first->next;
		break;
	default:
		goto unsupported;
	}
	if (first == NULL || (kind == MANT_LINK_MANUAL && second == NULL))
		goto unsupported;
	if (!mant_structured_charge(session, &session->relation_edges, 2,
	    session->limits->max_relation_edges, 30,
	    MANT_STRUCTURED_STAGE_RENDER))
		return 0;
	links = mant_structured_grow_array(session, session->result->links,
	    session->result->link_count, &session->result->link_capacity,
	    mant_structured_limit_u32(session->limits->max_links), sizeof(*links),
	    session->limits->max_builder_allocated_bytes, 16,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (links == NULL)
		return 0;
	session->result->links = links;
	link = links + session->result->link_count;
	memset(link, 0, sizeof(*link));
	link->key = session->result->link_count + 1;
	link->owner = owner;
	link->target_kind = kind;
	link->provenance = provenance;
	if (!copy_link_target(session, &link->target_a, first))
		goto unsupported;
	if (kind == MANT_LINK_MANUAL) {
		link->target_b_present = 1;
		if (!copy_link_target(session, &link->target_b, second))
			goto unsupported;
	}
	session->result->link_count++;
	session->last_link_node = canonical;
	session->last_link = link->key;
	return link->key;

unsupported:
	if (link != NULL) {
		mant_structured_free_bytes(link->target_a);
		mant_structured_free_bytes(link->target_b);
		memset(link, 0, sizeof(*link));
	}
	if (session->probe == NULL)
		mant_structured_set_failure(session, MANT_STRUCTURED_UNSUPPORTED,
		    MANT_STRUCTURED_STAGE_RENDER, 0, 0, 0);
	return 0;
}

void
mant_structured_record_link_ref(struct structured_session *session, uint32_t link_key)
{
	struct mant_structured_content_ref_view *refs, *ref;
	struct mant_structured_content_atom_view *atom;
	struct mant_structured_link_view *link;

	if (link_key == 0 || session->result->content_atom_count == 0)
		return;
	atom = session->result->content_atoms +
	    session->result->content_atom_count - 1;
	link = session->result->links + link_key - 1;
	if (link->label_ref_count != 0) {
		ref = session->result->content_refs + link->first_label_ref - 1 +
		    link->label_ref_count - 1;
		if (ref->atom == atom->key) {
			ref->byte_end = (uint32_t)atom->text.len;
			return;
		}
	}
	if (atom->text.len > UINT32_MAX) {
		mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET,
		    MANT_STRUCTURED_STAGE_RENDER, 14,
		    atom->text.len, UINT32_MAX);
		return;
	}
	if (!mant_structured_charge(session, &session->relation_edges, 2,
	    session->limits->max_relation_edges, 30,
	    MANT_STRUCTURED_STAGE_RENDER))
		return;
	refs = mant_structured_grow_array(session, session->result->content_refs,
	    session->result->content_ref_count,
	    &session->result->content_ref_capacity,
	    mant_structured_limit_u32(session->limits->max_content_refs), sizeof(*refs),
	    session->limits->max_builder_allocated_bytes, 14,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (refs == NULL)
		return;
	session->result->content_refs = refs;
	ref = refs + session->result->content_ref_count;
	memset(ref, 0, sizeof(*ref));
	ref->atom = atom->key;
	ref->byte_end = (uint32_t)atom->text.len;
	if (link->label_ref_count == 0)
		link->first_label_ref = session->result->content_ref_count + 1;
	link->label_ref_count++;
	session->result->content_ref_count++;
}
