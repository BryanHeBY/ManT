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
#include "tbl.h"
#include "out.h"
#include "mandoc_parse.h"
#include "main.h"
#include "manconf.h"
#include "term.h"

#include "mant_mandoc_structured_session.h"
#include "mant_mandoc_output.h"


_Static_assert(MANT_DIAGNOSTIC_CODE_NATIVE_LAST == MANDOCERR_MAX,
    "structured diagnostic code range must match pinned mandocerr");

#define MANT_TOKEN_PROJECTION_INLINE 8U

struct structured_token {
	const struct roff_node *node;
	uint32_t provenance;
	uint32_t root;
	uint32_t role;
	uint32_t style;
	uint32_t link;
	int value;
	enum term_collector_reason reason;
	uint32_t projection_length;
	uint32_t projection_capacity;
	uint32_t live_slots;
	uint32_t next_free;
	uint8_t *projection_bytes;
	uint8_t *projection_survived;
	uint8_t projection_inline[MANT_TOKEN_PROJECTION_INLINE];
	uint8_t projection_survived_inline[MANT_TOKEN_PROJECTION_INLINE];
	uint8_t survived;
	uint8_t committed;
	uint8_t active;
};

struct structured_slot {
	uint32_t token;
	uint32_t projection;
};

struct structured_column {
	struct structured_slot *slots;
	uint32_t capacity;
	uint32_t partial_end;
	uint8_t partial_pending;
};


MANT_THREAD_LOCAL struct structured_session *active_session;
MANT_THREAD_LOCAL int structured_active;
MANT_THREAD_LOCAL uint64_t structured_fail_after = UINT64_MAX;
MANT_THREAD_LOCAL uint64_t structured_allocation_count;
MANT_THREAD_LOCAL struct mant_structured_probe_metrics *structured_probe;

void
mant_structured_test_fail_after(uint64_t successful_allocations)
{
	structured_fail_after = successful_allocations;
}

int
mant_structured_injected_allocation_failure(void)
{
	return structured_allocation_count++ >= structured_fail_after;
}

static int safe_logical_name(const uint8_t *, uint64_t);
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
static int supported_tree(const struct roff_node *);
static void observe_terminal(struct termp *, void *,
    const struct term_collector_event *);



int
mant_structured_source_position_in_maps(const struct structured_source_map *maps,
    uint32_t map_count, uint32_t source_key, uint32_t line, uint32_t column)
{
	const struct structured_source_map *map;
	const struct structured_source_line *entry;

	if (maps == NULL || source_key == 0 || source_key > map_count || line == 0)
		return 0;
	map = maps + source_key - 1;
	if (line > map->line_count)
		return 0;
	entry = map->lines + line - 1;
	return entry->present && (uint64_t)column <= entry->length;
}

