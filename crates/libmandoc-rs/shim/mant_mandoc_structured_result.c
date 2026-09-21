/* Validation, borrowed views, and destruction for structured results. */
#include "mant_mandoc_structured_internal.h"
#include "mant_mandoc_structured_session.h"

#include <stdlib.h>
#include <string.h>

static int
zero_bytes(const uint8_t *bytes, size_t length)
{
	size_t i;

	for (i = 0; i < length; i++)
		if (bytes[i] != 0)
			return 0;
	return 1;
}

static int
valid_string(struct mant_bytes_view view)
{
	return mant_structured_valid_bytes(view) && mant_structured_valid_utf8(view.ptr, view.len);
}

static int
utf8_boundary(struct mant_bytes_view view, uint32_t offset)
{
	if (offset > view.len)
		return 0;
	return offset == view.len || (view.ptr[offset] & 0xc0) != 0x80;
}

static uint64_t
utf8_scalar_count(struct mant_bytes_view view, uint64_t length)
{
	uint64_t count, index;

	count = 0;
	for (index = 0; index < length; index++)
		if ((view.ptr[index] & 0xc0) != 0x80)
			count++;
	return count;
}

static int
allocation_fits(uint32_t count, size_t item_size)
{
	return count == 0 || item_size <= SIZE_MAX / count;
}

static int
owner_has_item(const struct mant_structured_result *result, uint32_t owner)
{
	uint32_t begin, end, middle;

	begin = 0;
	end = result->item_count;
	while (begin < end) {
		middle = begin + (end - begin) / 2;
		if (result->items[middle].owner < owner)
			begin = middle + 1;
		else
			end = middle;
	}
	return begin < result->item_count &&
	    result->items[begin].owner == owner;
}

static int
prepare_validation_scratch(const struct mant_structured_result *result,
    struct structured_session *session)
{
	struct mant_structured_result *mutable_result;
	uint32_t root_slots, block_slots, root_offset_slots;

	if (result->validation_ready != 0)
		return result->validation_owner_count == result->owner_count &&
		    result->validation_content_root_count ==
		    result->content_root_count &&
		    result->validation_block_count == result->block_count &&
		    result->validation_list_count == result->list_count &&
		    result->validation_item_count == result->item_count &&
		    result->validation_content_atom_count ==
		    result->content_atom_count;
	if (session == NULL)
		return 0;
	root_slots = result->owner_count > result->content_root_count ?
	    result->owner_count : result->content_root_count;
	block_slots = result->block_count + 1;
	root_offset_slots = result->content_root_count + 1;
	if (block_slots == 0 || root_offset_slots == 0 ||
	    !allocation_fits(root_slots, sizeof(uint32_t)) ||
	    !allocation_fits(block_slots, sizeof(uint32_t)) ||
	    !allocation_fits(result->list_count, sizeof(uint32_t)) ||
	    !allocation_fits(result->content_atom_count, sizeof(uint8_t)) ||
	    !allocation_fits(root_offset_slots, sizeof(uint32_t)) ||
	    !allocation_fits(result->content_atom_count, sizeof(uint32_t)) ||
	    !allocation_fits(result->content_atom_count, sizeof(uint64_t)) ||
	    !allocation_fits(result->content_root_count, sizeof(uint64_t)))
		return 0;
	mutable_result = (struct mant_structured_result *)result;
	mutable_result->validation_root_slots = root_slots;
	mutable_result->validation_block_slots = block_slots;
	mutable_result->validation_list_slots = result->list_count;
	mutable_result->validation_atom_slots = result->content_atom_count;
	mutable_result->validation_roots = root_slots == 0 ? NULL :
	    mant_structured_allocate(session,
	    (uint64_t)root_slots * sizeof(uint32_t), 1,
	    MANT_STRUCTURED_STAGE_CHECK);
	mutable_result->validation_blocks = mant_structured_allocate(session,
	    (uint64_t)block_slots * sizeof(uint32_t), 1,
	    MANT_STRUCTURED_STAGE_CHECK);
	mutable_result->validation_lists = result->list_count == 0 ? NULL :
	    mant_structured_allocate(session,
	    (uint64_t)result->list_count * sizeof(uint32_t), 1,
	    MANT_STRUCTURED_STAGE_CHECK);
	mutable_result->validation_atoms = result->content_atom_count == 0 ?
	    NULL : mant_structured_allocate(session,
	    (uint64_t)result->content_atom_count * sizeof(uint8_t), 1,
	    MANT_STRUCTURED_STAGE_CHECK);
	mutable_result->validation_root_atom_offsets = mant_structured_allocate(
	    session, (uint64_t)root_offset_slots * sizeof(uint32_t), 1,
	    MANT_STRUCTURED_STAGE_CHECK);
	mutable_result->validation_root_atoms = result->content_atom_count == 0 ?
	    NULL : mant_structured_allocate(session,
	    (uint64_t)result->content_atom_count * sizeof(uint32_t), 1,
	    MANT_STRUCTURED_STAGE_CHECK);
	mutable_result->validation_atom_scalar_offsets =
	    result->content_atom_count == 0 ? NULL :
	    mant_structured_allocate(session,
	    (uint64_t)result->content_atom_count * sizeof(uint64_t), 1,
	    MANT_STRUCTURED_STAGE_CHECK);
	mutable_result->validation_root_scalar_totals =
	    result->content_root_count == 0 ? NULL :
	    mant_structured_allocate(session,
	    (uint64_t)result->content_root_count * sizeof(uint64_t), 1,
	    MANT_STRUCTURED_STAGE_CHECK);
	if ((root_slots != 0 && result->validation_roots == NULL) ||
	    result->validation_blocks == NULL ||
	    (result->list_count != 0 && result->validation_lists == NULL) ||
	    (result->content_atom_count != 0 &&
	    (result->validation_atoms == NULL ||
	    result->validation_root_atoms == NULL ||
	    result->validation_atom_scalar_offsets == NULL)) ||
	    result->validation_root_atom_offsets == NULL ||
	    (result->content_root_count != 0 &&
	    result->validation_root_scalar_totals == NULL))
		return 0;
	mutable_result->validation_owner_count = result->owner_count;
	mutable_result->validation_content_root_count =
	    result->content_root_count;
	mutable_result->validation_block_count = result->block_count;
	mutable_result->validation_list_count = result->list_count;
	mutable_result->validation_item_count = result->item_count;
	mutable_result->validation_content_atom_count =
	    result->content_atom_count;
	mutable_result->validation_ready = 1;
	return 1;
}

