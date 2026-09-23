/* Structured link identity, target, and label-reference construction. */
#include "config.h"

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>

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

static uint32_t
link_identity_slot(const struct roff_node *node, uint32_t capacity)
{
	uint64_t hash = (uint64_t)(uintptr_t)node >> 3;

	hash ^= hash >> 33;
	hash *= UINT64_C(0xff51afd7ed558ccd);
	hash ^= hash >> 33;
	return (uint32_t)hash & (capacity - 1);
}

static uint32_t
find_link_identity(const struct structured_session *session,
    const struct roff_node *node)
{
	uint32_t slot;

	if (session->link_identity_capacity == 0)
		return 0;
	slot = link_identity_slot(node, session->link_identity_capacity);
	while (session->link_identities[slot].node != NULL) {
		if (session->link_identities[slot].node == node)
			return session->link_identities[slot].key;
		slot = (slot + 1) & (session->link_identity_capacity - 1);
	}
	return 0;
}

uint32_t
mant_structured_existing_link(const struct structured_session *session,
    const struct roff_node *node)
{
	const struct roff_node *canonical;

	canonical = mant_structured_link_node(node);
	if (canonical == NULL)
		return 0;
	return find_link_identity(session,
	    canonical->tok == MDOC_Mt ? node : canonical);
}

static int
insert_link_identity(struct structured_session *session,
    const struct roff_node *node, uint32_t key)
{
	struct structured_link_identity *entries, *old;
	uint32_t capacity, index, slot;

	if (session->link_identity_count * 2 >=
	    session->link_identity_capacity) {
		capacity = session->link_identity_capacity == 0 ? 16 :
		    session->link_identity_capacity * 2;
		if (capacity < session->link_identity_capacity ||
		    capacity > UINT32_MAX / sizeof(*entries)) {
			mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET,
			    MANT_STRUCTURED_STAGE_RENDER, 9, capacity,
			    UINT32_MAX / sizeof(*entries));
			return 0;
		}
		entries = mant_structured_allocate(session,
		    (uint64_t)capacity * sizeof(*entries), 1,
		    MANT_STRUCTURED_STAGE_RENDER);
		if (entries == NULL)
			return 0;
		old = session->link_identities;
		for (index = 0; index < session->link_identity_capacity; index++) {
			if (old[index].node == NULL)
				continue;
			slot = link_identity_slot(old[index].node, capacity);
			while (entries[slot].node != NULL)
				slot = (slot + 1) & (capacity - 1);
			entries[slot] = old[index];
		}
		session->link_identities = entries;
		session->link_identity_capacity = capacity;
		free(old);
	}
	slot = link_identity_slot(node, session->link_identity_capacity);
	while (session->link_identities[slot].node != NULL)
		slot = (slot + 1) & (session->link_identity_capacity - 1);
	session->link_identities[slot].node = node;
	session->link_identities[slot].key = key;
	session->link_identity_count++;
	return 1;
}

static size_t
target_scalar_bytes(int scalar, uint8_t *out)
{
	if (scalar < 0 || scalar > 0x10ffff ||
	    (scalar >= 0xd800 && scalar <= 0xdfff))
		return 0;
	if (scalar < 0x80) {
		if (out != NULL)
			out[0] = (uint8_t)scalar;
		return 1;
	}
	if (scalar < 0x800) {
		if (out != NULL) {
			out[0] = 0xc0 | (scalar >> 6);
			out[1] = 0x80 | (scalar & 0x3f);
		}
		return 2;
	}
	if (scalar < 0x10000) {
		if (out != NULL) {
			out[0] = 0xe0 | (scalar >> 12);
			out[1] = 0x80 | ((scalar >> 6) & 0x3f);
			out[2] = 0x80 | (scalar & 0x3f);
		}
		return 3;
	}
	if (out != NULL) {
		out[0] = 0xf0 | (scalar >> 18);
		out[1] = 0x80 | ((scalar >> 12) & 0x3f);
		out[2] = 0x80 | ((scalar >> 6) & 0x3f);
		out[3] = 0x80 | (scalar & 0x3f);
	}
	return 4;
}

