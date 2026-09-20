/* Bounded session and immutable result owner for structured rendering. */
#include "config.h"
#include "mant_thread_local.h"

#include <errno.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "mandoc.h"
#include "roff.h"
#include "mandoc_parse.h"

#include "mant_mandoc_structured.h"

#define MANT_STRUCTURED_MAGIC 0x4d535231U

_Static_assert(MANT_DIAGNOSTIC_CODE_NATIVE_LAST == MANDOCERR_MAX,
    "structured diagnostic code range must match pinned mandocerr");

struct mant_structured_result {
	uint32_t magic;
	uint32_t checked;
	uint32_t root_source;
	uint32_t profile;
	uint32_t width;
	struct mant_structured_metadata_view metadata;
	struct mant_structured_source_view *sources;
	uint32_t source_count;
	uint32_t source_capacity;
	struct mant_structured_span_view *spans;
	uint32_t span_count;
	uint32_t span_capacity;
	struct mant_structured_diagnostic_view *diagnostics;
	uint32_t diagnostic_count;
	uint32_t diagnostic_capacity;
};

struct structured_source_line {
	uint32_t source;
	uint32_t line;
	uint64_t length;
};

struct structured_session {
	const struct mant_structured_input_view *input;
	const struct mant_input_source_view *inputs;
	const struct mant_structured_limits *limits;
	struct mant_structured_result *result;
	uint32_t *source_keys;
	struct structured_source_line *source_lines;
	uint32_t source_line_count;
	uint32_t source_line_capacity;
	uint32_t current_input;
	uint32_t status;
	uint32_t stage;
	uint32_t limit_kind;
	uint64_t observed;
	uint64_t allowed;
	uint64_t source_path_bytes;
	uint64_t decoded_bytes;
	uint64_t source_map_entries;
	uint64_t source_map_bytes;
	uint64_t builder_operations;
	uint64_t allocated_bytes;
	uint64_t content_bytes;
	uint64_t include_depth;
};

MANT_THREAD_LOCAL struct structured_session *active_session;
MANT_THREAD_LOCAL int structured_active;
MANT_THREAD_LOCAL uint64_t structured_fail_after = UINT64_MAX;
MANT_THREAD_LOCAL uint64_t structured_allocation_count;

static void clear_failure(struct mant_structured_failure_view *);
static void set_failure(struct structured_session *, uint32_t, uint32_t,
    uint32_t, uint64_t, uint64_t);
static int charge(struct structured_session *, uint64_t *, uint64_t,
    uint64_t, uint32_t, uint32_t);
static void *allocate(struct structured_session *, uint64_t, int, uint32_t);
static void *grow_array(struct structured_session *, void *, uint32_t,
    uint32_t *, uint32_t, size_t, uint64_t, uint32_t, uint32_t);
static uint8_t *copy_bytes(struct structured_session *, const uint8_t *,
    uint64_t, int, uint32_t);
static struct mant_bytes_view copy_cstring(struct structured_session *,
    const char *);
static int valid_bytes(struct mant_bytes_view);
static int valid_utf8(const uint8_t *, uint64_t);
static int safe_logical_name(const uint8_t *, uint64_t);
static int valid_identity_name(uint32_t, struct mant_bytes_view);
static int bytes_equal(struct mant_bytes_view, const uint8_t *, size_t);
static uint32_t find_input_exact(struct structured_session *, const uint8_t *,
    size_t);
static uint32_t find_input_beside(struct structured_session *, const uint8_t *,
    size_t);
static uint32_t register_source(struct structured_session *, uint32_t);
static int read_input(struct structured_session *, struct mparse *, uint32_t);
static void observe_source_line(void *, uint32_t, int, size_t);
static int valid_source_position(struct structured_session *, uint32_t,
    uint32_t, uint32_t);
static int check_source_positions(struct structured_session *);
static uint32_t diagnostic_level(enum mandoclevel);
static void observe_diagnostic(void *, enum mandocerr, enum mandoclevel,
    uint32_t, int, int, const char *, const char *, va_list *);
static int copy_metadata(struct structured_session *, const struct roff_meta *);
static int check_nesting_depth(struct structured_session *,
    const struct roff_node *);
static int validate_limits(const struct mant_structured_limits *);
static int validate_input(struct structured_session *);
static int check_result(const struct mant_structured_result *);
static void free_bytes(struct mant_bytes_view);

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
	return valid_bytes(view) && valid_utf8(view.ptr, view.len);
}

void
mant_structured_test_fail_after(uint64_t successful_allocations)
{
	structured_fail_after = successful_allocations;
}

static int
valid_source_position(struct structured_session *session, uint32_t source_key,
    uint32_t line, uint32_t column)
{
	uint32_t index;

	if (source_key == 0 || line == 0)
		return 0;
	for (index = 0; index < session->source_line_count; index++) {
		if (!charge(session, &session->builder_operations, 1,
		    session->limits->max_builder_operations, 8,
		    MANT_STRUCTURED_STAGE_CHECK))
			return 0;
		if (session->source_lines[index].source == source_key &&
		    session->source_lines[index].line == line)
			return (uint64_t)column <=
			    session->source_lines[index].length;
	}
	return 0;
}

static int
check_source_positions(struct structured_session *session)
{
	const struct mant_structured_span_view *span;
	uint32_t i;

	for (i = 0; i < session->result->span_count; i++) {
		span = session->result->spans + i;
		if (span->line_column_present == 0)
			continue;
		if (span->column_start == 0 ||
		    !valid_source_position(session, span->source,
		    span->line_start, span->column_start - 1))
			return 0;
	}
	return 1;
}

static uint32_t
diagnostic_level(enum mandoclevel level)
{
	switch (level) {
	case MANDOCLEVEL_STYLE:
		return MANT_DIAGNOSTIC_STYLE;
	case MANDOCLEVEL_WARNING:
		return MANT_DIAGNOSTIC_WARNING;
	case MANDOCLEVEL_UNSUPP:
		return MANT_DIAGNOSTIC_UNSUPPORTED;
	case MANDOCLEVEL_ERROR:
	case MANDOCLEVEL_BADARG:
	case MANDOCLEVEL_SYSERR:
		return MANT_DIAGNOSTIC_ERROR;
	default:
		return 0;
	}
}

static int
injected_allocation_failure(void)
{
	return structured_allocation_count++ >= structured_fail_after;
}

uint32_t
mant_structured_abi_version(void)
{
	return 1;
}