static int
valid_dense_ordinals(const struct mant_structured_result *result)
{
	uint32_t i;

	if (result->validation_root_slots != 0)
		memset(result->validation_roots, 0,
		    (size_t)result->validation_root_slots * sizeof(uint32_t));
	memset(result->validation_blocks, 0,
	    (size_t)result->validation_block_slots * sizeof(uint32_t));
	if (result->validation_list_slots != 0)
		memset(result->validation_lists, 0,
		    (size_t)result->validation_list_slots * sizeof(uint32_t));
	for (i = 0; i < result->content_root_count; i++) {
		const struct mant_structured_content_root_view *root =
		    result->content_roots + i;
		if (root->ordinal !=
		    result->validation_roots[root->owner - 1]++)
			return 0;
	}
	for (i = 0; i < result->block_count; i++) {
		const struct mant_structured_block_view *block = result->blocks + i;
		if (block->ordinal != result->validation_blocks[block->parent]++)
			return 0;
	}
	for (i = 0; i < result->item_count; i++) {
		const struct mant_structured_item_view *item = result->items + i;
		if (item->ordinal != result->validation_lists[item->list - 1]++)
			return 0;
	}
	return 1;
}

int
mant_structured_result_is_valid(const struct mant_structured_result *result,
    struct structured_session *session)
{
	const struct mant_structured_metadata_view *metadata;
	const struct mant_structured_source_view *source;
	const struct mant_structured_span_view *span;
	const struct mant_structured_provenance_view *provenance;
	const struct mant_structured_owner_view *owner;
	const struct mant_structured_content_root_view *root;
	const struct mant_structured_content_atom_view *atom;
	const struct mant_structured_content_ref_view *content_ref;
	const struct mant_structured_content_point_view *point;
	const struct mant_structured_link_view *link;
	const struct mant_structured_link_label_part_view *label_part;
	const struct mant_structured_anchor_view *anchor;
	const struct mant_structured_heading_evidence_view *heading;
	const struct mant_structured_block_view *block;
	const struct mant_structured_list_view *list;
	const struct mant_structured_item_view *item;
	const struct mant_structured_form_view *form;
	const struct mant_structured_name_hint_view *hint;
	const struct mant_structured_diagnostic_view *diagnostic;
	struct mant_bytes_view metadata_strings[8];
	uint32_t metadata_flags[] = {
		MANT_METADATA_TITLE_PRESENT, MANT_METADATA_SECTION_PRESENT,
		MANT_METADATA_VOLUME_PRESENT, MANT_METADATA_OS_PRESENT,
		MANT_METADATA_ARCH_PRESENT, MANT_METADATA_NAME_PRESENT,
		MANT_METADATA_DATE_PRESENT, MANT_METADATA_ALIAS_PRESENT };
	uint32_t i, expected_ordinal, previous_root, next_form;
	uint32_t previous_hint_form, previous_item_owner, list_index;
	uint64_t next_label_part;