static int
decode_link_target(const char *source, uint32_t profile, uint8_t *output,
    size_t *length)
{
	const char *cursor, *sequence;
	const char *device;
	size_t used = 0, count;
	int size, scalar, skip = 0;
	enum mandoc_esc escape;

	/* Match the scalar rules in pinned html.c::print_encode(): the authored
	 * operand remains provenance, while its typed destination is decoded. */
	for (cursor = source; *cursor != '\0';) {
		if (*cursor != '\\') {
			if (!skip) {
				if (used == SIZE_MAX)
					return 0;
				if (output != NULL)
					output[used] = (uint8_t)*cursor;
				used++;
			}
			skip = 0;
			cursor++;
			continue;
		}
		cursor++;
		escape = mandoc_escape(&cursor, &sequence, &size);
		/* Pinned html.c::print_encode() handles font changes, another
		 * SKIPCHAR, and malformed escapes before consuming a pending skip.
		 * In particular, \z\fBX skips X, not the font escape. */
		switch (escape) {
		case ESCAPE_FONT:
		case ESCAPE_FONTPREV:
		case ESCAPE_FONTBOLD:
		case ESCAPE_FONTITALIC:
		case ESCAPE_FONTBI:
		case ESCAPE_FONTROMAN:
		case ESCAPE_FONTCR:
		case ESCAPE_FONTCB:
		case ESCAPE_FONTCI:
		case ESCAPE_ERROR:
			continue;
		case ESCAPE_SKIPCHAR:
			skip = 1;
			continue;
		default:
			break;
		}
		if (skip) {
			skip = 0;
			continue;
		}
		scalar = -1;
		switch (escape) {
		case ESCAPE_UNICODE:
			scalar = mchars_num2uc(sequence + 1, size - 1);
			break;
		case ESCAPE_NUMBERED:
			scalar = mchars_num2char(sequence, size);
			break;
		case ESCAPE_SPECIAL:
			scalar = mchars_spec2cp(sequence, size);
			break;
		case ESCAPE_UNDEF:
			scalar = (unsigned char)*sequence;
			break;
		case ESCAPE_OVERSTRIKE:
			if (size != 0)
				scalar = (unsigned char)sequence[size - 1];
			break;
		case ESCAPE_DEVICE:
			/* term.c::term_word emits the selected terminal device, not
			 * html.c's separate `html` href spelling. */
			device = profile == MANT_PROFILE_ASCII ? "ascii" : "utf8";
			count = strlen(device);
			if (used > SIZE_MAX - count)
				return 0;
			if (output != NULL)
				memcpy(output + used, device, count);
			used += count;
			continue;
		case ESCAPE_IGNORE:
		case ESCAPE_UNSUPP:
		case ESCAPE_BREAK:
		case ESCAPE_NOSPACE:
		case ESCAPE_HORIZ:
		case ESCAPE_HLINE:
			continue;
		default:
			/* Recursive interpolation should already have been expanded
			 * by the pinned parser before rendering a typed destination. */
			return 0;
		}
		if (scalar < 0)
			continue;
		if ((scalar < 0x20 && scalar != '\t') ||
		    (scalar > 0x7e && scalar < 0xa0))
			scalar = 0xfffd;
		count = target_scalar_bytes(scalar,
		    output == NULL ? NULL : output + used);
		if (count == 0 || used > SIZE_MAX - count)
			return 0;
		used += count;
	}
	*length = used;
	return used != 0;
}

static int
copy_link_target(struct structured_session *session,
    struct mant_bytes_view *out, const struct roff_node *node)
{
	uint8_t *decoded;
	size_t length, written;