static int
valid_source_position(struct structured_session *session, uint32_t source_key,
    uint32_t line, uint32_t column)
{
	if (source_key == 0 || source_key > session->input->sources.count ||
	    line == 0)
		return 0;
	if (!mant_structured_charge(session, &session->builder_operations, 1,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_CHECK))
		return 0;
	return mant_structured_source_position_in_maps(session->source_maps,
	    session->input->sources.count, source_key, line, column);
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
		if (span->line_end != 0 && (span->column_end == 0 ||
		    !valid_source_position(session, span->source,
		    span->line_end, span->column_end - 1)))
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


void
mant_structured_clear_failure(struct mant_structured_failure_view *failure)
{
	if (failure != NULL)
		memset(failure, 0, sizeof(*failure));
}


int
mant_structured_valid_bytes(struct mant_bytes_view view)
{
	return (view.len == 0 && view.ptr == NULL) ||
	    (view.len != 0 && view.ptr != NULL);
}

int
mant_structured_valid_utf8(const uint8_t *bytes, uint64_t length)
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

int
mant_structured_valid_identity_name(uint32_t kind, struct mant_bytes_view name)
{
	uint64_t i;

	if (!mant_structured_valid_bytes(name) || name.len == 0 || name.len > SIZE_MAX ||
	    !mant_structured_valid_utf8(name.ptr, name.len))
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
		if (!mant_structured_charge(session, &session->builder_operations, 1,
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
	joined = mant_structured_allocate(session, total, 0, MANT_STRUCTURED_STAGE_RESOLVE);
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
	if (!mant_structured_charge(session, &session->builder_operations, 2,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_RESOLVE))
		return 0;
	if (session->result->source_count >= session->limits->max_sources ||
	    session->result->source_count == UINT32_MAX) {
		mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET,
		    MANT_STRUCTURED_STAGE_RESOLVE, 2,
		    (uint64_t)session->result->source_count + 1,
		    session->limits->max_sources);
		return 0;
	}
	grown = mant_structured_grow_array(session, session->result->sources,
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
	source->logical_name.ptr = mant_structured_copy_bytes(session, input->logical_name.ptr,
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
		mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET,
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
	name = mant_structured_allocate(session, input->logical_name.len + 1, 0,
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
	struct structured_source_map *map;
	uint32_t needed, old_count;
	uint64_t added_bytes;

	if (session == NULL || session->status != MANT_STRUCTURED_OK ||
	    source_key == 0 || source_key > session->result->source_count ||
	    line <= 0)
		return;
	map = session->source_maps + source_key - 1;
	needed = (uint32_t)line;
	if (needed <= map->line_count) {
		entry = map->lines + needed - 1;
		if ((uint64_t)length > entry->length)
			entry->length = length;
		entry->present = 1;
		return;
	}
	old_count = map->line_count;
	added_bytes = (uint64_t)(needed - old_count) * sizeof(*entry);
	if (!mant_structured_charge(session, &session->builder_operations, 1,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_PARSE) ||
	    !mant_structured_charge(session, &session->source_map_entries, needed - old_count,
	    session->limits->max_source_map_entries, 6,
	    MANT_STRUCTURED_STAGE_PARSE) ||
	    !mant_structured_charge(session, &session->source_map_bytes, added_bytes,
	    session->limits->max_source_map_bytes, 7,
	    MANT_STRUCTURED_STAGE_PARSE))
		return;
	grown = mant_structured_grow_array(session, map->lines, needed - 1,
	    &map->line_capacity,
	    UINT32_MAX, sizeof(*grown),
	    session->limits->max_builder_allocated_bytes, 6,
	    MANT_STRUCTURED_STAGE_PARSE);
	if (grown == NULL)
		return;
	map->lines = grown;
	memset(grown + old_count, 0, (needed - old_count) * sizeof(*grown));
	map->line_count = needed;
	entry = grown + needed - 1;
	entry->length = length;
	entry->present = 1;
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
				mant_structured_set_failure(session, MANT_STRUCTURED_INVALID_INPUT,
				    MANT_STRUCTURED_STAGE_RESOLVE, 0,
				    callback_status, MANT_RESOLVE_INVALID);
			else if (callback_status != MANT_RESOLVE_NOT_FOUND)
				mant_structured_set_failure(session, MANT_STRUCTURED_NATIVE,
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
			mant_structured_set_failure(session, MANT_STRUCTURED_INVALID_INPUT,
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
			mant_structured_set_failure(session, MANT_STRUCTURED_INVALID_INPUT,
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
	if (!mant_structured_charge(session, &session->builder_operations, 1,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_PARSE))
		return;
	if (session->result->diagnostic_count >=
	    session->limits->max_diagnostics ||
	    session->result->diagnostic_count == UINT32_MAX) {
		mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET,
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
			mant_structured_set_failure(session, MANT_STRUCTURED_NATIVE,
			    MANT_STRUCTURED_STAGE_PARSE, 0, 0, 0);
			return;
		}
		detail_length = (uint64_t)needed;
	}
	if (base_length > UINT64_MAX - detail_length ||
	    (base_length != 0 && detail_length != 0 &&
	    base_length + detail_length > UINT64_MAX - 2)) {
		mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET,
		    MANT_STRUCTURED_STAGE_PARSE, 10, UINT64_MAX,
		    session->limits->max_content_bytes);
		return;
	}
	message_length = base_length + detail_length +
	    (base_length != 0 && detail_length != 0 ? 2 : 0);
	if (!mant_structured_charge(session, &session->content_bytes, message_length,
	    session->limits->max_content_bytes, 10,
	    MANT_STRUCTURED_STAGE_PARSE) || message_length == UINT64_MAX)
		return;
	message = NULL;
	if (message_length != 0) {
		message = mant_structured_allocate(session, message_length + 1, 0,
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
			mant_structured_set_failure(session, MANT_STRUCTURED_NATIVE,
			    MANT_STRUCTURED_STAGE_PARSE, 0, 0, 0);
			return;
		}
	} else if (message != NULL)
		message[message_length] = '\0';
	has_span = source_key != 0 && source_key <= session->result->source_count &&
	    line > 0 && column >= 0;
	if (has_span) {
		if (!mant_structured_charge(session, &session->builder_operations, 1,
		    session->limits->max_builder_operations, 8,
		    MANT_STRUCTURED_STAGE_PARSE)) {
			free(message);
			return;
		}
		grown_spans = mant_structured_grow_array(session, session->result->spans,
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
	grown_diagnostics = mant_structured_grow_array(session, session->result->diagnostics,
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
		mant_structured_set_failure(session, MANT_STRUCTURED_NATIVE,
		    MANT_STRUCTURED_STAGE_PARSE, 0, 0, 0);
}

static int
copy_metadata(struct structured_session *session, const struct roff_meta *meta)
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
			mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET,
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
		mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET,
		    MANT_STRUCTURED_STAGE_MARSHAL, 1, input->sources.count,
		    session->limits->max_input_sources);
		return 0;
	}
	if (input->sources.count > session->limits->max_source_map_entries) {
		mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET,
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
		    !mant_structured_valid_identity_name(source->identity_kind,
		    source->logical_name) ||
		    !mant_structured_valid_bytes(source->resolver_name) ||
		    !mant_structured_valid_bytes(source->source_bytes) ||
		    source->resolver_name.len > SIZE_MAX ||
		    !mant_structured_valid_utf8(source->resolver_name.ptr,
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
			mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET,
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
		    !mant_structured_charge(session, &session->source_path_bytes,
		    name_bytes,
		    session->limits->max_source_path_bytes, 3,
		    MANT_STRUCTURED_STAGE_MARSHAL) ||
		    !mant_structured_charge(session, &session->decoded_bytes,
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

static int
supported_token(enum roff_tok tok)
{
	switch (tok) {
	case TOKEN_NONE:
	case ROFF_br:
	case ROFF_ll:
	case MDOC_Dd:
	case MDOC_Dt:
	case MDOC_Os:
	case MDOC_Sh:
	case MDOC_Pp:
	case MDOC_Ar:
	case MDOC_Cm:
	case MDOC_Ev:
	case MDOC_Fl:
	case MDOC_Ic:
	case MDOC_Li:
	case MDOC_Nd:
	case MDOC_Nm:
	case MDOC_Pa:
	case MDOC_Xr:
	case MDOC_Em:
	case MDOC_No:
	case MDOC_Ns:
	case MDOC_Pf:
	case MDOC_Sy:
	case MDOC_Lk:
	case MDOC_Mt:
	case MAN_TH:
	case MAN_SH:
	case MAN_LP:
	case MAN_PP:
	case MAN_P:
	case MAN_SM:
	case MAN_SB:
	case MAN_BI:
	case MAN_IB:
	case MAN_BR:
	case MAN_RB:
	case MAN_R:
	case MAN_B:
	case MAN_I:
	case MAN_IR:
	case MAN_RI:
	case MAN_UR:
	case MAN_UE:
	case MAN_MT:
	case MAN_ME:
	case MAN_MR:
		return 1;
	default:
		return 0;
	}
}

static int
supported_tree(const struct roff_node *node)
{
	for (; node != NULL; node = node->next) {
		if ((node->flags & NODE_NOFILL) != 0 ||
		    node->type == ROFFT_TBL || node->type == ROFFT_EQN ||
		    !supported_token(node->tok) || !supported_tree(node->child))
			return 0;
	}
	return 1;
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

static uint32_t
append_provenance(struct structured_session *session,
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

static int
heading_context(const struct roff_node *node)
{
	const struct roff_node *child;

	child = node;
	for (; node != NULL; child = node, node = node->parent)
		if (node->tok == MAN_SH || node->tok == MDOC_Sh) {
			if (node->type == ROFFT_HEAD)
				return 1;
			if (node->type == ROFFT_BODY)
				return 0;
			if (node->type == ROFFT_BLOCK)
				return node->head == child ||
				    child->type == ROFFT_HEAD;
		}
	return 0;
}

static uint32_t
limit_u32(uint64_t value)
{
	return value > UINT32_MAX ? UINT32_MAX : (uint32_t)value;
}

static uint32_t
style_flags(enum termfont font)
{
	switch (font) {
	case TERMFONT_BOLD:
		return MANT_STYLE_BOLD;
	case TERMFONT_UNDER:
		return MANT_STYLE_ITALIC;
	case TERMFONT_BI:
		return MANT_STYLE_BOLD | MANT_STYLE_ITALIC;
	default:
		return 0;
	}
}

static uint32_t
semantic_role(const struct roff_node *node)
{
	for (; node != NULL; node = node->parent)
		switch (node->tok) {
		case MDOC_Fl: return MANT_ROLE_FLAG;
		case MDOC_Ev: return MANT_ROLE_ENVIRONMENT_VARIABLE;
		case MDOC_Ar: return MANT_ROLE_ARGUMENT;
		case MDOC_Cm:
		case MDOC_Ic: return MANT_ROLE_COMMAND_OR_DIRECTIVE;
		case MDOC_Pa: return MANT_ROLE_PATH;
		default: break;
		}
	return 0;
}

static int
open_content_root(struct structured_session *session, int heading,
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
		    limit_u32(session->limits->max_owners), sizeof(*owners),
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
	    limit_u32(session->limits->max_blocks), sizeof(*roots),
	    session->limits->max_builder_allocated_bytes, 12,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (roots == NULL)
		return 0;
	session->result->content_roots = roots;
	blocks = mant_structured_grow_array(session, session->result->blocks,
	    session->result->block_count, &session->result->block_capacity,
	    limit_u32(session->limits->max_blocks), sizeof(*blocks),
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

static int
append_atom(struct structured_session *session, uint32_t root,
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
	    limit_u32(session->limits->max_content_atoms), sizeof(*atoms),
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

static size_t
encode_scalar(int value, uint8_t bytes[4])
{
	uint32_t scalar = (uint32_t)value;

	if (scalar <= 0x7f) {
		bytes[0] = (uint8_t)scalar;
		return 1;
	}
	if (scalar <= 0x7ff) {
		bytes[0] = 0xc0 | (uint8_t)(scalar >> 6);
		bytes[1] = 0x80 | (uint8_t)(scalar & 0x3f);
		return 2;
	}
	if (scalar <= 0xffff && !(scalar >= 0xd800 && scalar <= 0xdfff)) {
		bytes[0] = 0xe0 | (uint8_t)(scalar >> 12);
		bytes[1] = 0x80 | (uint8_t)((scalar >> 6) & 0x3f);
		bytes[2] = 0x80 | (uint8_t)(scalar & 0x3f);
		return 3;
	}
	if (scalar <= 0x10ffff) {
		bytes[0] = 0xf0 | (uint8_t)(scalar >> 18);
		bytes[1] = 0x80 | (uint8_t)((scalar >> 12) & 0x3f);
		bytes[2] = 0x80 | (uint8_t)((scalar >> 6) & 0x3f);
		bytes[3] = 0x80 | (uint8_t)(scalar & 0x3f);
		return 4;
	}
	return 0;
}

static const struct roff_node *
collector_node(const struct structured_session *session,
    const struct term_collector_event *event)
{
	if (event->node != NULL)
		return event->node;
	return session->node_depth == 0 ? NULL :
	    session->node_stack[session->node_depth - 1];
}

static const struct roff_node *
link_node(const struct roff_node *node)
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

static uint32_t
ensure_link(struct structured_session *session, const struct roff_node *node,
    uint32_t owner, uint32_t provenance)
{
	const struct roff_node *canonical, *first, *second;
	struct mant_structured_link_view *links, *link = NULL;
	uint32_t kind;

	canonical = link_node(node);
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
	    limit_u32(session->limits->max_links), sizeof(*links),
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

static void
record_link_ref(struct structured_session *session, uint32_t link_key)
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
	    limit_u32(session->limits->max_content_refs), sizeof(*refs),
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

static void
commit_token(struct structured_session *session, uint32_t key)
{
	struct structured_token *token;
	uint8_t *projection_bytes, *projection_survived;
	uint32_t kind;
	uint8_t bytes[4];
	size_t display_length, index, length;
	int breakable;

	if (key == 0 || key > session->token_slot_count)
		return;
	token = session->tokens + key - 1;
	if (!token->active || token->committed)
		return;
	token->committed = 1;
	if (token->value == ASCII_NBRZW)
		return;
	if (token->value == '\n') {
		if (session->pending_break_root != 0) {
			if (session->probe == NULL)
				mant_structured_set_failure(session, MANT_STRUCTURED_UNSUPPORTED,
				    MANT_STRUCTURED_STAGE_RENDER, 0, 0, 0);
			return;
		}
		session->pending_break_root = token->root;
		session->pending_break_provenance = token->provenance;
		return;
	}
	if (token->value == ASCII_BREAK) {
		append_atom(session, token->root, token->provenance,
		    MANT_ATOM_BREAK_OPPORTUNITY, 0, 0, 0, NULL, 0,
		    NULL, 0, 0);
		return;
	}
	if (token->value == ASCII_HYPH) {
		bytes[0] = '-';
		if (!append_atom(session, token->root, token->provenance,
		    MANT_ATOM_TEXT, token->style, token->role, token->link,
		    bytes, 1, NULL, 0, 0))
			return;
		record_link_ref(session, token->link);
		append_atom(session, token->root, token->provenance,
		    MANT_ATOM_BREAK_OPPORTUNITY, 0, 0, 0, NULL, 0,
		    NULL, 0, 0);
		return;
	}
	if (token->value == ASCII_NBRSP || token->value == 0xa0) {
		static const uint8_t nbsp[] = { 0xc2, 0xa0 };
		static const uint8_t ascii_space[] = { ' ' };

		if (append_atom(session, token->root, token->provenance,
		    MANT_ATOM_WHITESPACE, token->style, token->role, token->link,
		    nbsp, sizeof(nbsp),
		    session->result->profile == MANT_PROFILE_ASCII ? ascii_space :
		    NULL,
		    session->result->profile == MANT_PROFILE_ASCII ?
		    sizeof(ascii_space) : 0, 0))
			record_link_ref(session, token->link);
		return;
	}
	length = encode_scalar(token->value, bytes);
	if (length == 0 || (token->value < 0x20 && token->value != '\t')) {
		if (session->probe == NULL)
			mant_structured_set_failure(session, MANT_STRUCTURED_UNSUPPORTED,
			    MANT_STRUCTURED_STAGE_RENDER, 0,
			    (uint32_t)token->value, 0);
		return;
	}
	kind = token->value == ' ' || token->value == '\t' ?
	    MANT_ATOM_WHITESPACE : MANT_ATOM_TEXT;
	breakable = kind == MANT_ATOM_WHITESPACE &&
	    token->reason != TERM_COLLECT_KEEP_SPACE;
	projection_bytes = token->projection_capacity == 0 ?
	    token->projection_inline : token->projection_bytes;
	projection_survived = token->projection_capacity == 0 ?
	    token->projection_survived_inline : token->projection_survived;
	display_length = 0;
	for (index = 0; index < token->projection_length; index++)
		if (projection_survived[index])
			projection_bytes[display_length++] = projection_bytes[index];
	if (append_atom(session, token->root, token->provenance, kind,
	    token->style, token->role, token->link, bytes, length,
	    display_length == 0 ? NULL : projection_bytes,
	    display_length, breakable))
		record_link_ref(session, token->link);
}

static void
retire_token(struct structured_session *session, uint32_t key)
{
	struct structured_token *token;
	uint64_t projection_bytes;

	if (key == 0 || key > session->token_slot_count ||
	    key == session->pending_token)
		return;
	token = session->tokens + key - 1;
	if (!token->active || token->live_slots != 0)
		return;
	projection_bytes = (uint64_t)token->projection_capacity * 2;
	if (projection_bytes <= session->projection_live_bytes)
		session->projection_live_bytes -= projection_bytes;
	else
		session->projection_live_bytes = 0;
	free(token->projection_bytes);
	free(token->projection_survived);
	token->projection_bytes = NULL;
	token->projection_survived = NULL;
	token->projection_capacity = 0;
	token->projection_length = 0;
	token->active = 0;
	token->next_free = session->free_token;
	session->free_token = key;
}

static void
clear_pending_token(struct structured_session *session)
{
	uint32_t key;

	key = session->pending_token;
	session->pending_token = 0;
	retire_token(session, key);
}

static int
grow_token_projection(struct structured_session *session,
    struct structured_token *token)
{
	uint8_t *bytes, *survived, *old_bytes, *old_survived;
	uint32_t maximum, new_capacity, old_capacity;
	uint64_t live_bytes;

	maximum = limit_u32(session->limits->max_content_bytes);
	old_capacity = token->projection_capacity;
	if (maximum <= token->projection_length) {
		mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET,
		    MANT_STRUCTURED_STAGE_RENDER, 10,
		    (uint64_t)token->projection_length + 1, maximum);
		return 0;
	}
	new_capacity = old_capacity == 0 ?
	    MANT_TOKEN_PROJECTION_INLINE * 2 : old_capacity;
	if (new_capacity > maximum)
		new_capacity = maximum;
	while (new_capacity <= token->projection_length) {
		if (new_capacity > maximum / 2) {
			new_capacity = maximum;
			break;
		}
		new_capacity *= 2;
	}
	if (new_capacity <= token->projection_length) {
		mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET,
		    MANT_STRUCTURED_STAGE_RENDER, 10,
		    (uint64_t)token->projection_length + 1, maximum);
		return 0;
	}
	bytes = mant_structured_allocate(session, new_capacity, 0,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (bytes == NULL)
		return 0;
	survived = mant_structured_allocate(session, new_capacity, 0,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (survived == NULL) {
		free(bytes);
		return 0;
	}
	old_bytes = old_capacity == 0 ? token->projection_inline :
	    token->projection_bytes;
	old_survived = old_capacity == 0 ?
	    token->projection_survived_inline : token->projection_survived;
	memcpy(bytes, old_bytes, token->projection_length);
	memcpy(survived, old_survived, token->projection_length);
	free(token->projection_bytes);
	free(token->projection_survived);
	token->projection_bytes = bytes;
	token->projection_survived = survived;
	token->projection_capacity = new_capacity;
	live_bytes = session->projection_live_bytes - (uint64_t)old_capacity * 2 +
	    (uint64_t)new_capacity * 2;
	session->projection_live_bytes = live_bytes;
	if (session->projection_peak_bytes < live_bytes)
		session->projection_peak_bytes = live_bytes;
	return 1;
}

static uint32_t
record_projection(struct structured_session *session, uint32_t key, int value)
{
	struct structured_token *token;
	uint8_t *bytes, *survived;

	if (key == 0 || key > session->token_slot_count || value < 0 ||
	    value > 0xff || !session->tokens[key - 1].active) {
		mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
		    MANT_STRUCTURED_STAGE_RENDER, 0, key,
		    session->token_slot_count);
		return 0;
	}
	token = session->tokens + key - 1;
	if (token->projection_length == MANT_TOKEN_PROJECTION_INLINE ||
	    (token->projection_capacity != 0 &&
	    token->projection_length == token->projection_capacity)) {
		if (!grow_token_projection(session, token))
			return 0;
	}
	bytes = token->projection_capacity == 0 ? token->projection_inline :
	    token->projection_bytes;
	survived = token->projection_capacity == 0 ?
	    token->projection_survived_inline : token->projection_survived;
	bytes[token->projection_length] = (uint8_t)value;
	survived[token->projection_length] = 0;
	token->projection_length++;
	return token->projection_length;
}

static void
discard_slot(struct structured_session *session, struct structured_slot *slot)
{
	struct structured_token *token;
	uint8_t *survived;
	uint32_t key;

	key = slot->token;
	if (key != 0 && key <= session->token_slot_count &&
	    session->tokens[key - 1].active) {
		token = session->tokens + key - 1;
		survived = token->projection_capacity == 0 ?
		    token->projection_survived_inline :
		    token->projection_survived;
		if (slot->projection != 0 &&
		    slot->projection <= token->projection_length)
			survived[slot->projection - 1] = 0;
		if (token->live_slots != 0)
			token->live_slots--;
		if (token->live_slots == 0 && token->survived)
			commit_token(session, key);
	}
	slot->token = 0;
	slot->projection = 0;
	retire_token(session, key);
}

static void
consume_slot(struct structured_session *session, struct structured_slot *slot)
{
	struct structured_token *token;
	uint8_t *survived;
	uint32_t key;

	key = slot->token;
	if (key == 0 || key > session->token_slot_count ||
	    !session->tokens[key - 1].active) {
		discard_slot(session, slot);
		return;
	}
	token = session->tokens + key - 1;
	token->survived = 1;
	if (slot->projection != 0 &&
	    slot->projection <= token->projection_length) {
		survived = token->projection_capacity == 0 ?
		    token->projection_survived_inline :
		    token->projection_survived;

		survived[slot->projection - 1] = 1;
	}
	if (token->live_slots != 0)
		token->live_slots--;
	slot->token = 0;
	slot->projection = 0;
	if (token->live_slots == 0)
		commit_token(session, key);
	retire_token(session, key);
}

static void
collect_logical(struct structured_session *session,
    const struct term_collector_event *event)
{
	const struct roff_node *node;
	struct structured_token *tokens, *token;
	uint32_t provenance;
	uint32_t key;
	int heading, authored;

	clear_pending_token(session);
	if (session->output_depth != 0) {
		return;
	}
	node = collector_node(session, event);
	heading = heading_context(node);
	authored = event->node != NULL &&
	    (event->reason == TERM_COLLECT_TEXT ||
	    event->reason == TERM_COLLECT_ESCAPE);
	provenance = append_provenance(session, node, authored);
	if (provenance == 0 || session->status != MANT_STRUCTURED_OK)
		return;
	if (session->current_root == 0 ||
	    (session->result->content_roots[session->current_root - 1].kind ==
	    MANT_ROOT_HEADING) != heading) {
		if (!open_content_root(session, heading, provenance))
			return;
	}
	if (!mant_structured_charge(session, &session->annotation_mutations, 1,
	    session->limits->max_annotation_mutations, 29,
	    MANT_STRUCTURED_STAGE_RENDER))
		return;
	if (session->free_token != 0) {
		key = session->free_token;
		token = session->tokens + key - 1;
		session->free_token = token->next_free;
	} else {
		tokens = mant_structured_grow_array(session, session->tokens,
		    session->token_slot_count, &session->token_capacity,
		    limit_u32(session->limits->max_annotation_mutations),
		    sizeof(*tokens), session->limits->max_builder_allocated_bytes,
		    29, MANT_STRUCTURED_STAGE_RENDER);
		if (tokens == NULL)
			return;
		session->tokens = tokens;
		key = ++session->token_slot_count;
		token = tokens + key - 1;
	}
	memset(token, 0, sizeof(*token));
	token->active = 1;
	token->node = node;
	token->provenance = provenance;
	token->root = session->current_root;
	token->role = semantic_role(node);
	token->style = style_flags(event->font);
	if (link_node(node) != NULL &&
	    (event->node == NULL ||
	    (event->node->flags & NODE_DELIMC) == 0)) {
		const struct roff_node *canonical = link_node(node);
		const struct roff_node *third = canonical->child == NULL ? NULL :
		    canonical->child->next == NULL ? NULL :
		    canonical->child->next->next;

		if (!(canonical->tok == MAN_MR && event->node == third))
			token->link = ensure_link(session, node,
			    session->current_owner, provenance);
		if (session->status != MANT_STRUCTURED_OK)
			return;
	}
	token->value = event->value;
	token->reason = event->reason;
	session->pending_token = key;
	if (session->token_total != UINT64_MAX)
		session->token_total++;
}

static struct structured_column *
collector_column(struct structured_session *session, size_t index)
{
	struct structured_column *columns;
	uint32_t needed;

	if (index >= UINT32_MAX) {
		mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET,
		    MANT_STRUCTURED_STAGE_RENDER, 29,
		    index, UINT32_MAX - 1);
		return NULL;
	}
	needed = (uint32_t)index + 1;
	if (needed <= session->column_count)
		return session->columns + index;
	columns = mant_structured_grow_array(session, session->columns, needed - 1,
	    &session->column_capacity, UINT32_MAX, sizeof(*columns),
	    session->limits->max_builder_allocated_bytes, 9,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (columns == NULL)
		return NULL;
	session->columns = columns;
	memset(columns + session->column_count, 0,
	    (needed - session->column_count) * sizeof(*columns));
	session->column_count = needed;
	return columns + index;
}

static int
ensure_slots(struct structured_session *session,
    struct structured_column *column, size_t end)
{
	struct structured_slot *slots;
	uint32_t needed, old_capacity;

	if (end > UINT32_MAX) {
		mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET,
		    MANT_STRUCTURED_STAGE_RENDER, 29, end, UINT32_MAX);
		return 0;
	}
	needed = (uint32_t)end;
	if (needed <= column->capacity)
		return 1;
	old_capacity = column->capacity;
	slots = mant_structured_grow_array(session, column->slots, needed - 1,
	    &column->capacity, UINT32_MAX, sizeof(*slots),
	    session->limits->max_builder_allocated_bytes, 9,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (slots == NULL)
		return 0;
	column->slots = slots;
	memset(slots + old_capacity, 0,
	    (column->capacity - old_capacity) * sizeof(*slots));
	return 1;
}

static void
observe_terminal(struct termp *p, void *arg,
    const struct term_collector_event *event)
{
	struct structured_session *session = arg;
	struct structured_column *column;
	const struct roff_node **stack;
	const struct roff_node *node;
	uint32_t maximum, key;
	size_t index;

	(void)p;
	if (session == NULL || event == NULL ||
	    session->status != MANT_STRUCTURED_OK)
		return;
	if (session->probe != NULL) {
		uint64_t columns;

		if (session->probe->collector_events != UINT64_MAX)
			session->probe->collector_events++;
		columns = event->column == SIZE_MAX ? UINT64_MAX :
		    (uint64_t)event->column + 1;
		if (session->probe->peak_columns < columns)
			session->probe->peak_columns = columns;
		if (session->probe->peak_slots < event->end)
			session->probe->peak_slots = event->end;
		switch (event->op) {
		case TERM_COLLECT_LOGICAL:
			if (session->probe->logical_events != UINT64_MAX)
				session->probe->logical_events++;
			break;
		case TERM_COLLECT_BUFFER_WRITE:
			if (session->probe->buffer_writes != UINT64_MAX)
				session->probe->buffer_writes++;
			break;
		case TERM_COLLECT_BUFFER_CURSOR:
			if (session->probe->cursor_moves != UINT64_MAX)
				session->probe->cursor_moves++;
			break;
		case TERM_COLLECT_BUFFER_TRUNCATE:
			if (session->probe->truncates != UINT64_MAX)
				session->probe->truncates++;
			break;
		case TERM_COLLECT_BUFFER_CONSUME:
			if (session->probe->consumes != UINT64_MAX)
				session->probe->consumes++;
			break;
		case TERM_COLLECT_BUFFER_RESET:
			if (session->probe->resets != UINT64_MAX)
				session->probe->resets++;
			break;
		default:
			break;
		}
	}
	if (!mant_structured_charge(session, &session->builder_operations, 1,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_RENDER))
		return;
	if (event->op == TERM_COLLECT_OUTPUT) {
		if (event->phase == TERM_COLLECT_ENTER)
			session->output_depth++;
		else if (event->phase == TERM_COLLECT_LEAVE &&
		    session->output_depth != 0)
			session->output_depth--;
		return;
	}
	if (event->op == TERM_COLLECT_NODE) {
		if (event->phase == TERM_COLLECT_ENTER) {
			maximum = session->limits->max_nesting_depth > UINT32_MAX ?
			    UINT32_MAX :
			    (uint32_t)session->limits->max_nesting_depth;
			stack = mant_structured_grow_array(session, session->node_stack,
			    session->node_depth, &session->node_capacity, maximum,
			    sizeof(*stack),
			    session->limits->max_builder_allocated_bytes, 35,
			    MANT_STRUCTURED_STAGE_RENDER);
			if (stack == NULL)
				return;
			session->node_stack = stack;
			session->node_stack[session->node_depth++] = event->node;
			if (event->node != NULL && (event->node->tok == MAN_SH ||
			    event->node->tok == MDOC_Sh ||
			    event->node->tok == MAN_PP ||
			    event->node->tok == MAN_LP ||
			    event->node->tok == MAN_P ||
			    event->node->tok == MDOC_Pp)) {
				session->current_root = 0;
				session->current_owner = 0;
				session->current_root_atom_count = 0;
			}
		} else if (event->phase == TERM_COLLECT_LEAVE) {
			if (session->node_depth == 0 ||
			    session->node_stack[session->node_depth - 1] != event->node) {
				mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
				    MANT_STRUCTURED_STAGE_RENDER, 0, 0, 0);
				return;
			}
			session->node_depth--;
		}
		return;
	}
	if (event->op == TERM_COLLECT_LOGICAL) {
		collect_logical(session, event);
		return;
	}
	if (event->op == TERM_COLLECT_BUFFER_WRITE) {
		uint32_t projection;
		struct structured_slot *slot;

		column = collector_column(session, event->column);
		if (column == NULL)
			return;
		if (event->pos >= event->end || event->end - event->pos != 1) {
			mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
			    MANT_STRUCTURED_STAGE_RENDER, 0, event->pos,
			    event->end);
			return;
		}
		if (!ensure_slots(session, column, event->end))
			return;
		if (event->reason == TERM_COLLECT_NORMALIZE)
			return;
		projection = event->reason == TERM_COLLECT_PROJECTION ?
		    record_projection(session, session->pending_token,
		    event->value) : 0;
		if (session->status != MANT_STRUCTURED_OK)
			return;
		if (!mant_structured_charge(session, &session->annotation_mutations, 1,
		    session->limits->max_annotation_mutations, 29,
		    MANT_STRUCTURED_STAGE_RENDER))
			return;
		key = event->reason == TERM_COLLECT_HORIZ ||
		    event->reason == TERM_COLLECT_FIELD ? 0 :
		    session->pending_token;
		slot = column->slots + event->pos;
		discard_slot(session, slot);
		slot->token = key;
		slot->projection = projection;
		if (key != 0)
			session->tokens[key - 1].live_slots++;
		return;
	}
	if (event->op == TERM_COLLECT_DIRECT) {
		commit_token(session, session->pending_token);
		clear_pending_token(session);
		return;
	}
	if (event->op == TERM_COLLECT_BUFFER_TRUNCATE ||
	    event->op == TERM_COLLECT_BUFFER_CONSUME ||
	    event->op == TERM_COLLECT_BUFFER_RESET) {
		column = collector_column(session, event->column);
		if (column == NULL)
			return;
		if (event->op == TERM_COLLECT_BUFFER_TRUNCATE ||
		    event->op == TERM_COLLECT_BUFFER_RESET) {
			column->partial_end = 0;
			column->partial_pending = 0;
		} else if (session->probe != NULL) {
			if (column->partial_pending &&
			    event->pos == column->partial_end) {
				if (session->probe->continued_consumes != UINT64_MAX)
					session->probe->continued_consumes++;
				column->partial_pending = 0;
			}
			if (event->end < p->tcol->lastcol) {
				if (session->probe->partial_consumes != UINT64_MAX)
					session->probe->partial_consumes++;
				column->partial_end = event->end > UINT32_MAX ?
				    UINT32_MAX : (uint32_t)event->end;
				column->partial_pending = 1;
			}
		}
		if (event->end > column->capacity || event->pos > event->end) {
			mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
			    MANT_STRUCTURED_STAGE_RENDER, 0, event->end,
			    column->capacity);
			return;
		}
		if (!mant_structured_charge(session, &session->annotation_mutations,
		    event->end - event->pos,
		    session->limits->max_annotation_mutations, 29,
		    MANT_STRUCTURED_STAGE_RENDER))
			return;
		for (index = event->pos; index < event->end; index++) {
			if (event->op == TERM_COLLECT_BUFFER_CONSUME)
				consume_slot(session, column->slots + index);
			else
				discard_slot(session, column->slots + index);
		}
		if (event->op == TERM_COLLECT_BUFFER_RESET)
			clear_pending_token(session);
		return;
	}
	if (event->op != TERM_COLLECT_ENDLINE)
		return;
	if (session->pending_break_root != 0) {
		if (!append_atom(session, session->pending_break_root,
		    session->pending_break_provenance, MANT_ATOM_HARD_BREAK,
		    0, 0, 0, NULL, 0, NULL, 0, 0) &&
		    session->status == MANT_STRUCTURED_OK)
			mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
			    MANT_STRUCTURED_STAGE_RENDER, 0, 0, 0);
		session->pending_break_root = 0;
		session->pending_break_provenance = 0;
		return;
	}
	node = collector_node(session, event);
	if (node != NULL && node->tok == ROFF_br && session->current_root != 0) {
		uint32_t provenance = append_provenance(session, node, 1);
		if (provenance != 0)
			append_atom(session, session->current_root, provenance,
			    MANT_ATOM_HARD_BREAK, 0, 0, 0, NULL, 0,
			    NULL, 0, 0);
	}
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
	struct manoutput output_options;
	struct mant_mandoc_output *output;
	struct termp *renderer;
	uint64_t source_map_bytes;
	uint32_t status;
	int options, message_state_saved, mchars_ready, output_active;

	if (out_result == NULL || failure == NULL)
		return MANT_STRUCTURED_INVALID_INPUT;
	*out_result = NULL;
	mant_structured_clear_failure(failure);
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
	session.probe = structured_probe;
	session.status = MANT_STRUCTURED_OK;
	result = NULL;
	parser = NULL;
	output = NULL;
	renderer = NULL;
	message_state_saved = 0;
	mchars_ready = 0;
	output_active = 0;
	if (!validate_limits(limits)) {
		mant_structured_set_failure(&session, MANT_STRUCTURED_INVALID_INPUT,
		    MANT_STRUCTURED_STAGE_MARSHAL, 0, 0, 0);
		goto cleanup;
	}
	if (!validate_input(&session)) {
		if (session.status == MANT_STRUCTURED_OK)
			mant_structured_set_failure(&session, MANT_STRUCTURED_INVALID_INPUT,
			    MANT_STRUCTURED_STAGE_MARSHAL, 0, 0, 0);
		goto cleanup;
	}
	result = mant_structured_allocate(&session, sizeof(*result), 1,
	    MANT_STRUCTURED_STAGE_MARSHAL);
	if (result == NULL)
		goto cleanup;
	session.result = result;
	source_map_bytes = (uint64_t)input->sources.count *
	    (sizeof(*session.source_keys) + sizeof(*session.source_maps));
	if (!mant_structured_charge(&session, &session.source_map_entries,
	    input->sources.count, limits->max_source_map_entries, 6,
	    MANT_STRUCTURED_STAGE_MARSHAL) ||
	    !mant_structured_charge(&session, &session.source_map_bytes, source_map_bytes,
	    limits->max_source_map_bytes, 7, MANT_STRUCTURED_STAGE_MARSHAL))
		goto cleanup;
	session.source_keys = mant_structured_allocate(&session,
	    (uint64_t)input->sources.count * sizeof(*session.source_keys), 1,
	    MANT_STRUCTURED_STAGE_MARSHAL);
	if (session.source_keys == NULL)
		goto cleanup;
	session.source_maps = mant_structured_allocate(&session,
	    (uint64_t)input->sources.count * sizeof(*session.source_maps), 1,
	    MANT_STRUCTURED_STAGE_MARSHAL);
	if (session.source_maps == NULL)
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
		mant_structured_set_failure(&session, MANT_STRUCTURED_NATIVE,
		    MANT_STRUCTURED_STAGE_PARSE, 0, 0, 0);
		goto native_cleanup;
	}
	if (!check_nesting_depth(&session, meta->first))
		goto native_cleanup;
	if (session.probe == NULL && !supported_tree(meta->first)) {
		mant_structured_set_failure(&session, MANT_STRUCTURED_UNSUPPORTED,
		    MANT_STRUCTURED_STAGE_RENDER, 0, 1, 0);
		goto native_cleanup;
	}
	result->root_source = session.source_keys[input->root_input - 1];
	result->profile = input->profile;
	result->width = input->width;
	if (!copy_metadata(&session, meta))
		goto native_cleanup;
	if (result->metadata.has_body) {
		output = mant_mandoc_output_alloc(
		    limits->max_content_bytes > SIZE_MAX ? SIZE_MAX :
		    (size_t)limits->max_content_bytes);
		if (output == NULL || !mant_mandoc_output_begin(output)) {
			mant_structured_set_failure(&session, MANT_STRUCTURED_BUILDER_ALLOC,
			    MANT_STRUCTURED_STAGE_RENDER, 0, 0,
			    limits->max_builder_allocated_bytes);
			goto native_cleanup;
		}
		output_active = 1;
		memset(&output_options, 0, sizeof(output_options));
		output_options.width = input->width;
		renderer = input->profile == MANT_PROFILE_ASCII ?
		    ascii_alloc(&output_options) : utf8_alloc(&output_options);
		if (renderer == NULL) {
			mant_structured_set_failure(&session, MANT_STRUCTURED_NATIVE,
			    MANT_STRUCTURED_STAGE_RENDER, 0, 0, 0);
			goto native_cleanup;
		}
		term_setcollector(renderer, observe_terminal, &session);
		if (meta->macroset == MACROSET_MDOC)
			terminal_mdoc(renderer, meta);
		else
			terminal_man(renderer, meta);
		term_setcollector(renderer, NULL, NULL);
		ascii_free(renderer);
		renderer = NULL;
		mant_mandoc_output_end();
		output_active = 0;
		if (mant_mandoc_output_status(output) != 0 &&
		    session.status == MANT_STRUCTURED_OK)
			mant_structured_set_failure(&session, MANT_STRUCTURED_BUDGET,
			    MANT_STRUCTURED_STAGE_RENDER, 10,
			    mant_mandoc_output_length(output),
			    limits->max_content_bytes);
		if (session.probe != NULL)
			session.probe->rendered_bytes =
			    mant_mandoc_output_length(output);
		mant_mandoc_output_free(output);
		output = NULL;
		if (session.probe == NULL && session.pending_break_root != 0 &&
		    session.status == MANT_STRUCTURED_OK)
			mant_structured_set_failure(&session, MANT_STRUCTURED_UNSUPPORTED,
			    MANT_STRUCTURED_STAGE_RENDER, 0, 0, 0);
		if (session.status != MANT_STRUCTURED_OK)
			goto native_cleanup;
	}
	result->magic = MANT_STRUCTURED_MAGIC;

native_cleanup:
	if (renderer != NULL) {
		term_setcollector(renderer, NULL, NULL);
		ascii_free(renderer);
	}
	if (output_active)
		mant_mandoc_output_end();
	mant_mandoc_output_free(output);
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
			mant_structured_set_failure(&session, MANT_STRUCTURED_RELATION,
			    MANT_STRUCTURED_STAGE_CHECK, 0, 0, 0);
		else if (session.status == MANT_STRUCTURED_OK) {
			result->source_maps = session.source_maps;
			result->source_map_count = input->sources.count;
			session.source_maps = NULL;
			if (!mant_structured_result_is_valid(result))
				mant_structured_set_failure(&session, MANT_STRUCTURED_RELATION,
				    MANT_STRUCTURED_STAGE_CHECK, 0, 0, 0);
			else
				result->checked = 1;
		}
	}
	if (session.probe != NULL && session.status == MANT_STRUCTURED_OK)
		mant_structured_set_failure(&session, MANT_STRUCTURED_UNSUPPORTED,
		    MANT_STRUCTURED_STAGE_RENDER, 0, 0, 0);

cleanup:
	if (session.probe != NULL) {
		uint64_t slots = 0, sidecar_bytes;
		uint32_t column;

		for (column = 0; column < session.column_count; column++) {
			if (UINT64_MAX - slots < session.columns[column].capacity) {
				slots = UINT64_MAX;
				break;
			}
			slots += session.columns[column].capacity;
		}
		sidecar_bytes = (uint64_t)session.node_capacity *
		    sizeof(*session.node_stack) +
		    (uint64_t)session.token_capacity * sizeof(*session.tokens) +
		    (uint64_t)session.column_capacity * sizeof(*session.columns) +
		    session.projection_peak_bytes;
		if (slots == UINT64_MAX || slots >
		    (UINT64_MAX - sidecar_bytes) / sizeof(struct structured_slot))
			sidecar_bytes = UINT64_MAX;
		else
			sidecar_bytes += slots * sizeof(struct structured_slot);
		session.probe->builder_allocated_bytes = session.allocated_bytes;
		session.probe->content_bytes = session.content_bytes;
		session.probe->source_count = result == NULL ? 0 :
		    result->source_count;
		session.probe->token_count = session.token_total;
		session.probe->slot_capacity = slots;
		session.probe->sidecar_allocated_bytes = sidecar_bytes;
	}
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
	if (session.source_maps != NULL)
		for (uint32_t source = 0; source < input->sources.count; source++)
			free(session.source_maps[source].lines);
	free(session.source_maps);
	free(session.node_stack);
	for (uint32_t column = 0; column < session.column_count; column++)
		free(session.columns[column].slots);
	free(session.columns);
	for (uint32_t token = 0; token < session.token_slot_count; token++) {
		free(session.tokens[token].projection_bytes);
		free(session.tokens[token].projection_survived);
	}
	free(session.tokens);
	mant_structured_result_free(result);
	structured_fail_after = UINT64_MAX;
	structured_active = 0;
	return status;
}

uint32_t
mant_structured_probe(const struct mant_structured_input_view *input,
    const struct mant_structured_limits *limits,
    struct mant_structured_probe_metrics *metrics,
    struct mant_structured_failure_view *failure)
{
	struct mant_structured_result *result = NULL;
	uint32_t status;

	if (metrics == NULL || failure == NULL)
		return MANT_STRUCTURED_INVALID_INPUT;
	memset(metrics, 0, sizeof(*metrics));
	mant_structured_clear_failure(failure);
	if (structured_active || structured_probe != NULL) {
		failure->status = MANT_STRUCTURED_REENTRANT;
		failure->stage = MANT_STRUCTURED_STAGE_MARSHAL;
		return failure->status;
	}
	structured_probe = metrics;
	status = mant_structured_render(input, limits, &result, failure);
	structured_probe = NULL;
	mant_structured_result_free(result);
	if (status == MANT_STRUCTURED_OK) {
		failure->status = MANT_STRUCTURED_RELATION;
		failure->stage = MANT_STRUCTURED_STAGE_CHECK;
		return failure->status;
	}
	return status;
}