	if (result == NULL || result->magic != MANT_STRUCTURED_MAGIC ||
	    result->root_source != 1 ||
	    (result->profile != MANT_PROFILE_UTF8 &&
	    result->profile != MANT_PROFILE_ASCII) || result->width == 0 ||
	    result->metadata.reserved != 0 ||
	    (result->source_count == 0) ||
	    (result->source_count != 0) != (result->sources != NULL) ||
	    (result->span_count != 0) != (result->spans != NULL) ||
	    (result->provenance_count != 0) !=
	    (result->provenances != NULL) ||
	    (result->owner_count != 0) != (result->owners != NULL) ||
	    (result->content_root_count != 0) !=
	    (result->content_roots != NULL) ||
	    (result->content_atom_count != 0) !=
	    (result->content_atoms != NULL) ||
	    (result->content_ref_count != 0) !=
	    (result->content_refs != NULL) ||
	    (result->content_point_count != 0) !=
	    (result->content_points != NULL) ||
	    (result->link_count != 0) != (result->links != NULL) ||
	    (result->link_label_part_count != 0) !=
	    (result->link_label_parts != NULL) ||
	    (result->anchor_count != 0) != (result->anchors != NULL) ||
	    (result->heading_evidence_count != 0) !=
	    (result->heading_evidence != NULL) ||
	    (result->block_count != 0) != (result->blocks != NULL) ||
	    (result->list_count != 0) != (result->lists != NULL) ||
	    (result->item_count != 0) != (result->items != NULL) ||
	    (result->form_count != 0) != (result->forms != NULL) ||
	    (result->name_hint_count != 0) != (result->name_hints != NULL) ||
	    (result->diagnostic_count != 0) != (result->diagnostics != NULL) ||
	    result->source_maps == NULL ||
	    result->source_map_count < result->source_count)
		return 0;
	if (!prepare_validation_scratch(result, session))
		return 0;
	metadata = &result->metadata;
	if ((metadata->macroset != MANT_FORMAT_MAN &&
	    metadata->macroset != MANT_FORMAT_MDOC) ||
	    (metadata->presence_flags & ~UINT32_C(0xff)) != 0 ||
	    metadata->has_body > 1 ||
	    !zero_bytes(metadata->reserved_bytes,
	    sizeof(metadata->reserved_bytes)))
		return 0;
	metadata_strings[0] = metadata->title;
	metadata_strings[1] = metadata->section;
	metadata_strings[2] = metadata->volume;
	metadata_strings[3] = metadata->operating_system;
	metadata_strings[4] = metadata->architecture;
	metadata_strings[5] = metadata->name;
	metadata_strings[6] = metadata->date;
	metadata_strings[7] = metadata->alias_target;
	for (i = 0; i < sizeof(metadata_flags) / sizeof(metadata_flags[0]); i++)
		if (!valid_string(metadata_strings[i]) ||
		    ((metadata->presence_flags & metadata_flags[i]) == 0 &&
		    (metadata_strings[i].ptr != NULL ||
		    metadata_strings[i].len != 0)))
			return 0;
	for (i = 0; i < result->source_count; i++) {
		source = result->sources + i;
		if (source->key != i + 1 || source->reserved != 0 ||
		    source->identity_kind < MANT_IDENTITY_PATH ||
		    source->identity_kind > MANT_IDENTITY_ANONYMOUS ||
		    (source->format != MANT_FORMAT_MAN &&
		    source->format != MANT_FORMAT_MDOC) ||
		    source->coordinate_kind != MANT_COORD_NATIVE_NORMALIZED_BYTES ||
		    !mant_structured_valid_identity_name(source->identity_kind,
		    source->logical_name) || source->hash_present > 1 ||
		    !zero_bytes(source->reserved_bytes,
		    sizeof(source->reserved_bytes)) ||
		    (source->hash_present == 0 &&
		    !zero_bytes(source->hash, sizeof(source->hash))))
			return 0;
	}
	for (i = 0; i < result->span_count; i++) {
		span = result->spans + i;
		if (span->reserved != 0 || span->line_column_present > 1 ||
		    span->byte_range_present > 1 ||
		    !zero_bytes(span->reserved_bytes,
		    sizeof(span->reserved_bytes)) || span->source == 0 ||
		    span->source > result->source_count)
			return 0;
		if (span->line_column_present == 0) {
			if (span->line_start != 0 || span->column_start != 0 ||
			    span->line_end != 0 || span->column_end != 0)
				return 0;
		} else if (span->line_start == 0 || span->column_start == 0 ||
		    ((span->line_end == 0) != (span->column_end == 0)) ||
		    (span->line_end != 0 && (span->line_end < span->line_start ||
		    (span->line_end == span->line_start &&
		    span->column_end < span->column_start))))
			return 0;
		if (span->line_column_present != 0 &&
		    (!mant_structured_source_position_in_maps(result->source_maps,
		    result->source_map_count, span->source, span->line_start,
		    span->column_start - 1) ||
		    (span->line_end != 0 && !mant_structured_source_position_in_maps(
		    result->source_maps, result->source_map_count, span->source,
		    span->line_end, span->column_end - 1))))
			return 0;
		if (span->byte_range_present != 0 || span->byte_start != 0 ||
		    span->byte_end != 0)
			return 0;
	}
	for (i = 0; i < result->provenance_count; i++) {
		provenance = result->provenances + i;
		if (provenance->reserved != 0)
			return 0;
		switch (provenance->kind) {
		case MANT_PROVENANCE_AUTHORED:
			if (provenance->authored_span == 0 ||
			    provenance->authored_span > result->span_count ||
			    provenance->generated_trigger_span != 0)
				return 0;
			break;
		case MANT_PROVENANCE_GENERATED:
			if (provenance->authored_span != 0 ||
			    provenance->generated_trigger_span > result->span_count)
				return 0;
			break;
		case MANT_PROVENANCE_UNKNOWN:
			if (provenance->authored_span != 0 ||
			    provenance->generated_trigger_span != 0)
				return 0;
			break;
		default:
			return 0;
		}
	}
	for (i = 0; i < result->owner_count; i++) {
		owner = result->owners + i;
		if (owner->key != i + 1 || owner->reserved != 0 ||
		    (owner->kind != MANT_OWNER_DOCUMENT &&
		    owner->kind != MANT_OWNER_SECTION &&
		    owner->kind != MANT_OWNER_LIST_ITEM &&
		    owner->kind != MANT_OWNER_DEFINITION_ITEM) ||
		    owner->provenance == 0 ||
		    owner->provenance > result->provenance_count)
			return 0;
	}
	for (i = 0; i < result->content_root_count; i++) {
		root = result->content_roots + i;
		if (root->key != i + 1 || root->owner == 0 ||
		    root->owner > result->owner_count ||
		    root->ordinal > i ||
		    (root->kind != MANT_ROOT_HEADING &&
		    root->kind != MANT_ROOT_TERM &&
		    root->kind != MANT_ROOT_BODY) || root->provenance == 0 ||
		    root->provenance > result->provenance_count ||
		    root->reserved != 0)
			return 0;
		if (root->kind == MANT_ROOT_TERM &&
		    result->owners[root->owner - 1].kind != MANT_OWNER_LIST_ITEM &&
		    result->owners[root->owner - 1].kind !=
		    MANT_OWNER_DEFINITION_ITEM)
			return 0;
	}
	previous_root = expected_ordinal = 0;
	for (i = 0; i < result->content_atom_count; i++) {
		atom = result->content_atoms + i;
		if (atom->root != previous_root) {
			previous_root = atom->root;
			expected_ordinal = 0;
		}
		if (atom->key != i + 1 || atom->root == 0 ||
		    atom->root > result->content_root_count ||
		    atom->ordinal != expected_ordinal++ || atom->owner == 0 ||
		    atom->owner != result->content_roots[atom->root - 1].owner ||
		    (atom->style_flags & ~(MANT_STYLE_BOLD | MANT_STYLE_ITALIC |
		    MANT_STYLE_LITERAL | MANT_STYLE_UNDERLINE)) != 0 ||
		    atom->role > MANT_ROLE_PATH || atom->link > result->link_count ||
		    atom->display_override_present > 1 ||
		    !zero_bytes(atom->display_reserved_bytes,
		    sizeof(atom->display_reserved_bytes)) ||
		    (atom->display_override_present == 0 ?
		    atom->display_override.ptr != NULL ||
		    atom->display_override.len != 0 :
		    !valid_string(atom->display_override) ||
		    atom->display_override.len == 0) ||
		    !zero_bytes(atom->reserved_bytes,
		    sizeof(atom->reserved_bytes)) || atom->provenance == 0 ||
		    atom->provenance > result->provenance_count ||
		    atom->reserved != 0)
			return 0;
		if (atom->kind == MANT_ATOM_TEXT) {
			if (!valid_string(atom->text) || atom->text.len == 0 ||
			    atom->whitespace_breakable != 0)
				return 0;
		} else if (atom->kind == MANT_ATOM_WHITESPACE) {
			if (!valid_string(atom->text) || atom->text.len == 0 ||
			    atom->whitespace_breakable > 1)
				return 0;
		} else if (atom->kind == MANT_ATOM_BREAK_OPPORTUNITY ||
		    atom->kind == MANT_ATOM_HARD_BREAK) {
			if (atom->text.ptr != NULL || atom->text.len != 0 ||
			    atom->whitespace_breakable != 0 ||
			    atom->style_flags != 0 || atom->role != 0 ||
			    (atom->kind == MANT_ATOM_BREAK_OPPORTUNITY &&
			    atom->link != 0) || atom->display_override_present != 0)
				return 0;
		} else
			return 0;
	}
	for (i = 0; i < result->content_ref_count; i++) {
		content_ref = result->content_refs + i;
		if (content_ref->reserved != 0 || content_ref->atom == 0 ||
		    content_ref->atom > result->content_atom_count ||
		    content_ref->byte_start >= content_ref->byte_end ||
		    content_ref->byte_end >
		    result->content_atoms[content_ref->atom - 1].text.len ||
		    !utf8_boundary(
		    result->content_atoms[content_ref->atom - 1].text,
		    content_ref->byte_start) ||
		    !utf8_boundary(
		    result->content_atoms[content_ref->atom - 1].text,
		    content_ref->byte_end))
			return 0;
	}
	if (result->validation_root_slots != 0)
		memset(result->validation_roots, 0,
		    (size_t)result->validation_root_slots * sizeof(uint32_t));
	for (i = 0; i < result->content_atom_count; i++)
		result->validation_roots[result->content_atoms[i].root - 1]++;
	if (result->content_root_count != 0)
		memset(result->validation_root_scalar_totals, 0,
		    (size_t)result->content_root_count * sizeof(uint64_t));
	result->validation_root_atom_offsets[0] = 0;
	for (i = 0; i < result->content_root_count; i++)
		result->validation_root_atom_offsets[i + 1] =
		    result->validation_root_atom_offsets[i] +
		    result->validation_roots[i];
	if (result->validation_root_slots != 0)
		memset(result->validation_roots, 0,
		    (size_t)result->validation_root_slots * sizeof(uint32_t));
	for (i = 0; i < result->content_atom_count; i++) {
		uint32_t root_index, slot;
		uint64_t scalar_count;

		atom = result->content_atoms + i;
		root_index = atom->root - 1;
		slot = result->validation_root_atom_offsets[root_index] +
		    result->validation_roots[root_index]++;
		result->validation_root_atoms[slot] = i + 1;
		result->validation_atom_scalar_offsets[i] =
		    result->validation_root_scalar_totals[root_index];
		scalar_count = atom->kind == MANT_ATOM_TEXT ||
		    atom->kind == MANT_ATOM_WHITESPACE ?
		    utf8_scalar_count(atom->text, atom->text.len) :
		    atom->kind == MANT_ATOM_HARD_BREAK ? 1 : 0;
		if (UINT64_MAX - result->validation_root_scalar_totals[root_index] <
		    scalar_count)
			return 0;
		result->validation_root_scalar_totals[root_index] += scalar_count;
	}
	if (result->validation_root_slots != 0)
		memset(result->validation_roots, 0,
		    (size_t)result->validation_root_slots * sizeof(uint32_t));
	for (i = 0; i < result->content_point_count; i++) {
		uint32_t root_index, root_atom_count;
		uint64_t expected_scalar;

		point = result->content_points + i;
		root_index = point->root == 0 ? UINT32_MAX : point->root - 1;
		if (point->key != i + 1 || point->root == 0 ||
		    point->root > result->content_root_count ||
		    point->owner != result->content_roots[point->root - 1].owner ||
		    point->ordinal != result->validation_roots[root_index]++ ||
		    point->provenance == 0 ||
		    point->provenance > result->provenance_count ||
		    point->reserved != 0)
			return 0;
		root_atom_count = result->validation_root_atom_offsets[root_index + 1] -
		    result->validation_root_atom_offsets[root_index];
		if (point->boundary_kind == MANT_POINT_BETWEEN_ATOMS) {
			if (point->atom != 0 || point->byte_offset != 0)
				return 0;
			if (point->atom_boundary > root_atom_count)
				return 0;
			if (point->atom_boundary == root_atom_count)
				expected_scalar =
				    result->validation_root_scalar_totals[root_index];
			else {
				uint32_t atom_key = result->validation_root_atoms[
				    result->validation_root_atom_offsets[root_index] +
				    point->atom_boundary];
				expected_scalar =
				    result->validation_atom_scalar_offsets[atom_key - 1];
			}
		} else if (point->boundary_kind == MANT_POINT_IN_ATOM) {
			if (point->atom_boundary != 0 || point->atom == 0 ||
			    point->atom > result->content_atom_count)
				return 0;
			atom = result->content_atoms + point->atom - 1;
			if (atom->root != point->root ||
			    (atom->kind != MANT_ATOM_TEXT &&
			    atom->kind != MANT_ATOM_WHITESPACE) ||
			    point->byte_offset > atom->text.len ||
			    !utf8_boundary(atom->text, point->byte_offset))
				return 0;
			expected_scalar =
			    result->validation_atom_scalar_offsets[point->atom - 1] +
			    utf8_scalar_count(atom->text, point->byte_offset);
		} else
			return 0;
		if (expected_scalar > UINT32_MAX ||
		    point->scalar_boundary != expected_scalar)
			return 0;
	}
	for (i = 0; i < result->anchor_count; i++) {
		anchor = result->anchors + i;
		if (anchor->key != i + 1 || anchor->owner == 0 ||
		    anchor->owner > result->owner_count || anchor->point == 0 ||
		    anchor->point > result->content_point_count ||
		    result->content_points[anchor->point - 1].owner != anchor->owner ||
		    (anchor->origin != MANT_TARGET_ORIGIN_GENERATED &&
		    anchor->origin != MANT_TARGET_ORIGIN_AUTHORED) ||
		    !valid_string(anchor->target) || anchor->target.len == 0 ||
		    anchor->provenance == 0 ||
		    anchor->provenance > result->provenance_count ||
		    anchor->reserved != 0)
			return 0;
		provenance = result->provenances + anchor->provenance - 1;
		if ((anchor->origin == MANT_TARGET_ORIGIN_GENERATED &&
		    provenance->kind != MANT_PROVENANCE_GENERATED) ||
		    (anchor->origin == MANT_TARGET_ORIGIN_AUTHORED &&
		    provenance->kind != MANT_PROVENANCE_AUTHORED))
			return 0;
	}
	if (result->validation_atom_slots != 0)
		memset(result->validation_atoms, 0,
		    (size_t)result->validation_atom_slots * sizeof(uint8_t));
	next_label_part = 1;
	for (i = 0; i < result->link_count; i++) {
		uint32_t part_index, previous_atom;

		link = result->links + i;
		if (link->key != i + 1 || link->owner == 0 ||
		    link->owner > result->owner_count ||
		    link->target_kind < MANT_LINK_EXTERNAL ||
		    link->target_kind > MANT_LINK_SECTION ||
		    !valid_string(link->target_a) || link->target_a.len == 0 ||
		    link->target_b_present > 1 ||
		    !zero_bytes(link->target_b_reserved_bytes,
		    sizeof(link->target_b_reserved_bytes)) ||
		    link->title_present > 1 ||
		    !zero_bytes(link->title_reserved_bytes,
		    sizeof(link->title_reserved_bytes)) ||
		    (link->target_b_present == 0 ?
		    link->target_b.ptr != NULL || link->target_b.len != 0 :
		    !valid_string(link->target_b) || link->target_b.len == 0) ||
		    (link->title_present == 0 ?
		    link->title.ptr != NULL || link->title.len != 0 :
		    !valid_string(link->title)) ||
		    link->first_label_ref != 0 || link->label_ref_count != 0 ||
		    link->first_label_part != next_label_part ||
		    link->label_part_count == 0 ||
		    link->first_label_part > result->link_label_part_count ||
		    link->label_part_count > result->link_label_part_count -
		    link->first_label_part + 1 || link->provenance == 0 ||
		    link->provenance > result->provenance_count ||
		    link->reserved != 0)
			return 0;
		if ((link->target_kind == MANT_LINK_MANUAL) !=
		    (link->target_b_present != 0))
			return 0;
		previous_atom = 0;
		for (part_index = 0; part_index < link->label_part_count;
		    part_index++) {
			label_part = result->link_label_parts +
			    link->first_label_part - 1 + part_index;
			if (label_part->reserved != 0 || label_part->atom == 0 ||
			    label_part->atom > result->content_atom_count)
				return 0;
			atom = result->content_atoms + label_part->atom - 1;
			if (atom->link != link->key || atom->owner != link->owner ||
			    label_part->atom <= previous_atom ||
			    result->validation_atoms[label_part->atom - 1] != 0)
				return 0;
			if (label_part->kind == MANT_LINK_LABEL_CONTENT) {
				if ((atom->kind != MANT_ATOM_TEXT &&
				    atom->kind != MANT_ATOM_WHITESPACE) ||
				    label_part->byte_start != 0 ||
				    label_part->byte_end != atom->text.len)
					return 0;
			} else if (label_part->kind ==
			    MANT_LINK_LABEL_HARD_BREAK) {
				if (atom->kind != MANT_ATOM_HARD_BREAK ||
				    label_part->byte_start != 0 ||
				    label_part->byte_end != 0)
					return 0;
			} else
				return 0;
			result->validation_atoms[label_part->atom - 1] = 1;
			previous_atom = label_part->atom;
		}
		next_label_part += link->label_part_count;
	}
	if (next_label_part != result->link_label_part_count + 1)
		return 0;
	for (i = 0; i < result->content_atom_count; i++) {
		atom = result->content_atoms + i;
		if ((atom->link != 0) != (result->validation_atoms[i] == 1))
			return 0;
	}
	for (i = 0; i < result->block_count; i++) {
		block = result->blocks + i;
		root = block->root == 0 ? NULL :
		    result->content_roots + block->root - 1;
		if (block->key != i + 1 || block->owner == 0 ||
		    block->owner > result->owner_count ||
		    block->parent > result->block_count ||
		    block->parent >= block->key || block->ordinal > i ||
		    block->provenance == 0 ||
		    block->provenance > result->provenance_count ||
		    block->table != 0 ||
		    block->fixed_view != 0 || block->reserved != 0 ||
		    (block->kind == MANT_BLOCK_HEADING ?
		    root == NULL || root->kind != MANT_ROOT_HEADING :
		    block->kind == MANT_BLOCK_PARAGRAPH ?
		    root == NULL || root->kind != MANT_ROOT_BODY :
		    block->kind == MANT_BLOCK_LIST ||
		    block->kind == MANT_BLOCK_DEFINITION_LIST ?
		    root != NULL : 1) ||
		    (root != NULL && root->owner != block->owner))
			return 0;
	}
	memset(result->validation_blocks, 0,
	    (size_t)result->validation_block_slots * sizeof(uint32_t));
	for (i = 0; i < result->heading_evidence_count; i++) {
		heading = result->heading_evidence + i;
		if (heading->key != i + 1 || heading->block == 0 ||
		    heading->block > result->block_count ||
		    result->blocks[heading->block - 1].kind != MANT_BLOCK_HEADING ||
		    heading->owner == 0 || heading->owner > result->owner_count ||
		    result->blocks[heading->block - 1].owner != heading->owner ||
		    heading->authored_phrase_present > 1 ||
		    !zero_bytes(heading->authored_phrase_reserved_bytes,
		    sizeof(heading->authored_phrase_reserved_bytes)) ||
		    (heading->authored_phrase_present == 0 ?
		    heading->authored_phrase.ptr != NULL ||
		    heading->authored_phrase.len != 0 :
		    !valid_string(heading->authored_phrase) ||
		    heading->authored_phrase.len == 0) ||
		    heading->provenance == 0 ||
		    heading->provenance > result->provenance_count ||
		    heading->reserved != 0)
			return 0;
		if (result->validation_blocks[heading->block]++ != 0)
			return 0;
	}
	for (i = 0; i < result->block_count; i++)
		if ((result->blocks[i].kind == MANT_BLOCK_HEADING) !=
		    (result->validation_blocks[i + 1] == 1))
			return 0;
	for (i = 0; i < result->list_count; i++) {
		list = result->lists + i;
		if (list->key != i + 1 || list->block == 0 ||
		    list->block > result->block_count ||
		    list->kind < MANT_LIST_BULLET ||
		    list->kind > MANT_LIST_NATIVE_MARKER || list->compact > 1 ||
		    (list->kind == MANT_LIST_ORDERED ? list->start == 0 :
		    list->start != 0) || list->provenance == 0 ||
		    list->provenance > result->provenance_count ||
		    list->reserved != 0 ||
		    (list->kind == MANT_LIST_DEFINITION ||
		    list->kind == MANT_LIST_NATIVE_MARKER ?
		    result->blocks[list->block - 1].kind !=
		    MANT_BLOCK_DEFINITION_LIST :
		    result->blocks[list->block - 1].kind != MANT_BLOCK_LIST))
			return 0;
	}
	list_index = 0;
	for (i = 0; i < result->block_count; i++) {
		block = result->blocks + i;
		if (block->kind != MANT_BLOCK_LIST &&
		    block->kind != MANT_BLOCK_DEFINITION_LIST)
			continue;
		if (list_index >= result->list_count ||
		    result->lists[list_index].block != block->key)
			return 0;
		list_index++;
	}
	if (list_index != result->list_count)
		return 0;
	next_form = 1;
	previous_item_owner = 0;
	for (i = 0; i < result->item_count; i++) {
		uint32_t form_index;
		uint32_t expected_owner_kind;

		item = result->items + i;
		if (item->list == 0 || item->list > result->list_count)
			return 0;
		expected_owner_kind = result->lists[item->list - 1].kind ==
		    MANT_LIST_DEFINITION || result->lists[item->list - 1].kind ==
		    MANT_LIST_NATIVE_MARKER ? MANT_OWNER_DEFINITION_ITEM :
		    MANT_OWNER_LIST_ITEM;
		if (item->key != i + 1 || item->owner == 0 ||
		    item->owner > result->owner_count || item->ordinal > i ||
		    item->owner <= previous_item_owner ||
		    result->owners[item->owner - 1].kind != expected_owner_kind ||
		    item->target_present != 0 ||
		    !zero_bytes(item->target_reserved_bytes,
		    sizeof(item->target_reserved_bytes)) ||
		    item->target.ptr != NULL || item->target.len != 0 ||
		    item->target_origin != MANT_TARGET_ORIGIN_ABSENT ||
		    item->provenance == 0 ||
		    item->provenance > result->provenance_count ||
		    item->reserved != 0 ||
		    (item->form_count == 0 ? item->first_form != 0 :
		    item->first_form != next_form ||
		    item->first_form > result->form_count ||
		    item->form_count > result->form_count - item->first_form + 1))
			return 0;
		for (form_index = 0; form_index < item->form_count; form_index++)
			if (result->forms[item->first_form - 1 + form_index].owner !=
			    item->owner)
				return 0;
		if (item->form_count != 0)
			next_form += item->form_count;
		previous_item_owner = item->owner;
	}
	if (next_form != result->form_count + 1)
		return 0;
	if (result->content_root_count != 0)
		memset(result->validation_roots, 0,
		    (size_t)result->content_root_count * sizeof(uint32_t));
	for (i = 0; i < result->form_count; i++) {
		uint32_t form_root, ref_index;

		form_root = 0;
		form = result->forms + i;
		if (form->key != i + 1 || form->owner == 0 ||
		    form->owner > result->owner_count || form->role > MANT_ROLE_PATH ||
		    form->first_ref == 0 || form->ref_count == 0 ||
		    form->first_ref > result->content_ref_count ||
		    form->ref_count > result->content_ref_count -
		    form->first_ref + 1 || form->provenance == 0 ||
		    form->provenance > result->provenance_count ||
		    form->reserved != 0)
			return 0;
		for (ref_index = 0; ref_index < form->ref_count; ref_index++) {
			content_ref = result->content_refs + form->first_ref - 1 +
			    ref_index;
			atom = result->content_atoms + content_ref->atom - 1;
			if (atom->owner != form->owner || result->content_roots[
			    atom->root - 1].kind != MANT_ROOT_TERM ||
			    (form_root != 0 && form_root != atom->root))
				return 0;
			form_root = atom->root;
		}
		if (form_root == 0)
			return 0;
		if ((result->validation_roots[form_root - 1] & 1) != 0)
			return 0;
		result->validation_roots[form_root - 1] = 1;
	}
	for (i = 0; i < result->content_atom_count; i++) {
		atom = result->content_atoms + i;
		if (atom->kind == MANT_ATOM_TEXT &&
		    result->content_roots[atom->root - 1].kind == MANT_ROOT_TERM)
			result->validation_roots[atom->root - 1] |= 2;
	}
	for (i = 0; i < result->content_root_count; i++) {
		root = result->content_roots + i;
		if (root->kind == MANT_ROOT_TERM) {
			if ((result->validation_roots[i] & 1) == 0 &&
			    ((result->validation_roots[i] & 2) != 0 ||
			    !owner_has_item(result, root->owner)))
				return 0;
		} else if (result->validation_roots[i] != 0)
			return 0;
	}
	previous_hint_form = 0;
	for (i = 0; i < result->name_hint_count; i++) {
		uint32_t relative_ref;

		hint = result->name_hints + i;
		if (hint->key != i + 1 || hint->form == 0 ||
		    hint->form > result->form_count || hint->first_ref == 0 ||
		    hint->ref_count == 0 || hint->provenance == 0 ||
		    hint->provenance > result->provenance_count ||
		    hint->reserved != 0 || hint->form < previous_hint_form ||
		    hint->first_ref <
		    result->forms[hint->form - 1].first_ref)
			return 0;
		relative_ref = hint->first_ref -
		    result->forms[hint->form - 1].first_ref;
		if (relative_ref >= result->forms[hint->form - 1].ref_count ||
		    hint->ref_count >
		    result->forms[hint->form - 1].ref_count - relative_ref)
			return 0;
		previous_hint_form = hint->form;
	}
	if (!valid_dense_ordinals(result))
		return 0;
	for (i = 0; i < result->diagnostic_count; i++) {
		diagnostic = result->diagnostics + i;
		if (diagnostic->reserved != 0 ||
		    diagnostic->level < MANT_DIAGNOSTIC_STYLE ||
		    diagnostic->level > MANT_DIAGNOSTIC_UNSUPPORTED ||
		    diagnostic->code < MANT_DIAGNOSTIC_CODE_NATIVE_FIRST ||
		    diagnostic->code > MANT_DIAGNOSTIC_CODE_NATIVE_LAST ||
		    !valid_string(diagnostic->message) ||
		    diagnostic->span > result->span_count ||
		    diagnostic->owner > result->owner_count)
			return 0;
	}
	if ((metadata->has_body == 0 && (result->owner_count != 0 ||
	    result->content_root_count != 0 || result->content_atom_count != 0 ||
	    result->block_count != 0)) ||
	    (metadata->has_body != 0 && (result->owner_count == 0 ||
	    result->content_root_count == 0 || result->block_count == 0)))
		return 0;
	return 1;
}