uint64_t
mant_structured_discriminant_fingerprint(void)
{
	static const uint32_t values[] = {
		/* status */ MANT_STRUCTURED_OK, MANT_STRUCTURED_INVALID_INPUT,
		MANT_STRUCTURED_REENTRANT, MANT_STRUCTURED_BUDGET,
		MANT_STRUCTURED_BUILDER_ALLOC, MANT_STRUCTURED_NATIVE,
		MANT_STRUCTURED_RELATION, MANT_STRUCTURED_UNSUPPORTED,
		/* stage */ 0, MANT_STRUCTURED_STAGE_MARSHAL,
		MANT_STRUCTURED_STAGE_RESOLVE, MANT_STRUCTURED_STAGE_PARSE,
		MANT_STRUCTURED_STAGE_RENDER, MANT_STRUCTURED_STAGE_FINALIZE,
		MANT_STRUCTURED_STAGE_CHECK,
		/* view */ 0, MANT_VIEW_INPUT_SOURCE, MANT_VIEW_INPUT,
		MANT_VIEW_FAILURE, MANT_VIEW_LIMITS, MANT_VIEW_RESULT,
		MANT_VIEW_SOURCE, MANT_VIEW_SPAN, MANT_VIEW_PROVENANCE,
		MANT_VIEW_OWNER, MANT_VIEW_CONTENT_ROOT, MANT_VIEW_CONTENT_ATOM,
		MANT_VIEW_CONTENT_REF, MANT_VIEW_CONTENT_POINT, MANT_VIEW_LINK,
		MANT_VIEW_BLOCK, MANT_VIEW_TABLE, MANT_VIEW_TABLE_ROW,
		MANT_VIEW_TABLE_CELL, MANT_VIEW_FIXED_VIEW, MANT_VIEW_FIXED_LINE,
		MANT_VIEW_PLACEMENT, MANT_VIEW_DECORATION, MANT_VIEW_FORM,
		MANT_VIEW_NAME_HINT, MANT_VIEW_RELATION, MANT_VIEW_DIAGNOSTIC,
		MANT_VIEW_METADATA,
		/* identity */ 0, MANT_IDENTITY_PATH, MANT_IDENTITY_BUNDLE_MEMBER,
		MANT_IDENTITY_ANONYMOUS,
		/* format */ 0, MANT_FORMAT_MAN, MANT_FORMAT_MDOC,
		MANT_FORMAT_MARKDOWN,
		/* profile */ 0, MANT_PROFILE_UTF8, MANT_PROFILE_ASCII,
		/* coordinate */ 0, MANT_COORD_DECODED_UTF8_BYTES,
		MANT_COORD_NATIVE_NORMALIZED_BYTES,
		/* diagnostic level and native range */ 0, MANT_DIAGNOSTIC_STYLE,
		MANT_DIAGNOSTIC_WARNING, MANT_DIAGNOSTIC_ERROR,
		MANT_DIAGNOSTIC_UNSUPPORTED, MANT_DIAGNOSTIC_CODE_NATIVE_FIRST,
		MANT_DIAGNOSTIC_CODE_NATIVE_LAST,
		/* provenance */ 0, MANT_PROVENANCE_AUTHORED,
		MANT_PROVENANCE_GENERATED, MANT_PROVENANCE_UNKNOWN,
		/* atom */ 0, MANT_ATOM_TEXT, MANT_ATOM_WHITESPACE,
		MANT_ATOM_BREAK_OPPORTUNITY, MANT_ATOM_HARD_BREAK,
		/* style bit values */ 0, MANT_STYLE_BOLD, MANT_STYLE_ITALIC,
		MANT_STYLE_LITERAL, MANT_STYLE_UNDERLINE,
		/* role */ 0, MANT_ROLE_FLAG, MANT_ROLE_ENVIRONMENT_VARIABLE,
		MANT_ROLE_ARGUMENT, MANT_ROLE_COMMAND_OR_DIRECTIVE, MANT_ROLE_PATH,
		/* owner */ 0, MANT_OWNER_DOCUMENT, MANT_OWNER_SECTION,
		MANT_OWNER_PARAGRAPH, MANT_OWNER_LIST_ITEM,
		MANT_OWNER_DEFINITION_ITEM, MANT_OWNER_TABLE_CELL,
		MANT_OWNER_FIXED_DISPLAY,
		/* root */ 0, MANT_ROOT_HEADING, MANT_ROOT_TERM, MANT_ROOT_BODY,
		MANT_ROOT_CELL, MANT_ROOT_FIXED_BODY,
		/* block */ 0, MANT_BLOCK_HEADING, MANT_BLOCK_PARAGRAPH,
		MANT_BLOCK_LIST, MANT_BLOCK_DEFINITION_LIST, MANT_BLOCK_TABLE,
		MANT_BLOCK_INDENTED, MANT_BLOCK_FIXED_DISPLAY,
		MANT_BLOCK_VERTICAL_SPACE, MANT_BLOCK_THEMATIC_BREAK,
		/* link target */ 0, MANT_LINK_EXTERNAL, MANT_LINK_EMAIL,
		MANT_LINK_DOCUMENT, MANT_LINK_MANUAL, MANT_LINK_SECTION,
		/* point boundary */ 0, MANT_POINT_BETWEEN_ATOMS,
		MANT_POINT_IN_ATOM,
		/* placement target */ 0, MANT_PLACEMENT_CONTENT,
		MANT_PLACEMENT_POINT,
		/* cell map */ 0, MANT_CELL_MAP_AFFINE,
		MANT_CELL_MAP_GRAPHEME_CLUSTER, MANT_CELL_MAP_OVERLAY,
		/* table cell */ 0, MANT_TABLE_CELL_TEXT,
		MANT_TABLE_CELL_HORIZONTAL_RULE,
		MANT_TABLE_CELL_DOUBLE_HORIZONTAL_RULE,
		MANT_TABLE_CELL_ISOLATED_HORIZONTAL_RULE,
		MANT_TABLE_CELL_ISOLATED_DOUBLE_HORIZONTAL_RULE,
		/* table alignment */ 0, MANT_TABLE_ALIGN_LEFT,
		MANT_TABLE_ALIGN_CENTER, MANT_TABLE_ALIGN_RIGHT,
		/* decoration */ 0, MANT_DECORATION_BORDER, MANT_DECORATION_RULE,
		MANT_DECORATION_PADDING,
		/* relation */ 0, MANT_RELATION_ALIAS,
		MANT_RELATION_READING_CONTEXT,
		/* resolver */ MANT_RESOLVE_FOUND, MANT_RESOLVE_NOT_FOUND,
		MANT_RESOLVE_DENIED, MANT_RESOLVE_IO, MANT_RESOLVE_PANIC,
		MANT_RESOLVE_INVALID,
		/* metadata presence bits */ 0, MANT_METADATA_TITLE_PRESENT,
		MANT_METADATA_SECTION_PRESENT, MANT_METADATA_VOLUME_PRESENT,
		MANT_METADATA_OS_PRESENT, MANT_METADATA_ARCH_PRESENT,
		MANT_METADATA_NAME_PRESENT, MANT_METADATA_DATE_PRESENT,
		MANT_METADATA_ALIAS_PRESENT
	};
	uint64_t hash = UINT64_C(14695981039346656037);
	size_t i;
	unsigned int shift;

	for (i = 0; i < sizeof(values) / sizeof(values[0]); i++)
		for (shift = 0; shift < 32; shift += 8) {
			hash ^= (values[i] >> shift) & 0xffU;
			hash *= UINT64_C(1099511628211);
		}
	return hash;
}

static void
clear_failure(struct mant_structured_failure_view *failure)
{
	if (failure != NULL)
		memset(failure, 0, sizeof(*failure));
}

static void
set_failure(struct structured_session *session, uint32_t status,
    uint32_t stage, uint32_t kind, uint64_t observed, uint64_t allowed)
{
	if (session->status != MANT_STRUCTURED_OK)
		return;
	session->status = status;
	session->stage = stage;
	session->limit_kind = kind;
	session->observed = observed;
	session->allowed = allowed;
}

static int
charge(struct structured_session *session, uint64_t *counter,
    uint64_t amount, uint64_t maximum, uint32_t kind, uint32_t stage)
{
	uint64_t observed;

	if (amount <= maximum && *counter <= maximum - amount) {
		*counter += amount;
		return 1;
	}
	observed = amount > UINT64_MAX - *counter ? UINT64_MAX :
	    *counter + amount;
	set_failure(session, MANT_STRUCTURED_BUDGET, stage, kind,
	    observed, maximum);
	return 0;
}

static void *
allocate(struct structured_session *session, uint64_t bytes, int zeroed,
    uint32_t stage)
{
	void *pointer;

	if (bytes == 0)
		return NULL;
	if (bytes > SIZE_MAX) {
		set_failure(session, MANT_STRUCTURED_BUDGET, stage, 9,
		    bytes, SIZE_MAX);
		return NULL;
	}
	if (!charge(session, &session->allocated_bytes, bytes,
	    session->limits->max_builder_allocated_bytes, 9, stage))
		return NULL;
	pointer = injected_allocation_failure() ? NULL :
	    (zeroed ? calloc(1, (size_t)bytes) : malloc((size_t)bytes));
	if (pointer == NULL)
		set_failure(session, MANT_STRUCTURED_BUILDER_ALLOC, stage, 0,
		    bytes, session->limits->max_builder_allocated_bytes);
	return pointer;
}

static void *
grow_array(struct structured_session *session, void *old, uint32_t count,
    uint32_t *capacity, uint32_t maximum, size_t element_size, uint64_t byte_limit,
    uint32_t limit_kind, uint32_t stage)
{
	void *grown;
	uint32_t new_capacity;
	uint64_t bytes, added_bytes;

	if (session->status != MANT_STRUCTURED_OK)
		return NULL;
	if (count >= maximum) {
		set_failure(session, MANT_STRUCTURED_BUDGET, stage, limit_kind,
		    (uint64_t)count + 1, maximum);
		return NULL;
	}
	if (count < *capacity)
		return old;
	new_capacity = *capacity == 0 ? 8 : *capacity;
	if (new_capacity > maximum)
		new_capacity = maximum;
	while (new_capacity <= count) {
		if (new_capacity > maximum / 2) {
			new_capacity = maximum;
			break;
		}
		new_capacity *= 2;
	}
	if (new_capacity <= count || (element_size != 0 &&
	    (uint64_t)new_capacity > UINT64_MAX / element_size)) {
		set_failure(session, MANT_STRUCTURED_BUDGET, stage,
		    limit_kind, UINT64_MAX,
		    byte_limit);
		return NULL;
	}
	bytes = (uint64_t)new_capacity * element_size;
	if (bytes > byte_limit || bytes > SIZE_MAX) {
		set_failure(session, MANT_STRUCTURED_BUDGET, stage, 9,
		    bytes, byte_limit < SIZE_MAX ? byte_limit : SIZE_MAX);
		return NULL;
	}
	added_bytes = (uint64_t)(new_capacity - *capacity) * element_size;
	if (!charge(session, &session->allocated_bytes, added_bytes,
	    session->limits->max_builder_allocated_bytes, 9,
	    stage))
		return NULL;
	grown = injected_allocation_failure() ? NULL : realloc(old, (size_t)bytes);
	if (grown == NULL) {
		set_failure(session, MANT_STRUCTURED_BUILDER_ALLOC, stage,
		    0, bytes, byte_limit);
		return NULL;
	}
	*capacity = new_capacity;
	return grown;
}

static uint8_t *
copy_bytes(struct structured_session *session, const uint8_t *bytes,
    uint64_t length, int content, uint32_t stage)
{
	uint8_t *copy;

	if (session->status != MANT_STRUCTURED_OK)
		return NULL;
	if (length == 0)
		return NULL;
	if (length > SIZE_MAX || (content != 0 &&
	    !charge(session, &session->content_bytes, length,
	    session->limits->max_content_bytes, 10,
	    stage)) ||
	    !charge(session, &session->allocated_bytes, length,
	    session->limits->max_builder_allocated_bytes, 9,
	    stage))
		return NULL;
	copy = injected_allocation_failure() ? NULL : malloc((size_t)length);
	if (copy == NULL) {
		set_failure(session, MANT_STRUCTURED_BUILDER_ALLOC, stage, 0,
		    length,
		    session->limits->max_builder_allocated_bytes);
		return NULL;
	}
	memcpy(copy, bytes, (size_t)length);
	return copy;
}

static struct mant_bytes_view
copy_cstring(struct structured_session *session, const char *string)
{
	struct mant_bytes_view view;

	memset(&view, 0, sizeof(view));
	if (string == NULL || *string == '\0')
		return view;
	view.len = strlen(string);
	view.ptr = copy_bytes(session, (const uint8_t *)string, view.len, 1,
	    MANT_STRUCTURED_STAGE_FINALIZE);
	if (view.ptr == NULL)
		view.len = 0;
	return view;
}

static int
valid_bytes(struct mant_bytes_view view)
{
	return (view.len == 0 && view.ptr == NULL) ||
	    (view.len != 0 && view.ptr != NULL);
}