	if (node == NULL || node->type != ROFFT_TEXT || node->string == NULL ||
	    node->string[0] == '\0')
		return 0;
	if (strchr(node->string, '\\') == NULL) {
		length = strlen(node->string);
		if (!mant_structured_valid_utf8((const uint8_t *)node->string, length))
			return 0;
		out->ptr = mant_structured_copy_bytes(session,
		    (const uint8_t *)node->string, length, 1,
		    MANT_STRUCTURED_STAGE_RENDER);
		if (out->ptr == NULL)
			return 0;
		out->len = length;
		return 1;
	}
	if (!decode_link_target(node->string, session->result->profile,
	    NULL, &length))
		return 0;
	decoded = mant_structured_allocate(session, length, 0,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (decoded == NULL)
		return 0;
	if (!decode_link_target(node->string, session->result->profile,
	    decoded, &written) ||
	    written != length || !mant_structured_valid_utf8(decoded, length)) {
		free(decoded);
		return 0;
	}
	out->ptr = decoded;
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
	uint32_t existing, kind, provenance;

	canonical = mant_structured_link_node(node);
	if (canonical == NULL)
		return 0;
	/* mdoc_html.c::mdoc_mt_pre emits one mailto anchor per text child,
	 * whereas all other supported macros own one occurrence per macro. */
	identity = canonical->tok == MDOC_Mt ? node : canonical;
	existing = find_link_identity(session, identity);
	if (existing != 0)
		return existing;
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
	if (!insert_link_identity(session, identity, link->key))
		return 0;
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

int
mant_structured_finalize_links(struct structured_session *session)
{
	struct mant_structured_result *result = session->result;
	struct mant_structured_link_label_part_view *parts, *part;
	struct mant_structured_content_atom_view *atom;
	struct mant_structured_link_view *link;
	uint32_t *next = NULL;
	uint32_t index, total = 0, key, position;

	/* Nested macros produce outer/inner/outer execution order.  The atom
	 * owns its one link key during collection; only the final transfer view
	 * requires each occurrence's label parts to be a contiguous slice. */
	for (index = 0; index < result->content_atom_count; index++) {
		atom = result->content_atoms + index;
		if (atom->link == 0)
			continue;
		if (atom->link > result->link_count ||
		    (atom->kind != MANT_ATOM_TEXT &&
		    atom->kind != MANT_ATOM_WHITESPACE &&
		    atom->kind != MANT_ATOM_HARD_BREAK))
			goto invalid;
		if (total >= UINT32_MAX - 1 ||
		    total >= session->limits->max_link_label_parts ||
		    (atom->kind != MANT_ATOM_HARD_BREAK &&
		    atom->text.len > UINT32_MAX)) {
			mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET,
			    MANT_STRUCTURED_STAGE_RENDER, 39,
			    (uint64_t)total + 1,
			    session->limits->max_link_label_parts);
			return 0;
		}
		if (!mant_structured_charge(session, &session->relation_edges, 2,
		    session->limits->max_relation_edges, 30,
		    MANT_STRUCTURED_STAGE_RENDER))
			return 0;
		link = result->links + atom->link - 1;
		link->label_part_count++;
		total++;
	}
	for (index = 0, position = 1; index < result->link_count; index++) {
		link = result->links + index;
		link->first_label_part = position;
		position += link->label_part_count;
	}
	if (total == 0)
		return 1;
	parts = mant_structured_grow_array(session, result->link_label_parts,
	    total - 1, &result->link_label_part_capacity,
	    mant_structured_limit_u32(session->limits->max_link_label_parts),
	    sizeof(*parts), session->limits->max_builder_allocated_bytes, 39,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (parts == NULL)
		return 0;
	result->link_label_parts = parts;
	next = mant_structured_allocate(session,
	    (uint64_t)result->link_count * sizeof(*next), 0,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (next == NULL)
		return 0;
	for (index = 0; index < result->link_count; index++)
		next[index] = result->links[index].first_label_part - 1;
	for (index = 0; index < result->content_atom_count; index++) {
		atom = result->content_atoms + index;
		if (atom->link == 0)
			continue;
		key = atom->link - 1;
		part = parts + next[key]++;
		memset(part, 0, sizeof(*part));
		part->kind = atom->kind == MANT_ATOM_HARD_BREAK ?
		    MANT_LINK_LABEL_HARD_BREAK : MANT_LINK_LABEL_CONTENT;
		part->atom = atom->key;
		if (part->kind == MANT_LINK_LABEL_CONTENT)
			part->byte_end = (uint32_t)atom->text.len;
	}
	result->link_label_part_count = total;
	free(next);
	return 1;

invalid:
	mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
	    MANT_STRUCTURED_STAGE_RENDER, 0, 0, 0);
	return 0;
}