uint32_t
mant_structured_result_check(const struct mant_structured_result *result,
    struct mant_structured_failure_view *failure)
{
	mant_structured_clear_failure(failure);
	if (failure == NULL)
		return MANT_STRUCTURED_INVALID_INPUT;
	if (!mant_structured_result_is_valid(result, NULL) || result->checked == 0) {
		failure->status = MANT_STRUCTURED_RELATION;
		failure->stage = MANT_STRUCTURED_STAGE_CHECK;
		return failure->status;
	}
	return MANT_STRUCTURED_OK;
}

#define SLICE(what, amount) ((struct mant_slice_view){ \
	(what), (amount), (uint32_t)sizeof(*(what)) })
#define EMPTY_SLICE(type) ((struct mant_slice_view){ NULL, 0, sizeof(type) })

uint32_t
mant_structured_result_view(const struct mant_structured_result *result,
    struct mant_structured_result_view *view)
{
	if (view == NULL)
		return MANT_STRUCTURED_INVALID_INPUT;
	memset(view, 0, sizeof(*view));
	if (!mant_structured_result_is_valid(result, NULL) || result->checked == 0)
		return MANT_STRUCTURED_RELATION;
	view->root_source = result->root_source;
	view->profile = result->profile;
	view->width = result->width;
	view->metadata = result->metadata;
	view->sources = SLICE(result->sources, result->source_count);
	view->spans = SLICE(result->spans, result->span_count);
	view->provenances = SLICE(result->provenances,
	    result->provenance_count);
	view->owners = SLICE(result->owners, result->owner_count);
	view->content_roots = SLICE(result->content_roots,
	    result->content_root_count);
	view->content_atoms = SLICE(result->content_atoms,
	    result->content_atom_count);
	view->content_refs = SLICE(result->content_refs,
	    result->content_ref_count);
	view->content_points = SLICE(result->content_points,
	    result->content_point_count);
	view->links = SLICE(result->links, result->link_count);
	view->blocks = SLICE(result->blocks, result->block_count);
	view->lists = SLICE(result->lists, result->list_count);
	view->items = SLICE(result->items, result->item_count);
	view->tables = EMPTY_SLICE(struct mant_structured_table_view);
	view->table_rows = EMPTY_SLICE(struct mant_structured_table_row_view);
	view->table_cells = EMPTY_SLICE(struct mant_structured_table_cell_view);
	view->fixed_views = EMPTY_SLICE(struct mant_structured_fixed_view);
	view->fixed_lines = EMPTY_SLICE(struct mant_structured_fixed_line_view);
	view->placements = EMPTY_SLICE(struct mant_structured_placement_view);
	view->decorations = EMPTY_SLICE(struct mant_structured_decoration_view);
	view->forms = SLICE(result->forms, result->form_count);
	view->name_hints = SLICE(result->name_hints, result->name_hint_count);
	view->relations = EMPTY_SLICE(struct mant_structured_relation_view);
	view->diagnostics = SLICE(result->diagnostics, result->diagnostic_count);
	view->anchors = SLICE(result->anchors, result->anchor_count);
	view->heading_evidence = SLICE(result->heading_evidence,
	    result->heading_evidence_count);
	view->link_label_parts = SLICE(result->link_label_parts,
	    result->link_label_part_count);
	return MANT_STRUCTURED_OK;
}

