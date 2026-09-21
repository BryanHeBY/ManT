/* Structured link identity, target, and label-reference construction. */
#include "config.h"

#include <stdio.h>
#include <string.h>

#include "mandoc.h"
#include "roff.h"

#include "mant_mandoc_structured_builder.h"
#include "mant_mandoc_structured_link.h"

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
mant_structured_ensure_link(struct structured_session *session,
    const struct roff_node *node, uint32_t owner, uint32_t provenance)
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
mant_structured_record_link_ref(struct structured_session *session,
    uint32_t link_key)
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
