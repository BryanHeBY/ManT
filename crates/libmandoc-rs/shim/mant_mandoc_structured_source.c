/* Bundle resolution, source coordinates, and sourced diagnostics. */
#include "config.h"

#include <errno.h>
#include <stdarg.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "mandoc.h"
#include "roff.h"
#include "mandoc_parse.h"

#include "mant_mandoc_structured_source.h"

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

int
mant_structured_check_source_positions(struct structured_session *session)
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

int
mant_structured_read_input(struct structured_session *session, struct mparse *parser,
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

void
mant_structured_observe_source_line(void *arg, uint32_t source_key, int line, size_t length)
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

	session = mant_structured_active_session();
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
	if (!mant_structured_read_input(session, parser, slot)) {
		errno = session->status == MANT_STRUCTURED_BUDGET ? EFBIG : ENOMEM;
		return -1;
	}
	return 1;
}

void
mant_structured_observe_diagnostic(void *arg, enum mandocerr code, enum mandoclevel level,
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
int
mant_structured_validate_input(struct structured_session *session)
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