static int
valid_utf8(const uint8_t *bytes, uint64_t length)
{
	uint64_t i;
	uint8_t c, need;
	uint32_t value, minimum;

	for (i = 0; i < length;) {
		c = bytes[i++];
		if (c < 0x80)
			continue;
		if ((c & 0xe0) == 0xc0) {
			need = 1; value = c & 0x1f; minimum = 0x80;
		} else if ((c & 0xf0) == 0xe0) {
			need = 2; value = c & 0x0f; minimum = 0x800;
		} else if ((c & 0xf8) == 0xf0) {
			need = 3; value = c & 0x07; minimum = 0x10000;
		} else
			return 0;
		if (i + need > length)
			return 0;
		while (need-- != 0) {
			c = bytes[i++];
			if ((c & 0xc0) != 0x80)
				return 0;
			value = (value << 6) | (c & 0x3f);
		}
		if (value < minimum || value > 0x10ffff ||
		    (value >= 0xd800 && value <= 0xdfff))
			return 0;
	}
	return 1;
}

static int
safe_logical_name(const uint8_t *path, uint64_t length)
{
	uint64_t start, i;

	if (length == 0 || path[0] == '/' || path[length - 1] == '/')
		return 0;
	for (start = i = 0; i <= length; i++) {
		if (i < length && path[i] != '/') {
			if (path[i] == '\\' || path[i] == '\0')
				return 0;
			continue;
		}
		if (i == start || (i - start == 1 && path[start] == '.') ||
		    (i - start == 2 && path[start] == '.' && path[start + 1] == '.'))
			return 0;
		start = i + 1;
	}
	return 1;
}

static int
valid_identity_name(uint32_t kind, struct mant_bytes_view name)
{
	uint64_t i;

	if (!valid_bytes(name) || name.len == 0 || name.len > SIZE_MAX ||
	    !valid_utf8(name.ptr, name.len))
		return 0;
	for (i = 0; i < name.len; i++)
		if (name.ptr[i] == '\0')
			return 0;
	return kind != MANT_IDENTITY_BUNDLE_MEMBER ||
	    safe_logical_name(name.ptr, name.len);
}

static int
bytes_equal(struct mant_bytes_view view, const uint8_t *bytes, size_t length)
{
	return view.len == length &&
	    (length == 0 || memcmp(view.ptr, bytes, length) == 0);
}

static uint32_t
find_input_exact(struct structured_session *session, const uint8_t *path,
    size_t length)
{
	uint32_t i;

	for (i = 0; i < session->input->sources.count; i++) {
		if (!charge(session, &session->builder_operations, 1,
		    session->limits->max_builder_operations, 8,
		    MANT_STRUCTURED_STAGE_RESOLVE))
			return 0;
		if (bytes_equal(session->inputs[i].resolver_name, path, length))
			return i + 1;
	}
	return 0;
}

static uint32_t
find_input_beside(struct structured_session *session, const uint8_t *path,
    size_t length)
{
	struct mant_bytes_view current, candidate;
	uint8_t *joined;
	uint64_t prefix, total;
	uint32_t found;

	if (session->current_input == 0)
		return 0;
	current = session->inputs[session->current_input - 1].resolver_name;
	for (prefix = current.len; prefix != 0 && current.ptr[prefix - 1] != '/';)
		prefix--;
	if (prefix == 0 || length > UINT64_MAX - prefix)
		return 0;
	total = prefix + length;
	if (total > SIZE_MAX)
		return 0;
	joined = allocate(session, total, 0, MANT_STRUCTURED_STAGE_RESOLVE);
	if (joined == NULL)
		return 0;
	memcpy(joined, current.ptr, (size_t)prefix);
	memcpy(joined + prefix, path, length);
	candidate.ptr = joined;
	candidate.len = total;
	found = 0;
	if (safe_logical_name(candidate.ptr, candidate.len))
		found = find_input_exact(session, candidate.ptr, (size_t)candidate.len);
	free(joined);
	return found;
}

static uint32_t
register_source(struct structured_session *session, uint32_t input_slot)
{
	const struct mant_input_source_view *input;
	struct mant_structured_source_view *grown, *source;
	uint32_t key;

	if (session->source_keys[input_slot - 1] != 0)
		return session->source_keys[input_slot - 1];
	if (!charge(session, &session->builder_operations, 2,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_RESOLVE))
		return 0;
	if (session->result->source_count >= session->limits->max_sources ||
	    session->result->source_count == UINT32_MAX) {
		set_failure(session, MANT_STRUCTURED_BUDGET,
		    MANT_STRUCTURED_STAGE_RESOLVE, 2,
		    (uint64_t)session->result->source_count + 1,
		    session->limits->max_sources);
		return 0;
	}
	grown = grow_array(session, session->result->sources,
	    session->result->source_count, &session->result->source_capacity,
	    (uint32_t)session->limits->max_sources, sizeof(*grown),
	    session->limits->max_builder_allocated_bytes, 2,
	    MANT_STRUCTURED_STAGE_RESOLVE);
	if (grown == NULL)
		return 0;
	session->result->sources = grown;
	source = grown + session->result->source_count;
	memset(source, 0, sizeof(*source));
	input = session->inputs + input_slot - 1;
	key = session->result->source_count + 1;
	source->key = key;
	source->identity_kind = input->identity_kind;
	source->format = input->format;
	source->coordinate_kind = MANT_COORD_NATIVE_NORMALIZED_BYTES;
	source->decoded_length = input->source_bytes.len;
	source->logical_name.len = input->logical_name.len;
	source->logical_name.ptr = copy_bytes(session, input->logical_name.ptr,
	    input->logical_name.len, 0, MANT_STRUCTURED_STAGE_RESOLVE);
	if (input->logical_name.len != 0 && source->logical_name.ptr == NULL)
		return 0;
	session->result->source_count++;
	session->source_keys[input_slot - 1] = key;
	return key;
}

static int
read_input(struct structured_session *session, struct mparse *parser,
    uint32_t input_slot)
{
	const struct mant_input_source_view *input;
	char *name;
	uint32_t saved_input, saved_key, key;
	int is_include;

	if (input_slot == 0 || input_slot > session->input->sources.count)
		return 0;
	is_include = session->current_input != 0;
	if (is_include &&
	    session->include_depth >= session->limits->max_include_depth) {
		set_failure(session, MANT_STRUCTURED_BUDGET,
		    MANT_STRUCTURED_STAGE_RESOLVE, 36,
		    session->include_depth + 1,
		    session->limits->max_include_depth);
		return 0;
	}
	key = register_source(session, input_slot);
	if (key == 0)
		return 0;
	input = session->inputs + input_slot - 1;
	if (input->logical_name.len >= SIZE_MAX)
		return 0;
	name = allocate(session, input->logical_name.len + 1, 0,
	    MANT_STRUCTURED_STAGE_RESOLVE);
	if (name == NULL)
		return 0;
	memcpy(name, input->logical_name.ptr, (size_t)input->logical_name.len);
	name[input->logical_name.len] = '\0';
	saved_input = session->current_input;
	saved_key = mandoc_msg_getsourcekey();
	session->current_input = input_slot;
	if (is_include)
		session->include_depth++;
	mandoc_msg_setsourcekey(key);
	mparse_readmem(parser, input->source_bytes.ptr,
	    (size_t)input->source_bytes.len, name);
	mandoc_msg_setsourcekey(saved_key);
	if (is_include)
		session->include_depth--;
	session->current_input = saved_input;
	free(name);
	return session->status == MANT_STRUCTURED_OK;
}

static void
observe_source_line(void *arg, uint32_t source_key, int line, size_t length)
{
	struct structured_session *session = arg;
	struct structured_source_line *entry, *grown;
	uint32_t i;

	if (session == NULL || session->status != MANT_STRUCTURED_OK ||
	    source_key == 0 || source_key > session->result->source_count ||
	    line <= 0)
		return;
	for (i = 0; i < session->source_line_count; i++) {
		entry = session->source_lines + i;
		if (entry->source != source_key)
			continue;
		if (entry->line == (uint32_t)line) {
			if ((uint64_t)length > entry->length)
				entry->length = length;
			return;
		}
	}
	if (!charge(session, &session->builder_operations, 1,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_PARSE) ||
	    !charge(session, &session->source_map_entries, 1,
	    session->limits->max_source_map_entries, 6,
	    MANT_STRUCTURED_STAGE_PARSE) ||
	    !charge(session, &session->source_map_bytes, sizeof(*entry),
	    session->limits->max_source_map_bytes, 7,
	    MANT_STRUCTURED_STAGE_PARSE))
		return;
	grown = grow_array(session, session->source_lines,
	    session->source_line_count, &session->source_line_capacity,
	    UINT32_MAX, sizeof(*grown),
	    session->limits->max_builder_allocated_bytes, 6,
	    MANT_STRUCTURED_STAGE_PARSE);
	if (grown == NULL)
		return;
	session->source_lines = grown;
	entry = grown + session->source_line_count++;
	entry->source = source_key;
	entry->line = (uint32_t)line;
	entry->length = length;
}