void
mant_structured_free_bytes(struct mant_bytes_view view)
{
	free((void *)view.ptr);
}

void
mant_structured_result_free(struct mant_structured_result *result)
{
	uint32_t i;

	if (result == NULL)
		return;
	for (i = 0; i < result->source_count; i++)
		mant_structured_free_bytes(result->sources[i].logical_name);
	for (i = 0; i < result->diagnostic_count; i++)
		mant_structured_free_bytes(result->diagnostics[i].message);
	for (i = 0; i < result->content_atom_count; i++) {
		mant_structured_free_bytes(result->content_atoms[i].text);
		mant_structured_free_bytes(result->content_atoms[i].display_override);
	}
	for (i = 0; i < result->link_count; i++) {
		mant_structured_free_bytes(result->links[i].target_a);
		mant_structured_free_bytes(result->links[i].target_b);
		mant_structured_free_bytes(result->links[i].title);
	}
	for (i = 0; i < result->anchor_count; i++)
		mant_structured_free_bytes(result->anchors[i].target);
	for (i = 0; i < result->heading_evidence_count; i++)
		mant_structured_free_bytes(
		    result->heading_evidence[i].authored_phrase);
	for (i = 0; i < result->item_count; i++)
		mant_structured_free_bytes(result->items[i].target);
	for (i = 0; i < result->source_map_count; i++)
		free(result->source_maps[i].lines);
	mant_structured_free_bytes(result->metadata.title);
	mant_structured_free_bytes(result->metadata.section);
	mant_structured_free_bytes(result->metadata.volume);
	mant_structured_free_bytes(result->metadata.operating_system);
	mant_structured_free_bytes(result->metadata.architecture);
	mant_structured_free_bytes(result->metadata.name);
	mant_structured_free_bytes(result->metadata.date);
	mant_structured_free_bytes(result->metadata.alias_target);
	free(result->sources);
	free(result->spans);
	free(result->provenances);
	free(result->owners);
	free(result->content_roots);
	free(result->content_atoms);
	free(result->content_refs);
	free(result->content_points);
	free(result->links);
	free(result->link_label_parts);
	free(result->anchors);
	free(result->heading_evidence);
	free(result->blocks);
	free(result->lists);
	free(result->items);
	free(result->forms);
	free(result->name_hints);
	free(result->diagnostics);
	free(result->source_maps);
	free(result->validation_roots);
	free(result->validation_blocks);
	free(result->validation_lists);
	free(result->validation_atoms);
	free(result->validation_root_atom_offsets);
	free(result->validation_root_atoms);
	free(result->validation_atom_scalar_offsets);
	free(result->validation_root_scalar_totals);
	result->magic = 0;
	free(result);
}
