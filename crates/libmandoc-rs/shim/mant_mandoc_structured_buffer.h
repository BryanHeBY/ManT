/* Private active-token and terminal-collector boundary. */
#ifndef MANT_MANDOC_STRUCTURED_BUFFER_H
#define MANT_MANDOC_STRUCTURED_BUFFER_H

#include "mant_mandoc_structured_session.h"

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

void mant_structured_observe_terminal(struct termp *, void *,
    const struct term_collector_event *);
int mant_structured_buffer_is_settled(const struct structured_session *);
void mant_structured_buffer_release(struct structured_session *,
    const struct mant_structured_result *);

#endif
