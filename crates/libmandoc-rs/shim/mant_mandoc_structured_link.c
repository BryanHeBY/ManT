/* Structured link identity, target, and label-reference construction. */
#include "config.h"

#include <stdio.h>
#include <stdlib.h>
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
		case MDOC_Sx:
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

static int
copy_deroff_target(struct structured_session *session,
    struct mant_bytes_view *out, const struct roff_node *node)
{
	char *target;
	size_t length;
	int ok;

	target = NULL;
	deroff(&target, node);
	if (target == NULL || target[0] == '\0') {
		free(target);
		return 0;
	}
	length = strlen(target);
	ok = mant_structured_valid_utf8((const uint8_t *)target, length);
	if (ok) {
		out->ptr = mant_structured_copy_bytes(session,
		    (const uint8_t *)target, length, 1,
		    MANT_STRUCTURED_STAGE_RENDER);
		ok = out->ptr != NULL;
		if (ok)
			out->len = length;
	}
	free(target);
	return ok;
}

uint32_t
mant_structured_ensure_link(struct structured_session *session,
    const struct roff_node *node, uint32_t owner)
{
	const struct roff_node *canonical, *identity, *first, *second;
	struct mant_structured_link_view *links, *link = NULL;
	uint32_t kind, provenance;

	canonical = mant_structured_link_node(node);
	if (canonical == NULL)
		return 0;
	/* mdoc_html.c::mdoc_mt_pre emits one mailto anchor per text child,
	 * whereas all other supported macros own one occurrence per macro. */
	identity = canonical->tok == MDOC_Mt ? node : canonical;
	if (identity == session->last_link_node)
		return session->last_link;
	first = second = NULL;
	switch (canonical->tok) {
	case MDOC_Lk:
		kind = MANT_LINK_EXTERNAL;
		first = canonical->child;
		break;
	case MDOC_Mt:
		kind = MANT_LINK_EMAIL;
		if (node->parent != canonical || node->type != ROFFT_TEXT)
			goto unsupported;
		first = node;
		break;
	case MDOC_Xr:
		first = canonical->child;
		second = first == NULL ? NULL : first->next;
		/* mdoc_term.c::termp_xr_pre accepts the name alone. */
		kind = second == NULL ? MANT_LINK_DOCUMENT : MANT_LINK_MANUAL;
		break;
	case MDOC_Sx:
		kind = MANT_LINK_SECTION;
		first = canonical->child;
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
		first = canonical->child;
		second = first == NULL ? NULL : first->next;
		/* man_term.c::pre_MR prints name() without a section. */
		kind = second == NULL ? MANT_LINK_DOCUMENT : MANT_LINK_MANUAL;
		break;
	default:
		goto unsupported;
	}
	if (first == NULL)
		goto unsupported;
	/* The occurrence belongs to its destination operand.  Label
	 * atoms retain their independent provenance and must not replace it.
	 * mdoc_validate.c::post_defaults marks a synthesized .Mt ~ NODE_NOSRC. */
	provenance = mant_structured_append_provenance(session, first,
	    (first->flags & NODE_NOSRC) == 0);
	if (provenance == 0)
		return 0;
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
	if (!(canonical->tok == MDOC_Sx ?
	    copy_deroff_target(session, &link->target_a, canonical) :
	    copy_link_target(session, &link->target_a, first)))
		goto unsupported;
	if (kind == MANT_LINK_MANUAL) {
		link->target_b_present = 1;
		if (!copy_link_target(session, &link->target_b, second))
			goto unsupported;
	}
	session->result->link_count++;
	session->last_link_node = identity;
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
mant_structured_record_link_part(struct structured_session *session,
    uint32_t link_key)
{
	struct mant_structured_link_label_part_view *parts, *part;
	struct mant_structured_content_atom_view *atom;
	struct mant_structured_link_view *link;
	uint32_t part_kind;

	if (link_key == 0 || session->result->content_atom_count == 0)
		return;
	atom = session->result->content_atoms +
	    session->result->content_atom_count - 1;
	if (atom->kind == MANT_ATOM_TEXT || atom->kind == MANT_ATOM_WHITESPACE)
		part_kind = MANT_LINK_LABEL_CONTENT;
	else if (atom->kind == MANT_ATOM_HARD_BREAK)
		part_kind = MANT_LINK_LABEL_HARD_BREAK;
	else
		return;
	link = session->result->links + link_key - 1;
	if (link->label_part_count != 0) {
		part = session->result->link_label_parts +
		    link->first_label_part - 1 + link->label_part_count - 1;
		if (part->atom == atom->key) {
			if (part_kind == MANT_LINK_LABEL_CONTENT)
				part->byte_end = (uint32_t)atom->text.len;
			return;
		}
	}
	if (part_kind == MANT_LINK_LABEL_CONTENT && atom->text.len > UINT32_MAX) {
		mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET,
		    MANT_STRUCTURED_STAGE_RENDER, 39,
		    atom->text.len, UINT32_MAX);
		return;
	}
	if (!mant_structured_charge(session, &session->relation_edges, 2,
	    session->limits->max_relation_edges, 30,
	    MANT_STRUCTURED_STAGE_RENDER))
		return;
	parts = mant_structured_grow_array(session,
	    session->result->link_label_parts,
	    session->result->link_label_part_count,
	    &session->result->link_label_part_capacity,
	    mant_structured_limit_u32(session->limits->max_link_label_parts),
	    sizeof(*parts), session->limits->max_builder_allocated_bytes, 39,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (parts == NULL)
		return;
	session->result->link_label_parts = parts;
	part = parts + session->result->link_label_part_count;
	memset(part, 0, sizeof(*part));
	part->kind = part_kind;
	part->atom = atom->key;
	if (part_kind == MANT_LINK_LABEL_CONTENT)
		part->byte_end = (uint32_t)atom->text.len;
	if (link->label_part_count == 0)
		link->first_label_part =
		    session->result->link_label_part_count + 1;
	link->label_part_count++;
	session->result->link_label_part_count++;
}
