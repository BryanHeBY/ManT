/* Bounded allocation, accounting, and injected-failure ownership. */
#include "mant_mandoc_structured_session.h"

#include <errno.h>
#include <stdlib.h>
#include <string.h>


void
mant_structured_set_failure(struct structured_session *session, uint32_t status,
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

int
mant_structured_charge(struct structured_session *session, uint64_t *counter,
    uint64_t amount, uint64_t maximum, uint32_t kind, uint32_t stage)
{
	uint64_t observed;

	if (amount <= maximum && *counter <= maximum - amount) {
		*counter += amount;
		return 1;
	}
	observed = amount > UINT64_MAX - *counter ? UINT64_MAX :
	    *counter + amount;
	mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET, stage, kind,
	    observed, maximum);
	return 0;
}

void *
mant_structured_allocate(struct structured_session *session, uint64_t bytes, int zeroed,
    uint32_t stage)
{
	void *pointer;

	if (bytes == 0)
		return NULL;
	if (bytes > SIZE_MAX) {
		mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET, stage, 9,
		    bytes, SIZE_MAX);
		return NULL;
	}
	if (!mant_structured_charge(session, &session->allocated_bytes, bytes,
	    session->limits->max_builder_allocated_bytes, 9, stage))
		return NULL;
	pointer = mant_structured_injected_allocation_failure() ? NULL :
	    (zeroed ? calloc(1, (size_t)bytes) : malloc((size_t)bytes));
	if (pointer == NULL)
		mant_structured_set_failure(session, MANT_STRUCTURED_BUILDER_ALLOC, stage, 0,
		    bytes, session->limits->max_builder_allocated_bytes);
	return pointer;
}

void *
mant_structured_grow_array(struct structured_session *session, void *old, uint32_t count,
    uint32_t *capacity, uint32_t maximum, size_t element_size, uint64_t byte_limit,
    uint32_t limit_kind, uint32_t stage)
{
	void *grown;
	uint32_t new_capacity;
	uint64_t bytes, added_bytes;

	if (session->status != MANT_STRUCTURED_OK)
		return NULL;
	if (count >= maximum) {
		mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET, stage, limit_kind,
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
		mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET, stage,
		    limit_kind, UINT64_MAX,
		    byte_limit);
		return NULL;
	}
	bytes = (uint64_t)new_capacity * element_size;
	if (bytes > byte_limit || bytes > SIZE_MAX) {
		mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET, stage, 9,
		    bytes, byte_limit < SIZE_MAX ? byte_limit : SIZE_MAX);
		return NULL;
	}
	added_bytes = (uint64_t)(new_capacity - *capacity) * element_size;
	if (!mant_structured_charge(session, &session->allocated_bytes, added_bytes,
	    session->limits->max_builder_allocated_bytes, 9,
	    stage))
		return NULL;
	grown = mant_structured_injected_allocation_failure() ? NULL : realloc(old, (size_t)bytes);
	if (grown == NULL) {
		mant_structured_set_failure(session, MANT_STRUCTURED_BUILDER_ALLOC, stage,
		    0, bytes, byte_limit);
		return NULL;
	}
	*capacity = new_capacity;
	return grown;
}

uint8_t *
mant_structured_copy_bytes(struct structured_session *session, const uint8_t *bytes,
    uint64_t length, int content, uint32_t stage)
{
	uint8_t *copy;

	if (session->status != MANT_STRUCTURED_OK)
		return NULL;
	if (length == 0)
		return NULL;
	if (length > SIZE_MAX || (content != 0 &&
	    !mant_structured_charge(session, &session->content_bytes, length,
	    session->limits->max_content_bytes, 10,
	    stage)) ||
	    !mant_structured_charge(session, &session->allocated_bytes, length,
	    session->limits->max_builder_allocated_bytes, 9,
	    stage))
		return NULL;
	copy = mant_structured_injected_allocation_failure() ? NULL : malloc((size_t)length);
	if (copy == NULL) {
		mant_structured_set_failure(session, MANT_STRUCTURED_BUILDER_ALLOC, stage, 0,
		    length,
		    session->limits->max_builder_allocated_bytes);
		return NULL;
	}
	memcpy(copy, bytes, (size_t)length);
	return copy;
}

struct mant_bytes_view
mant_structured_copy_cstring(struct structured_session *session, const char *string)
{
	struct mant_bytes_view view;

	memset(&view, 0, sizeof(view));
	if (string == NULL || *string == '\0')
		return view;
	view.len = strlen(string);
	view.ptr = mant_structured_copy_bytes(session, (const uint8_t *)string, view.len, 1,
	    MANT_STRUCTURED_STAGE_FINALIZE);
	if (view.ptr == NULL)
		view.len = 0;
	return view;
}