int
mant_structured_read_bundle(struct mparse *parser, const char *requested)
{
	struct structured_session *session;
	struct mant_bytes_view requested_view;
	uint32_t slot, callback_status;
	int callback_used;

	session = active_session;
	callback_used = 0;
	if (session == NULL)
		return 0;
	if (requested == NULL || *requested == '\0') {
		errno = EINVAL;
		return -1;
	}
	requested_view.ptr = (const uint8_t *)requested;
	requested_view.len = strlen(requested);
	if (!safe_logical_name(requested_view.ptr, requested_view.len)) {
		errno = EPERM;
		return -1;
	}
	slot = find_input_exact(session, requested_view.ptr,
	    (size_t)requested_view.len);
	if (slot == 0)
		slot = find_input_beside(session, requested_view.ptr,
		    (size_t)requested_view.len);
	if (session->status != MANT_STRUCTURED_OK) {
		errno = session->status == MANT_STRUCTURED_BUDGET ? EFBIG : ENOMEM;
		return -1;
	}
	if (slot == 0 && session->input->resolve != NULL) {
		callback_used = 1;
		slot = 0;
		callback_status = session->input->resolve(
		    session->input->resolve_context, session->current_input,
		    requested_view, &slot);
		if (callback_status != MANT_RESOLVE_FOUND) {
			if (slot != 0 || callback_status > MANT_RESOLVE_INVALID ||
			    callback_status == MANT_RESOLVE_INVALID)
				set_failure(session, MANT_STRUCTURED_INVALID_INPUT,
				    MANT_STRUCTURED_STAGE_RESOLVE, 0,
				    callback_status, MANT_RESOLVE_INVALID);
			else if (callback_status != MANT_RESOLVE_NOT_FOUND)
				set_failure(session, MANT_STRUCTURED_NATIVE,
				    MANT_STRUCTURED_STAGE_RESOLVE, 0,
				    callback_status, 0);
			slot = 0;
		}
		if (session->status != MANT_STRUCTURED_OK) {
			errno = callback_status == MANT_RESOLVE_DENIED ? EACCES : EIO;
			return -1;
		}
		if (callback_status == MANT_RESOLVE_NOT_FOUND) {
			errno = ENOENT;
			return -1;
		}
		if (slot == 0 || slot > session->input->sources.count) {
			set_failure(session, MANT_STRUCTURED_INVALID_INPUT,
			    MANT_STRUCTURED_STAGE_RESOLVE, 0, slot,
			    session->input->sources.count);
			errno = EINVAL;
			return -1;
		}
	}
	if (slot == 0 || slot > session->input->sources.count ||
	    (!bytes_equal(session->inputs[slot - 1].resolver_name,
	    requested_view.ptr, (size_t)requested_view.len) &&
	    slot != find_input_beside(session, requested_view.ptr,
	    (size_t)requested_view.len))) {
		if (session->status != MANT_STRUCTURED_OK) {
			errno = session->status == MANT_STRUCTURED_BUDGET ? EFBIG : ENOMEM;
			return -1;
		}
		if (callback_used)
			set_failure(session, MANT_STRUCTURED_INVALID_INPUT,
			    MANT_STRUCTURED_STAGE_RESOLVE, 0, slot, 0);
		errno = ENOENT;
		return -1;
	}
	if (!read_input(session, parser, slot)) {
		errno = session->status == MANT_STRUCTURED_BUDGET ? EFBIG : ENOMEM;
		return -1;
	}
	return 1;
}

static void
observe_diagnostic(void *arg, enum mandocerr code, enum mandoclevel level,
    uint32_t source_key, int line, int column, const char *native_message,
    const char *format, va_list *arguments)
{
	struct structured_session *session = arg;
	struct mant_structured_diagnostic_view *diagnostic, *grown_diagnostics;
	struct mant_structured_span_view *span, *grown_spans;
	char *message;
	va_list measured, written;
	uint64_t base_length, detail_length, message_length;
	int needed, written_count, has_span;
	uint32_t span_key;

	if (session == NULL || session->status != MANT_STRUCTURED_OK)
		return;
	if (!charge(session, &session->builder_operations, 1,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_PARSE))
		return;
	if (session->result->diagnostic_count >=
	    session->limits->max_diagnostics ||
	    session->result->diagnostic_count == UINT32_MAX) {
		set_failure(session, MANT_STRUCTURED_BUDGET,
		    MANT_STRUCTURED_STAGE_PARSE, 31,
		    (uint64_t)session->result->diagnostic_count + 1,
		    session->limits->max_diagnostics);
		return;
	}
	base_length = native_message == NULL ? 0 : strlen(native_message);
	detail_length = 0;
	if (format != NULL && arguments != NULL) {
		va_copy(measured, *arguments);
		needed = vsnprintf(NULL, 0, format, measured);
		va_end(measured);
		if (needed < 0) {
			set_failure(session, MANT_STRUCTURED_NATIVE,
			    MANT_STRUCTURED_STAGE_PARSE, 0, 0, 0);
			return;
		}
		detail_length = (uint64_t)needed;
	}
	if (base_length > UINT64_MAX - detail_length ||
	    (base_length != 0 && detail_length != 0 &&
	    base_length + detail_length > UINT64_MAX - 2)) {
		set_failure(session, MANT_STRUCTURED_BUDGET,
		    MANT_STRUCTURED_STAGE_PARSE, 10, UINT64_MAX,
		    session->limits->max_content_bytes);
		return;
	}
	message_length = base_length + detail_length +
	    (base_length != 0 && detail_length != 0 ? 2 : 0);
	if (!charge(session, &session->content_bytes, message_length,
	    session->limits->max_content_bytes, 10,
	    MANT_STRUCTURED_STAGE_PARSE) || message_length == UINT64_MAX)
		return;
	message = NULL;
	if (message_length != 0) {
		message = allocate(session, message_length + 1, 0,
		    MANT_STRUCTURED_STAGE_PARSE);
		if (message == NULL)
			return;
	}
	if (base_length != 0)
		memcpy(message, native_message, (size_t)base_length);
	if (base_length != 0 && detail_length != 0)
		memcpy(message + base_length, ": ", 2);
	if (detail_length != 0) {
		va_copy(written, *arguments);
		written_count = vsnprintf(message + base_length +
		    (base_length != 0 ? 2 : 0), (size_t)detail_length + 1,
		    format, written);
		va_end(written);
		if (written_count < 0 || (uint64_t)written_count != detail_length) {
			free(message);
			set_failure(session, MANT_STRUCTURED_NATIVE,
			    MANT_STRUCTURED_STAGE_PARSE, 0, 0, 0);
			return;
		}
	} else if (message != NULL)
		message[message_length] = '\0';
	has_span = source_key != 0 && source_key <= session->result->source_count &&
	    line > 0 && column >= 0;
	if (has_span) {
		if (!charge(session, &session->builder_operations, 1,
		    session->limits->max_builder_operations, 8,
		    MANT_STRUCTURED_STAGE_PARSE)) {
			free(message);
			return;
		}
		grown_spans = grow_array(session, session->result->spans,
		    session->result->span_count, &session->result->span_capacity,
		    UINT32_MAX,
		    sizeof(*grown_spans),
		    session->limits->max_builder_allocated_bytes, 6,
		    MANT_STRUCTURED_STAGE_PARSE);
		if (grown_spans == NULL) {
			free(message);
			return;
		}
		session->result->spans = grown_spans;
	}
	grown_diagnostics = grow_array(session, session->result->diagnostics,
	    session->result->diagnostic_count,
	    &session->result->diagnostic_capacity,
	    (uint32_t)session->limits->max_diagnostics,
	    sizeof(*grown_diagnostics),
	    session->limits->max_builder_allocated_bytes, 31,
	    MANT_STRUCTURED_STAGE_PARSE);
	if (grown_diagnostics == NULL) {
		free(message);
		return;
	}
	session->result->diagnostics = grown_diagnostics;
	span_key = 0;
	if (has_span) {
		span = session->result->spans + session->result->span_count;
		memset(span, 0, sizeof(*span));
		span->line_column_present = 1;
		span->source = source_key;
		span->line_start = (uint32_t)line;
		span->column_start = (uint32_t)column + 1;
		span_key = ++session->result->span_count;
	}
	diagnostic = grown_diagnostics + session->result->diagnostic_count;
	memset(diagnostic, 0, sizeof(*diagnostic));
	diagnostic->level = diagnostic_level(level);
	diagnostic->code = (uint32_t)code + 1;
	diagnostic->message.len = message_length;
	diagnostic->message.ptr = (const uint8_t *)message;
	diagnostic->span = span_key;
	session->result->diagnostic_count++;
	if (diagnostic->level == 0 ||
	    diagnostic->code < MANT_DIAGNOSTIC_CODE_NATIVE_FIRST ||
	    diagnostic->code > MANT_DIAGNOSTIC_CODE_NATIVE_LAST)
		set_failure(session, MANT_STRUCTURED_NATIVE,
		    MANT_STRUCTURED_STAGE_PARSE, 0, 0, 0);
}

