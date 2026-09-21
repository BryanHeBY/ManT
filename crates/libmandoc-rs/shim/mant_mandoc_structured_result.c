/* Validation, borrowed views, and destruction for structured results. */
#include "mant_mandoc_structured_internal.h"

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
int
mant_structured_result_is_valid(const struct mant_structured_result *result)
{
	const struct mant_structured_metadata_view *metadata;
	const struct mant_structured_source_view *source;
	const struct mant_structured_span_view *span;
	const struct mant_structured_provenance_view *provenance;
	const struct mant_structured_owner_view *owner;
	const struct mant_structured_content_root_view *root;
	const struct mant_structured_content_atom_view *atom;
	const struct mant_structured_content_ref_view *content_ref;
	const struct mant_structured_link_view *link;
	const struct mant_structured_block_view *block;
	const struct mant_structured_diagnostic_view *diagnostic;
	struct mant_bytes_view metadata_strings[8];
	uint32_t metadata_flags[] = {
		MANT_METADATA_TITLE_PRESENT, MANT_METADATA_SECTION_PRESENT,
		MANT_METADATA_VOLUME_PRESENT, MANT_METADATA_OS_PRESENT,
		MANT_METADATA_ARCH_PRESENT, MANT_METADATA_NAME_PRESENT,
		MANT_METADATA_DATE_PRESENT, MANT_METADATA_ALIAS_PRESENT };
	uint32_t i, expected_ordinal, previous_root, previous_owner;
	uint32_t next_ref, previous_ref_atom;
	uint32_t top_level_ordinal, current_parent, child_ordinal;

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
	    (result->link_count != 0) != (result->links != NULL) ||
	    (result->block_count != 0) != (result->blocks != NULL) ||
	    (result->diagnostic_count != 0) != (result->diagnostics != NULL) ||
	    result->source_maps == NULL ||
	    result->source_map_count < result->source_count)
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
		    owner->kind != MANT_OWNER_SECTION) ||
		    owner->provenance == 0 ||
		    owner->provenance > result->provenance_count)
			return 0;
	}
	previous_owner = expected_ordinal = 0;
	for (i = 0; i < result->content_root_count; i++) {
		root = result->content_roots + i;
		if (root->owner != previous_owner) {
			if (root->owner < previous_owner)
				return 0;
			previous_owner = root->owner;
			expected_ordinal = 0;
		}
		if (root->key != i + 1 || root->owner == 0 ||
		    root->owner > result->owner_count ||
		    root->ordinal != expected_ordinal++ ||
		    (root->kind != MANT_ROOT_HEADING &&
		    root->kind != MANT_ROOT_BODY) || root->provenance == 0 ||
		    root->provenance > result->provenance_count ||
		    root->reserved != 0)
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
			    atom->link != 0 || atom->display_override_present != 0)
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
	next_ref = previous_ref_atom = 0;
	for (i = 0; i < result->link_count; i++) {
		uint32_t ref_index;

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
		    link->first_label_ref != next_ref + 1 ||
		    link->label_ref_count == 0 ||
		    link->label_ref_count > result->content_ref_count -
		    link->first_label_ref + 1 || link->provenance == 0 ||
		    link->provenance > result->provenance_count ||
		    link->reserved != 0)
			return 0;
		if ((link->target_kind == MANT_LINK_MANUAL) !=
		    (link->target_b_present != 0))
			return 0;
		for (ref_index = 0; ref_index < link->label_ref_count;
		    ref_index++) {
			content_ref = result->content_refs +
			    link->first_label_ref - 1 + ref_index;
			if (content_ref->atom <= previous_ref_atom ||
			    result->content_atoms[content_ref->atom - 1].link !=
			    link->key)
				return 0;
			previous_ref_atom = content_ref->atom;
		}
		next_ref += link->label_ref_count;
	}
	if (next_ref != result->content_ref_count)
		return 0;
	next_ref = 0;
	for (i = 0; i < result->content_atom_count; i++) {
		atom = result->content_atoms + i;
		if (atom->link == 0)
			continue;
		if (next_ref >= result->content_ref_count ||
		    result->content_refs[next_ref].atom != atom->key)
			return 0;
		next_ref++;
	}
	if (next_ref != result->content_ref_count)
		return 0;
	if (result->block_count != result->content_root_count)
		return 0;
	top_level_ordinal = current_parent = child_ordinal = 0;
	for (i = 0; i < result->block_count; i++) {
		block = result->blocks + i;
		root = result->content_roots + i;
		if (block->parent == 0) {
			if (block->ordinal != top_level_ordinal++)
				return 0;
			current_parent = block->key;
			child_ordinal = 0;
		} else if (block->parent != current_parent ||
		    block->ordinal != child_ordinal++)
			return 0;
		if (block->key != i + 1 || block->owner != root->owner ||
		    block->parent > result->block_count ||
		    block->parent == block->key ||
		    (block->parent != 0 &&
		    result->blocks[block->parent - 1].owner != block->owner) ||
		    block->provenance == 0 ||
		    block->provenance > result->provenance_count ||
		    block->root != root->key || block->table != 0 ||
		    block->fixed_view != 0 || block->reserved != 0 ||
		    (root->kind == MANT_ROOT_HEADING ?
		    block->kind != MANT_BLOCK_HEADING :
		    block->kind != MANT_BLOCK_PARAGRAPH))
			return 0;
	}
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
	if (!mant_structured_result_is_valid(result) || result->checked == 0) {
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
	if (!mant_structured_result_is_valid(result) || result->checked == 0)
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
	view->content_points = EMPTY_SLICE(struct mant_structured_content_point_view);
	view->links = SLICE(result->links, result->link_count);
	view->blocks = SLICE(result->blocks, result->block_count);
	view->tables = EMPTY_SLICE(struct mant_structured_table_view);
	view->table_rows = EMPTY_SLICE(struct mant_structured_table_row_view);
	view->table_cells = EMPTY_SLICE(struct mant_structured_table_cell_view);
	view->fixed_views = EMPTY_SLICE(struct mant_structured_fixed_view);
	view->fixed_lines = EMPTY_SLICE(struct mant_structured_fixed_line_view);
	view->placements = EMPTY_SLICE(struct mant_structured_placement_view);
	view->decorations = EMPTY_SLICE(struct mant_structured_decoration_view);
	view->forms = EMPTY_SLICE(struct mant_structured_form_view);
	view->name_hints = EMPTY_SLICE(struct mant_structured_name_hint_view);
	view->relations = EMPTY_SLICE(struct mant_structured_relation_view);
	view->diagnostics = SLICE(result->diagnostics, result->diagnostic_count);
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
	free(result->links);
	free(result->blocks);
	free(result->diagnostics);
	free(result->source_maps);
	result->magic = 0;
	free(result);
}