static int
copy_metadata(struct structured_session *session, const struct roff_meta *meta)
{
	struct mant_structured_metadata_view *out = &session->result->metadata;

	if (!charge(session, &session->builder_operations, 9,
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
	out->title = copy_cstring(session, meta->title);
	out->section = copy_cstring(session, meta->msec);
	out->volume = copy_cstring(session, meta->vol);
	out->operating_system = copy_cstring(session, meta->os);
	out->architecture = copy_cstring(session, meta->arch);
	out->name = copy_cstring(session, meta->name);
	out->date = copy_cstring(session, meta->date);
	out->alias_target = copy_cstring(session, meta->sodest);
	out->has_body = meta->hasbody != 0;
	return session->status == MANT_STRUCTURED_OK;
}

static int
check_nesting_depth(struct structured_session *session,
    const struct roff_node *node)
{
	uint64_t depth;

	if (node == NULL)
		return 1;
	depth = 1;
	for (;;) {
		if (depth > session->limits->max_nesting_depth) {
			set_failure(session, MANT_STRUCTURED_BUDGET,
			    MANT_STRUCTURED_STAGE_PARSE, 35, depth,
			    session->limits->max_nesting_depth);
			return 0;
		}
		if (node->child != NULL) {
			node = node->child;
			depth++;
			continue;
		}
		while (node->next == NULL) {
			node = node->parent;
			if (node == NULL)
				return 1;
			depth--;
		}
		node = node->next;
	}
}

static int
validate_limits(const struct mant_structured_limits *limits)
{
	const uint64_t *value;
	size_t count, i;

	if (limits == NULL || limits->reserved != 0)
		return 0;
	value = &limits->max_input_sources;
	count = (offsetof(struct mant_structured_limits, reserved) -
	    offsetof(struct mant_structured_limits, max_input_sources)) /
	    sizeof(uint64_t);
	for (i = 0; i < count; i++)
		if (value[i] == 0)
			return 0;
	return limits->max_input_sources <= UINT32_MAX &&
	    limits->max_sources <= UINT32_MAX &&
	    limits->max_diagnostics <= UINT32_MAX;
}

static int
validate_input(struct structured_session *session)
{
	const struct mant_structured_input_view *input = session->input;
	const struct mant_input_source_view *source;
	uint32_t i, j, root_format;
	uint64_t name_bytes;

	if (input == NULL || input->reserved != 0 || input->width == 0 ||
	    (input->profile != MANT_PROFILE_UTF8 &&
	    input->profile != MANT_PROFILE_ASCII) ||
	    input->sources.count == 0 || input->sources.ptr == NULL ||
	    input->sources.stride != sizeof(*session->inputs) ||
	    input->root_input == 0 || input->root_input > input->sources.count ||
	    (uintptr_t)input->sources.ptr %
	    _Alignof(struct mant_input_source_view) != 0)
		return 0;
#if SIZE_MAX < UINT64_MAX
	if ((uint64_t)input->sources.count >
	    SIZE_MAX / sizeof(*session->inputs))
		return 0;
#endif
	if (input->sources.count > session->limits->max_input_sources) {
		set_failure(session, MANT_STRUCTURED_BUDGET,
		    MANT_STRUCTURED_STAGE_MARSHAL, 1, input->sources.count,
		    session->limits->max_input_sources);
		return 0;
	}
	if (input->sources.count > session->limits->max_source_map_entries) {
		set_failure(session, MANT_STRUCTURED_BUDGET,
		    MANT_STRUCTURED_STAGE_MARSHAL, 6, input->sources.count,
		    session->limits->max_source_map_entries);
		return 0;
	}
	session->inputs = input->sources.ptr;
	root_format = session->inputs[input->root_input - 1].format;
	if (root_format != MANT_FORMAT_MAN && root_format != MANT_FORMAT_MDOC)
		return 0;
	for (i = 0; i < input->sources.count; i++) {
		source = session->inputs + i;
		if (source->reserved != 0 ||
		    (source->identity_kind < MANT_IDENTITY_PATH ||
		    source->identity_kind > MANT_IDENTITY_ANONYMOUS) ||
		    source->format != root_format ||
		    !valid_identity_name(source->identity_kind,
		    source->logical_name) ||
		    !valid_bytes(source->resolver_name) ||
		    !valid_bytes(source->source_bytes) ||
		    source->resolver_name.len > SIZE_MAX ||
		    !valid_utf8(source->resolver_name.ptr,
		    source->resolver_name.len) ||
		    !safe_logical_name(source->resolver_name.ptr,
		    source->resolver_name.len) ||
		    (source->identity_kind == MANT_IDENTITY_BUNDLE_MEMBER &&
		    !bytes_equal(source->logical_name, source->resolver_name.ptr,
		    (size_t)source->resolver_name.len)) ||
		    source->source_bytes.len > SIZE_MAX)
			return 0;
		if (source->source_bytes.len >
		    session->limits->max_decoded_source_bytes_per_source) {
			set_failure(session, MANT_STRUCTURED_BUDGET,
			    MANT_STRUCTURED_STAGE_MARSHAL, 4,
			    source->source_bytes.len,
			    session->limits->max_decoded_source_bytes_per_source);
			return 0;
		}
		name_bytes = source->logical_name.len;
		if (source->identity_kind != MANT_IDENTITY_BUNDLE_MEMBER &&
		    (source->resolver_name.len > UINT64_MAX - name_bytes))
			return 0;
		if (source->identity_kind != MANT_IDENTITY_BUNDLE_MEMBER)
			name_bytes += source->resolver_name.len;
		if (
		    !charge(session, &session->source_path_bytes,
		    name_bytes,
		    session->limits->max_source_path_bytes, 3,
		    MANT_STRUCTURED_STAGE_MARSHAL) ||
		    !charge(session, &session->decoded_bytes,
		    source->source_bytes.len,
		    session->limits->max_decoded_source_bytes_total, 5,
		    MANT_STRUCTURED_STAGE_MARSHAL))
			return 0;
		for (j = 0; j < i; j++)
			if (bytes_equal(session->inputs[j].resolver_name,
			    source->resolver_name.ptr,
			    (size_t)source->resolver_name.len))
				return 0;
	}
	return 1;
}

uint32_t
mant_structured_render(const struct mant_structured_input_view *input,
    const struct mant_structured_limits *limits,
    struct mant_structured_result **out_result,
    struct mant_structured_failure_view *failure)
{
	struct structured_session session;
	struct mant_structured_result *result;
	struct mparse *parser;
	struct roff_meta *meta;
	struct mandoc_msg_state message_state;
	uint64_t source_map_bytes;
	uint32_t status;
	int options, message_state_saved, mchars_ready;

	if (out_result == NULL || failure == NULL)
		return MANT_STRUCTURED_INVALID_INPUT;
	*out_result = NULL;
	clear_failure(failure);
	if (structured_active) {
		failure->status = MANT_STRUCTURED_REENTRANT;
		failure->stage = MANT_STRUCTURED_STAGE_MARSHAL;
		return failure->status;
	}
	structured_active = 1;
	structured_allocation_count = 0;
	memset(&session, 0, sizeof(session));
	session.input = input;
	session.limits = limits;
	session.status = MANT_STRUCTURED_OK;
	result = NULL;
	parser = NULL;
	message_state_saved = 0;
	mchars_ready = 0;
	if (!validate_limits(limits)) {
		set_failure(&session, MANT_STRUCTURED_INVALID_INPUT,
		    MANT_STRUCTURED_STAGE_MARSHAL, 0, 0, 0);
		goto cleanup;
	}
	if (!validate_input(&session)) {
		if (session.status == MANT_STRUCTURED_OK)
			set_failure(&session, MANT_STRUCTURED_INVALID_INPUT,
			    MANT_STRUCTURED_STAGE_MARSHAL, 0, 0, 0);
		goto cleanup;
	}
	result = allocate(&session, sizeof(*result), 1,
	    MANT_STRUCTURED_STAGE_MARSHAL);
	if (result == NULL)
		goto cleanup;
	session.result = result;
	source_map_bytes = (uint64_t)input->sources.count *
	    sizeof(*session.source_keys);
	if (!charge(&session, &session.source_map_entries,
	    input->sources.count, limits->max_source_map_entries, 6,
	    MANT_STRUCTURED_STAGE_MARSHAL) ||
	    !charge(&session, &session.source_map_bytes, source_map_bytes,
	    limits->max_source_map_bytes, 7, MANT_STRUCTURED_STAGE_MARSHAL))
		goto cleanup;
	session.source_keys = allocate(&session, source_map_bytes, 1,
	    MANT_STRUCTURED_STAGE_MARSHAL);
	if (session.source_keys == NULL)
		goto cleanup;
	options = MPARSE_UTF8 | MPARSE_LATIN1 | MPARSE_VALIDATE |
	    MPARSE_COMMENT | MPARSE_SO;
	if (session.inputs[input->root_input - 1].format == MANT_FORMAT_MAN)
		options |= MPARSE_MAN;
	else
		options |= MPARSE_MDOC;
	/* Structured diagnostics are captured by the bounded observer.  Leaving
	 * the legacy FILE sink disabled prevents an unbounded duplicate stream. */
	mandoc_msg_getstate(&message_state);
	message_state_saved = 1;
	mandoc_msg_setoutfile(NULL);
	mandoc_msg_setmin(MANDOCERR_BASE);
	mandoc_msg_setobserver(observe_diagnostic, &session);
	mandoc_msg_setlineobserver(observe_source_line, &session);
	active_session = &session;
	mchars_alloc();
	mchars_ready = 1;
	parser = mparse_alloc(options, MANDOC_OS_OTHER, NULL);
	if (!read_input(&session, parser, input->root_input))
		goto native_cleanup;
	mandoc_msg_setsourcekey(session.source_keys[input->root_input - 1]);
	meta = mparse_result(parser);
	if (meta == NULL) {
		set_failure(&session, MANT_STRUCTURED_NATIVE,
		    MANT_STRUCTURED_STAGE_PARSE, 0, 0, 0);
		goto native_cleanup;
	}
	if (!check_nesting_depth(&session, meta->first))
		goto native_cleanup;
	result->root_source = session.source_keys[input->root_input - 1];
	result->profile = input->profile;
	result->width = input->width;
	if (!copy_metadata(&session, meta))
		goto native_cleanup;
	if (meta->hasbody) {
		set_failure(&session, MANT_STRUCTURED_UNSUPPORTED,
		    MANT_STRUCTURED_STAGE_RENDER, 0, 1, 0);
		goto native_cleanup;
	}
	result->magic = MANT_STRUCTURED_MAGIC;

native_cleanup:
	if (parser != NULL)
		mparse_free(parser);
	parser = NULL;
	if (mchars_ready)
		mchars_free();
	active_session = NULL;
	if (message_state_saved)
		mandoc_msg_setstate(&message_state);
	if (session.status == MANT_STRUCTURED_OK) {
		if (!check_source_positions(&session) &&
		    session.status == MANT_STRUCTURED_OK)
			set_failure(&session, MANT_STRUCTURED_RELATION,
			    MANT_STRUCTURED_STAGE_CHECK, 0, 0, 0);
		else if (session.status == MANT_STRUCTURED_OK &&
		    !check_result(result))
			set_failure(&session, MANT_STRUCTURED_RELATION,
			    MANT_STRUCTURED_STAGE_CHECK, 0, 0, 0);
		else if (session.status == MANT_STRUCTURED_OK)
			result->checked = 1;
	}

cleanup:
	status = session.status;
	if (status == MANT_STRUCTURED_OK) {
		*out_result = result;
		result = NULL;
	} else {
		failure->status = status;
		failure->stage = session.stage;
		failure->limit_kind = session.limit_kind;
		failure->observed = session.observed;
		failure->allowed = session.allowed;
	}
	free(session.source_keys);
	free(session.source_lines);
	mant_structured_result_free(result);
	structured_fail_after = UINT64_MAX;
	structured_active = 0;
	return status;
}

static int
check_result(const struct mant_structured_result *result)
{
	const struct mant_structured_metadata_view *metadata;
	const struct mant_structured_source_view *source;
	const struct mant_structured_span_view *span;
	const struct mant_structured_diagnostic_view *diagnostic;
	struct mant_bytes_view metadata_strings[8];
	uint32_t metadata_flags[] = {
		MANT_METADATA_TITLE_PRESENT, MANT_METADATA_SECTION_PRESENT,
		MANT_METADATA_VOLUME_PRESENT, MANT_METADATA_OS_PRESENT,
		MANT_METADATA_ARCH_PRESENT, MANT_METADATA_NAME_PRESENT,
		MANT_METADATA_DATE_PRESENT, MANT_METADATA_ALIAS_PRESENT };
	uint32_t i;

	if (result == NULL || result->magic != MANT_STRUCTURED_MAGIC ||
	    result->root_source == 0 ||
	    result->root_source > result->source_count ||
	    (result->profile != MANT_PROFILE_UTF8 &&
	    result->profile != MANT_PROFILE_ASCII) || result->width == 0 ||
	    result->metadata.reserved != 0 ||
	    (result->source_count == 0) ||
	    (result->source_count != 0) != (result->sources != NULL) ||
	    (result->span_count != 0) != (result->spans != NULL) ||
	    (result->diagnostic_count != 0) != (result->diagnostics != NULL))
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
		    !valid_identity_name(source->identity_kind,
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
		if (span->byte_range_present == 0) {
			if (span->byte_start != 0 || span->byte_end != 0)
				return 0;
		} else if (span->byte_start > span->byte_end ||
		    span->byte_end > result->sources[span->source - 1].decoded_length)
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
		    diagnostic->owner != 0)
			return 0;
	}
	return 1;
}

uint32_t
mant_structured_result_check(const struct mant_structured_result *result,
    struct mant_structured_failure_view *failure)
{
	clear_failure(failure);
	if (failure == NULL)
		return MANT_STRUCTURED_INVALID_INPUT;
	if (!check_result(result) || result->checked == 0) {
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
	if (!check_result(result) || result->checked == 0)
		return MANT_STRUCTURED_RELATION;
	view->root_source = result->root_source;
	view->profile = result->profile;
	view->width = result->width;
	view->metadata = result->metadata;
	view->sources = SLICE(result->sources, result->source_count);
	view->spans = SLICE(result->spans, result->span_count);
	view->provenances = EMPTY_SLICE(struct mant_structured_provenance_view);
	view->owners = EMPTY_SLICE(struct mant_structured_owner_view);
	view->content_roots = EMPTY_SLICE(struct mant_structured_content_root_view);
	view->content_atoms = EMPTY_SLICE(struct mant_structured_content_atom_view);
	view->content_refs = EMPTY_SLICE(struct mant_structured_content_ref_view);
	view->content_points = EMPTY_SLICE(struct mant_structured_content_point_view);
	view->links = EMPTY_SLICE(struct mant_structured_link_view);
	view->blocks = EMPTY_SLICE(struct mant_structured_block_view);
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

static void
free_bytes(struct mant_bytes_view view)
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
		free_bytes(result->sources[i].logical_name);
	for (i = 0; i < result->diagnostic_count; i++)
		free_bytes(result->diagnostics[i].message);
	free_bytes(result->metadata.title);
	free_bytes(result->metadata.section);
	free_bytes(result->metadata.volume);
	free_bytes(result->metadata.operating_system);
	free_bytes(result->metadata.architecture);
	free_bytes(result->metadata.name);
	free_bytes(result->metadata.date);
	free_bytes(result->metadata.alias_target);
	free(result->sources);
	free(result->spans);
	free(result->diagnostics);
	result->magic = 0;
	free(result);
}

#if defined(_MSC_VER)
#define MANT_ALIGNOF(type) __alignof(type)
#else
#define MANT_ALIGNOF(type) _Alignof(type)
#endif

#define VIEW_CASE(id, type) case id: return sizeof(type)
size_t
mant_structured_view_size(uint32_t kind)
{
	switch (kind) {
	VIEW_CASE(MANT_VIEW_INPUT_SOURCE, struct mant_input_source_view);
	VIEW_CASE(MANT_VIEW_INPUT, struct mant_structured_input_view);
	VIEW_CASE(MANT_VIEW_FAILURE, struct mant_structured_failure_view);
	VIEW_CASE(MANT_VIEW_LIMITS, struct mant_structured_limits);
	VIEW_CASE(MANT_VIEW_RESULT, struct mant_structured_result_view);
	VIEW_CASE(MANT_VIEW_SOURCE, struct mant_structured_source_view);
	VIEW_CASE(MANT_VIEW_SPAN, struct mant_structured_span_view);
	VIEW_CASE(MANT_VIEW_PROVENANCE, struct mant_structured_provenance_view);
	VIEW_CASE(MANT_VIEW_OWNER, struct mant_structured_owner_view);
	VIEW_CASE(MANT_VIEW_CONTENT_ROOT, struct mant_structured_content_root_view);
	VIEW_CASE(MANT_VIEW_CONTENT_ATOM, struct mant_structured_content_atom_view);
	VIEW_CASE(MANT_VIEW_CONTENT_REF, struct mant_structured_content_ref_view);
	VIEW_CASE(MANT_VIEW_CONTENT_POINT, struct mant_structured_content_point_view);
	VIEW_CASE(MANT_VIEW_LINK, struct mant_structured_link_view);
	VIEW_CASE(MANT_VIEW_BLOCK, struct mant_structured_block_view);
	VIEW_CASE(MANT_VIEW_TABLE, struct mant_structured_table_view);
	VIEW_CASE(MANT_VIEW_TABLE_ROW, struct mant_structured_table_row_view);
	VIEW_CASE(MANT_VIEW_TABLE_CELL, struct mant_structured_table_cell_view);
	VIEW_CASE(MANT_VIEW_FIXED_VIEW, struct mant_structured_fixed_view);
	VIEW_CASE(MANT_VIEW_FIXED_LINE, struct mant_structured_fixed_line_view);
	VIEW_CASE(MANT_VIEW_PLACEMENT, struct mant_structured_placement_view);
	VIEW_CASE(MANT_VIEW_DECORATION, struct mant_structured_decoration_view);
	VIEW_CASE(MANT_VIEW_FORM, struct mant_structured_form_view);
	VIEW_CASE(MANT_VIEW_NAME_HINT, struct mant_structured_name_hint_view);
	VIEW_CASE(MANT_VIEW_RELATION, struct mant_structured_relation_view);
	VIEW_CASE(MANT_VIEW_DIAGNOSTIC, struct mant_structured_diagnostic_view);
	VIEW_CASE(MANT_VIEW_METADATA, struct mant_structured_metadata_view);
	default: return 0;
	}
}

#undef VIEW_CASE
#define VIEW_CASE(id, type) case id: return MANT_ALIGNOF(type)
size_t
mant_structured_view_align(uint32_t kind)
{
	switch (kind) {
	VIEW_CASE(MANT_VIEW_INPUT_SOURCE, struct mant_input_source_view);
	VIEW_CASE(MANT_VIEW_INPUT, struct mant_structured_input_view);
	VIEW_CASE(MANT_VIEW_FAILURE, struct mant_structured_failure_view);
	VIEW_CASE(MANT_VIEW_LIMITS, struct mant_structured_limits);
	VIEW_CASE(MANT_VIEW_RESULT, struct mant_structured_result_view);
	VIEW_CASE(MANT_VIEW_SOURCE, struct mant_structured_source_view);
	VIEW_CASE(MANT_VIEW_SPAN, struct mant_structured_span_view);
	VIEW_CASE(MANT_VIEW_PROVENANCE, struct mant_structured_provenance_view);
	VIEW_CASE(MANT_VIEW_OWNER, struct mant_structured_owner_view);
	VIEW_CASE(MANT_VIEW_CONTENT_ROOT, struct mant_structured_content_root_view);
	VIEW_CASE(MANT_VIEW_CONTENT_ATOM, struct mant_structured_content_atom_view);
	VIEW_CASE(MANT_VIEW_CONTENT_REF, struct mant_structured_content_ref_view);
	VIEW_CASE(MANT_VIEW_CONTENT_POINT, struct mant_structured_content_point_view);
	VIEW_CASE(MANT_VIEW_LINK, struct mant_structured_link_view);
	VIEW_CASE(MANT_VIEW_BLOCK, struct mant_structured_block_view);
	VIEW_CASE(MANT_VIEW_TABLE, struct mant_structured_table_view);
	VIEW_CASE(MANT_VIEW_TABLE_ROW, struct mant_structured_table_row_view);
	VIEW_CASE(MANT_VIEW_TABLE_CELL, struct mant_structured_table_cell_view);
	VIEW_CASE(MANT_VIEW_FIXED_VIEW, struct mant_structured_fixed_view);
	VIEW_CASE(MANT_VIEW_FIXED_LINE, struct mant_structured_fixed_line_view);
	VIEW_CASE(MANT_VIEW_PLACEMENT, struct mant_structured_placement_view);
	VIEW_CASE(MANT_VIEW_DECORATION, struct mant_structured_decoration_view);
	VIEW_CASE(MANT_VIEW_FORM, struct mant_structured_form_view);
	VIEW_CASE(MANT_VIEW_NAME_HINT, struct mant_structured_name_hint_view);
	VIEW_CASE(MANT_VIEW_RELATION, struct mant_structured_relation_view);
	VIEW_CASE(MANT_VIEW_DIAGNOSTIC, struct mant_structured_diagnostic_view);
	VIEW_CASE(MANT_VIEW_METADATA, struct mant_structured_metadata_view);
	default: return 0;
	}
}

#define FIELD(type, member) offsetof(type, member)
#define PICK(array) ((field > 0 && field <= sizeof(array) / sizeof(array[0])) ? \
	array[field - 1] : SIZE_MAX)

size_t
mant_structured_view_offset(uint32_t kind, uint32_t field)
{
	static const size_t input_source[] = {
		FIELD(struct mant_input_source_view, identity_kind),
		FIELD(struct mant_input_source_view, format),
		FIELD(struct mant_input_source_view, logical_name),
		FIELD(struct mant_input_source_view, resolver_name),
		FIELD(struct mant_input_source_view, source_bytes),
		FIELD(struct mant_input_source_view, reserved) };
	static const size_t input[] = {
		FIELD(struct mant_structured_input_view, sources),
		FIELD(struct mant_structured_input_view, root_input),
		FIELD(struct mant_structured_input_view, profile),
		FIELD(struct mant_structured_input_view, width),
		FIELD(struct mant_structured_input_view, resolve),
		FIELD(struct mant_structured_input_view, resolve_context),
		FIELD(struct mant_structured_input_view, reserved) };
	static const size_t failure[] = {
		FIELD(struct mant_structured_failure_view, status),
		FIELD(struct mant_structured_failure_view, stage),
		FIELD(struct mant_structured_failure_view, limit_kind),
		FIELD(struct mant_structured_failure_view, observed),
		FIELD(struct mant_structured_failure_view, allowed),
		FIELD(struct mant_structured_failure_view, reserved) };
	static const size_t limits[] = {
		FIELD(struct mant_structured_limits, max_input_sources),
		FIELD(struct mant_structured_limits, max_sources),
		FIELD(struct mant_structured_limits, max_source_path_bytes),
		FIELD(struct mant_structured_limits, max_decoded_source_bytes_per_source),
		FIELD(struct mant_structured_limits, max_decoded_source_bytes_total),
		FIELD(struct mant_structured_limits, max_source_map_entries),
		FIELD(struct mant_structured_limits, max_source_map_bytes),
		FIELD(struct mant_structured_limits, max_builder_operations),
		FIELD(struct mant_structured_limits, max_builder_allocated_bytes),
		FIELD(struct mant_structured_limits, max_content_bytes),
		FIELD(struct mant_structured_limits, max_owners),
		FIELD(struct mant_structured_limits, max_blocks),
		FIELD(struct mant_structured_limits, max_content_atoms),
		FIELD(struct mant_structured_limits, max_content_refs),
		FIELD(struct mant_structured_limits, max_content_points),
		FIELD(struct mant_structured_limits, max_links),
		FIELD(struct mant_structured_limits, max_tables),
		FIELD(struct mant_structured_limits, max_table_rows),
		FIELD(struct mant_structured_limits, max_table_cells),
		FIELD(struct mant_structured_limits, max_fixed_views),
		FIELD(struct mant_structured_limits, max_fixed_lines),
		FIELD(struct mant_structured_limits, max_placements),
		FIELD(struct mant_structured_limits, max_decorations),
		FIELD(struct mant_structured_limits, max_forms),
		FIELD(struct mant_structured_limits, max_name_hints),
		FIELD(struct mant_structured_limits, max_relations),
		FIELD(struct mant_structured_limits, max_connection_atoms),
		FIELD(struct mant_structured_limits, max_annotation_runs),
		FIELD(struct mant_structured_limits, max_annotation_mutations),
		FIELD(struct mant_structured_limits, max_relation_edges),
		FIELD(struct mant_structured_limits, max_diagnostics),
		FIELD(struct mant_structured_limits, max_transfer_objects),
		FIELD(struct mant_structured_limits, max_transfer_edges),
		FIELD(struct mant_structured_limits, max_transfer_bytes),
		FIELD(struct mant_structured_limits, max_nesting_depth),
		FIELD(struct mant_structured_limits, max_include_depth),
		FIELD(struct mant_structured_limits, reserved) };
	static const size_t result[] = {
		FIELD(struct mant_structured_result_view, root_source),
		FIELD(struct mant_structured_result_view, profile),
		FIELD(struct mant_structured_result_view, width),
		FIELD(struct mant_structured_result_view, metadata),
		FIELD(struct mant_structured_result_view, sources),
		FIELD(struct mant_structured_result_view, spans),
		FIELD(struct mant_structured_result_view, provenances),
		FIELD(struct mant_structured_result_view, owners),
		FIELD(struct mant_structured_result_view, content_roots),
		FIELD(struct mant_structured_result_view, content_atoms),
		FIELD(struct mant_structured_result_view, content_refs),
		FIELD(struct mant_structured_result_view, content_points),
		FIELD(struct mant_structured_result_view, links),
		FIELD(struct mant_structured_result_view, blocks),
		FIELD(struct mant_structured_result_view, tables),
		FIELD(struct mant_structured_result_view, table_rows),
		FIELD(struct mant_structured_result_view, table_cells),
		FIELD(struct mant_structured_result_view, fixed_views),
		FIELD(struct mant_structured_result_view, fixed_lines),
		FIELD(struct mant_structured_result_view, placements),
		FIELD(struct mant_structured_result_view, decorations),
		FIELD(struct mant_structured_result_view, forms),
		FIELD(struct mant_structured_result_view, name_hints),
		FIELD(struct mant_structured_result_view, relations),
		FIELD(struct mant_structured_result_view, diagnostics),
		FIELD(struct mant_structured_result_view, reserved) };
	static const size_t source[] = {
		FIELD(struct mant_structured_source_view, key),
		FIELD(struct mant_structured_source_view, identity_kind),
		FIELD(struct mant_structured_source_view, format),
		FIELD(struct mant_structured_source_view, coordinate_kind),
		FIELD(struct mant_structured_source_view, logical_name),
		FIELD(struct mant_structured_source_view, decoded_length),
		FIELD(struct mant_structured_source_view, hash_present),
		FIELD(struct mant_structured_source_view, hash),
		FIELD(struct mant_structured_source_view, reserved_bytes),
		FIELD(struct mant_structured_source_view, reserved) };
	static const size_t span[] = {
		FIELD(struct mant_structured_span_view, line_column_present),
		FIELD(struct mant_structured_span_view, byte_range_present),
		FIELD(struct mant_structured_span_view, reserved_bytes),
		FIELD(struct mant_structured_span_view, source),
		FIELD(struct mant_structured_span_view, line_start),
		FIELD(struct mant_structured_span_view, column_start),
		FIELD(struct mant_structured_span_view, line_end),
		FIELD(struct mant_structured_span_view, column_end),
		FIELD(struct mant_structured_span_view, byte_start),
		FIELD(struct mant_structured_span_view, byte_end),
		FIELD(struct mant_structured_span_view, reserved) };
	static const size_t provenance[] = {
		FIELD(struct mant_structured_provenance_view, kind),
		FIELD(struct mant_structured_provenance_view, authored_span),
		FIELD(struct mant_structured_provenance_view, generated_trigger_span),
		FIELD(struct mant_structured_provenance_view, reserved) };
	static const size_t owner[] = {
		FIELD(struct mant_structured_owner_view, key),
		FIELD(struct mant_structured_owner_view, kind),
		FIELD(struct mant_structured_owner_view, provenance),
		FIELD(struct mant_structured_owner_view, reserved) };
	static const size_t content_root[] = {
		FIELD(struct mant_structured_content_root_view, key),
		FIELD(struct mant_structured_content_root_view, owner),
		FIELD(struct mant_structured_content_root_view, ordinal),
		FIELD(struct mant_structured_content_root_view, kind),
		FIELD(struct mant_structured_content_root_view, provenance),
		FIELD(struct mant_structured_content_root_view, reserved) };
	static const size_t content_atom[] = {
		FIELD(struct mant_structured_content_atom_view, key),
		FIELD(struct mant_structured_content_atom_view, root),
		FIELD(struct mant_structured_content_atom_view, ordinal),
		FIELD(struct mant_structured_content_atom_view, owner),
		FIELD(struct mant_structured_content_atom_view, kind),
		FIELD(struct mant_structured_content_atom_view, style_flags),
		FIELD(struct mant_structured_content_atom_view, role),
		FIELD(struct mant_structured_content_atom_view, link),
		FIELD(struct mant_structured_content_atom_view, text),
		FIELD(struct mant_structured_content_atom_view, display_override_present),
		FIELD(struct mant_structured_content_atom_view, display_reserved_bytes),
		FIELD(struct mant_structured_content_atom_view, display_override),
		FIELD(struct mant_structured_content_atom_view, whitespace_breakable),
		FIELD(struct mant_structured_content_atom_view, reserved_bytes),
		FIELD(struct mant_structured_content_atom_view, provenance),
		FIELD(struct mant_structured_content_atom_view, reserved) };
	static const size_t content_ref[] = {
		FIELD(struct mant_structured_content_ref_view, atom),
		FIELD(struct mant_structured_content_ref_view, byte_start),
		FIELD(struct mant_structured_content_ref_view, byte_end),
		FIELD(struct mant_structured_content_ref_view, reserved) };
	static const size_t content_point[] = {
		FIELD(struct mant_structured_content_point_view, key),
		FIELD(struct mant_structured_content_point_view, root),
		FIELD(struct mant_structured_content_point_view, ordinal),
		FIELD(struct mant_structured_content_point_view, owner),
		FIELD(struct mant_structured_content_point_view, boundary_kind),
		FIELD(struct mant_structured_content_point_view, atom_boundary),
		FIELD(struct mant_structured_content_point_view, atom),
		FIELD(struct mant_structured_content_point_view, byte_offset),
		FIELD(struct mant_structured_content_point_view, scalar_boundary),
		FIELD(struct mant_structured_content_point_view, provenance),
		FIELD(struct mant_structured_content_point_view, reserved) };
	static const size_t link[] = {
		FIELD(struct mant_structured_link_view, key),
		FIELD(struct mant_structured_link_view, owner),
		FIELD(struct mant_structured_link_view, target_kind),
		FIELD(struct mant_structured_link_view, target_a),
		FIELD(struct mant_structured_link_view, target_b_present),
		FIELD(struct mant_structured_link_view, target_b_reserved_bytes),
		FIELD(struct mant_structured_link_view, target_b),
		FIELD(struct mant_structured_link_view, title_present),
		FIELD(struct mant_structured_link_view, title_reserved_bytes),
		FIELD(struct mant_structured_link_view, title),
		FIELD(struct mant_structured_link_view, first_label_ref),
		FIELD(struct mant_structured_link_view, label_ref_count),
		FIELD(struct mant_structured_link_view, provenance),
		FIELD(struct mant_structured_link_view, reserved) };
	static const size_t block[] = {
		FIELD(struct mant_structured_block_view, key),
		FIELD(struct mant_structured_block_view, owner),
		FIELD(struct mant_structured_block_view, kind),
		FIELD(struct mant_structured_block_view, parent),
		FIELD(struct mant_structured_block_view, ordinal),
		FIELD(struct mant_structured_block_view, provenance),
		FIELD(struct mant_structured_block_view, root),
		FIELD(struct mant_structured_block_view, table),
		FIELD(struct mant_structured_block_view, fixed_view),
		FIELD(struct mant_structured_block_view, reserved) };
	static const size_t table[] = {
		FIELD(struct mant_structured_table_view, key),
		FIELD(struct mant_structured_table_view, block),
		FIELD(struct mant_structured_table_view, fixed_view),
		FIELD(struct mant_structured_table_view, provenance),
		FIELD(struct mant_structured_table_view, reserved) };
	static const size_t table_row[] = {
		FIELD(struct mant_structured_table_row_view, key),
		FIELD(struct mant_structured_table_row_view, table),
		FIELD(struct mant_structured_table_row_view, ordinal),
		FIELD(struct mant_structured_table_row_view, provenance),
		FIELD(struct mant_structured_table_row_view, reserved) };
	static const size_t table_cell[] = {
		FIELD(struct mant_structured_table_cell_view, key),
		FIELD(struct mant_structured_table_cell_view, row),
		FIELD(struct mant_structured_table_cell_view, column),
		FIELD(struct mant_structured_table_cell_view, owner),
		FIELD(struct mant_structured_table_cell_view, kind),
		FIELD(struct mant_structured_table_cell_view, alignment),
		FIELD(struct mant_structured_table_cell_view, row_span),
		FIELD(struct mant_structured_table_cell_view, column_span),
		FIELD(struct mant_structured_table_cell_view, provenance),
		FIELD(struct mant_structured_table_cell_view, reserved) };
	static const size_t fixed_view[] = {
		FIELD(struct mant_structured_fixed_view, key),
		FIELD(struct mant_structured_fixed_view, owner),
		FIELD(struct mant_structured_fixed_view, block),
		FIELD(struct mant_structured_fixed_view, table),
		FIELD(struct mant_structured_fixed_view, provenance),
		FIELD(struct mant_structured_fixed_view, reserved) };
	static const size_t fixed_line[] = {
		FIELD(struct mant_structured_fixed_line_view, key),
		FIELD(struct mant_structured_fixed_line_view, view),
		FIELD(struct mant_structured_fixed_line_view, ordinal),
		FIELD(struct mant_structured_fixed_line_view, total_columns),
		FIELD(struct mant_structured_fixed_line_view, reserved) };
	static const size_t placement[] = {
		FIELD(struct mant_structured_placement_view, key),
		FIELD(struct mant_structured_placement_view, line),
		FIELD(struct mant_structured_placement_view, ordinal),
		FIELD(struct mant_structured_placement_view, target_kind),
		FIELD(struct mant_structured_placement_view, atom),
		FIELD(struct mant_structured_placement_view, byte_start),
		FIELD(struct mant_structured_placement_view, byte_end),
		FIELD(struct mant_structured_placement_view, point),
		FIELD(struct mant_structured_placement_view, scalar_start),
		FIELD(struct mant_structured_placement_view, scalar_end),
		FIELD(struct mant_structured_placement_view, column_start),
		FIELD(struct mant_structured_placement_view, column_end),
		FIELD(struct mant_structured_placement_view, cell_map_kind),
		FIELD(struct mant_structured_placement_view, cell_map_value),
		FIELD(struct mant_structured_placement_view, reserved) };
	static const size_t decoration[] = {
		FIELD(struct mant_structured_decoration_view, key),
		FIELD(struct mant_structured_decoration_view, line),
		FIELD(struct mant_structured_decoration_view, ordinal),
		FIELD(struct mant_structured_decoration_view, kind),
		FIELD(struct mant_structured_decoration_view, text),
		FIELD(struct mant_structured_decoration_view, column_start),
		FIELD(struct mant_structured_decoration_view, column_end),
		FIELD(struct mant_structured_decoration_view, provenance),
		FIELD(struct mant_structured_decoration_view, reserved) };
	static const size_t form[] = {
		FIELD(struct mant_structured_form_view, key),
		FIELD(struct mant_structured_form_view, owner),
		FIELD(struct mant_structured_form_view, role),
		FIELD(struct mant_structured_form_view, first_ref),
		FIELD(struct mant_structured_form_view, ref_count),
		FIELD(struct mant_structured_form_view, provenance),
		FIELD(struct mant_structured_form_view, reserved) };
	static const size_t name_hint[] = {
		FIELD(struct mant_structured_name_hint_view, key),
		FIELD(struct mant_structured_name_hint_view, form),
		FIELD(struct mant_structured_name_hint_view, first_ref),
		FIELD(struct mant_structured_name_hint_view, ref_count),
		FIELD(struct mant_structured_name_hint_view, provenance),
		FIELD(struct mant_structured_name_hint_view, reserved) };
	static const size_t relation[] = {
		FIELD(struct mant_structured_relation_view, key),
		FIELD(struct mant_structured_relation_view, owner),
		FIELD(struct mant_structured_relation_view, kind),
		FIELD(struct mant_structured_relation_view, target_owner),
		FIELD(struct mant_structured_relation_view, provenance),
		FIELD(struct mant_structured_relation_view, reserved) };
	static const size_t diagnostic[] = {
		FIELD(struct mant_structured_diagnostic_view, level),
		FIELD(struct mant_structured_diagnostic_view, code),
		FIELD(struct mant_structured_diagnostic_view, message),
		FIELD(struct mant_structured_diagnostic_view, span),
		FIELD(struct mant_structured_diagnostic_view, owner),
		FIELD(struct mant_structured_diagnostic_view, reserved) };
	static const size_t metadata[] = {
		FIELD(struct mant_structured_metadata_view, macroset),
		FIELD(struct mant_structured_metadata_view, presence_flags),
		FIELD(struct mant_structured_metadata_view, title),
		FIELD(struct mant_structured_metadata_view, section),
		FIELD(struct mant_structured_metadata_view, volume),
		FIELD(struct mant_structured_metadata_view, operating_system),
		FIELD(struct mant_structured_metadata_view, architecture),
		FIELD(struct mant_structured_metadata_view, name),
		FIELD(struct mant_structured_metadata_view, date),
		FIELD(struct mant_structured_metadata_view, alias_target),
		FIELD(struct mant_structured_metadata_view, has_body),
		FIELD(struct mant_structured_metadata_view, reserved_bytes),
		FIELD(struct mant_structured_metadata_view, reserved) };

	if (field == 0)
		return SIZE_MAX;
	switch (kind) {
	case MANT_VIEW_INPUT_SOURCE: return PICK(input_source);
	case MANT_VIEW_INPUT: return PICK(input);
	case MANT_VIEW_FAILURE: return PICK(failure);
	case MANT_VIEW_LIMITS: return PICK(limits);
	case MANT_VIEW_RESULT: return PICK(result);
	case MANT_VIEW_SOURCE: return PICK(source);
	case MANT_VIEW_SPAN: return PICK(span);
	case MANT_VIEW_PROVENANCE: return PICK(provenance);
	case MANT_VIEW_OWNER: return PICK(owner);
	case MANT_VIEW_CONTENT_ROOT: return PICK(content_root);
	case MANT_VIEW_CONTENT_ATOM: return PICK(content_atom);
	case MANT_VIEW_CONTENT_REF: return PICK(content_ref);
	case MANT_VIEW_CONTENT_POINT: return PICK(content_point);
	case MANT_VIEW_LINK: return PICK(link);
	case MANT_VIEW_BLOCK: return PICK(block);
	case MANT_VIEW_TABLE: return PICK(table);
	case MANT_VIEW_TABLE_ROW: return PICK(table_row);
	case MANT_VIEW_TABLE_CELL: return PICK(table_cell);
	case MANT_VIEW_FIXED_VIEW: return PICK(fixed_view);
	case MANT_VIEW_FIXED_LINE: return PICK(fixed_line);
	case MANT_VIEW_PLACEMENT: return PICK(placement);
	case MANT_VIEW_DECORATION: return PICK(decoration);
	case MANT_VIEW_FORM: return PICK(form);
	case MANT_VIEW_NAME_HINT: return PICK(name_hint);
	case MANT_VIEW_RELATION: return PICK(relation);
	case MANT_VIEW_DIAGNOSTIC: return PICK(diagnostic);
	case MANT_VIEW_METADATA: return PICK(metadata);
	default: return SIZE_MAX;
	}
}
