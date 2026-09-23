/* Structured terminal-event collector and reclaimable active buffer. */
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

#include "mant_mandoc_structured_source.h"
#include "mant_mandoc_structured_address.h"
#include "mant_mandoc_structured_buffer.h"
#include "mant_mandoc_structured_builder.h"
#include "mant_mandoc_structured_link.h"
#include "mant_mandoc_structured_structure.h"
#include "mant_mandoc_output.h"


_Static_assert(MANT_DIAGNOSTIC_CODE_NATIVE_LAST == MANDOCERR_MAX,
    "structured diagnostic code range must match pinned mandocerr");










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
		session->pending_break_link = token->link;
		session->pending_break_sequence = token->sequence;
		return;
	}
	if (token->value == ASCII_BREAK) {
		mant_structured_address_before_atom(session, token->root,
		    token->sequence);
		mant_structured_append_atom(session, token->root, token->provenance,
		    MANT_ATOM_BREAK_OPPORTUNITY, 0, 0, 0, NULL, 0,
		    NULL, 0, 0);
		return;
	}
	if (token->value == ASCII_HYPH) {
		bytes[0] = '-';
		mant_structured_address_before_atom(session, token->root,
		    token->sequence);
		if (!mant_structured_append_atom(session, token->root, token->provenance,
		    MANT_ATOM_TEXT, token->style, token->role, token->link,
		    bytes, 1, NULL, 0, 0))
			return;
		mant_structured_append_atom(session, token->root, token->provenance,
		    MANT_ATOM_BREAK_OPPORTUNITY, 0, 0, 0, NULL, 0,
		    NULL, 0, 0);
		return;
	}
	if (token->value == ASCII_NBRSP || token->value == 0xa0) {
		static const uint8_t nbsp[] = { 0xc2, 0xa0 };
		static const uint8_t ascii_space[] = { ' ' };

		mant_structured_address_before_atom(session, token->root,
		    token->sequence);
		mant_structured_append_atom(session, token->root, token->provenance,
		    MANT_ATOM_WHITESPACE, token->style, token->role, token->link,
		    nbsp, sizeof(nbsp),
		    session->result->profile == MANT_PROFILE_ASCII ? ascii_space :
		    NULL,
		    session->result->profile == MANT_PROFILE_ASCII ?
		    sizeof(ascii_space) : 0, 0);
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
	mant_structured_address_before_atom(session, token->root, token->sequence);
	mant_structured_append_atom(session, token->root, token->provenance, kind,
	    token->style, token->role, token->link, bytes, length,
	    display_length == 0 ? NULL : projection_bytes,
	    display_length, breakable);
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

	maximum = mant_structured_limit_u32(session->limits->max_content_bytes);
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
	    (event->node->flags & NODE_NOSRC) == 0 &&
	    (event->reason == TERM_COLLECT_TEXT ||
	    event->reason == TERM_COLLECT_ESCAPE);
	provenance = mant_structured_append_provenance(session, node, authored);
	if (provenance == 0 || session->status != MANT_STRUCTURED_OK)
		return;
	if (session->current_root == 0 ||
	    (session->result->content_roots[session->current_root - 1].kind ==
	    MANT_ROOT_HEADING) != heading) {
		if (!mant_structured_open_content_root(session, heading, provenance))
			return;
		mant_structured_address_root_opened(session, node, heading);
		if (session->status != MANT_STRUCTURED_OK)
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
		    mant_structured_limit_u32(
		    session->limits->max_annotation_mutations),
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
	if (session->token_total != UINT64_MAX)
		session->token_total++;
	token->sequence = session->token_total;
	token->node = node;
	token->provenance = provenance;
	token->root = session->current_root;
	token->role = semantic_role(node);
	token->style = style_flags(event->font);
	if (mant_structured_link_node(node) != NULL) {
		const struct roff_node *canonical = mant_structured_link_node(node);
		const struct roff_node *third = canonical->child == NULL ? NULL :
		    canonical->child->next == NULL ? NULL :
		    canonical->child->next->next;
		int outside_inner;

		/* mdoc_html.c::mdoc_mt_pre gives each address its own link;
		 * term.c::term_word inserts auto-space before each child, outside
		 * that address's label.  The authored child remains the identity
		 * across any buffer consumption or terminal wrap. */
		outside_inner = (event->reason == TERM_COLLECT_AUTO_SPACE &&
		    mant_structured_existing_link(session, node) == 0) ||
		    (event->node != NULL &&
		    (event->node->flags & NODE_DELIMC) != 0) ||
		    (canonical->tok == MDOC_Mt &&
		    (event->node == NULL ||
		    event->reason == TERM_COLLECT_AUTO_SPACE)) ||
		    (canonical->tok == MAN_MR && third != NULL &&
		    event->node == third);
		if (outside_inner)
			canonical = mant_structured_link_node(canonical->parent);
		if (canonical != NULL)
			token->link = mant_structured_ensure_link(session,
			    outside_inner ? canonical : node,
			    session->current_owner);
		if (session->status != MANT_STRUCTURED_OK)
			return;
	}
	token->value = event->value;
	token->reason = event->reason;
	session->pending_token = key;
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

void
mant_structured_observe_terminal(struct termp *p, void *arg,
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
			if (!mant_structured_enter_node(session, event->node))
				return;
			if (event->node != NULL && (event->node->tok == MAN_SH ||
			    event->node->tok == MDOC_Sh ||
			    event->node->tok == MAN_PP ||
			    event->node->tok == MAN_LP ||
			    event->node->tok == MAN_P ||
			    event->node->tok == MDOC_Pp)) {
				session->current_root = 0;
				session->current_owner = 0;
			}
			mant_structured_address_enter_node(session, event->node);
		} else if (event->phase == TERM_COLLECT_LEAVE) {
			if (session->node_depth == 0 ||
			    session->node_stack[session->node_depth - 1] != event->node) {
				mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
				    MANT_STRUCTURED_STAGE_RENDER, 0, 0, 0);
				return;
			}
			mant_structured_leave_node(session, event->node);
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
		uint32_t link = session->pending_break_link;

		mant_structured_address_before_atom(session,
		    session->pending_break_root, session->pending_break_sequence);
		if (!mant_structured_append_atom(session, session->pending_break_root,
		    session->pending_break_provenance, MANT_ATOM_HARD_BREAK,
		    0, 0, link, NULL, 0, NULL, 0, 0) &&
		    session->status == MANT_STRUCTURED_OK)
			mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
			    MANT_STRUCTURED_STAGE_RENDER, 0, 0, 0);
		session->pending_break_root = 0;
		session->pending_break_provenance = 0;
		session->pending_break_link = 0;
		session->pending_break_sequence = 0;
		return;
	}
	node = collector_node(session, event);
	if (node != NULL && node->tok == ROFF_br && session->current_root != 0) {
		uint32_t provenance = mant_structured_append_provenance(session, node, 1);
		uint32_t link = 0;

		if (provenance != 0 && mant_structured_link_node(node) != NULL)
			link = mant_structured_ensure_link(session, node,
			    session->current_owner);
		if (provenance != 0) {
			mant_structured_address_before_atom(session,
			    session->current_root,
			    session->token_total == UINT64_MAX ? UINT64_MAX :
			    session->token_total + 1);
			mant_structured_append_atom(session, session->current_root,
			    provenance, MANT_ATOM_HARD_BREAK, 0, 0, link, NULL, 0,
			    NULL, 0, 0);
		}
	}
}

int
mant_structured_buffer_is_settled(const struct structured_session *session)
{
	const struct structured_column *column;
	uint32_t column_index, slot, token;

	if (session->node_depth != 0 || session->output_depth != 0 ||
	    session->pending_token != 0 || session->pending_break_root != 0)
		return 0;
	for (token = 0; token < session->token_slot_count; token++)
		if (session->tokens[token].active != 0)
			return 0;
	for (column_index = 0; column_index < session->column_count;
	    column_index++) {
		column = session->columns + column_index;
		if (column->partial_pending != 0)
			return 0;
		for (slot = 0; slot < column->capacity; slot++)
			if (column->slots[slot].token != 0 ||
			    column->slots[slot].projection != 0)
				return 0;
	}
	return 1;
}

void
mant_structured_buffer_release(struct structured_session *session,
    const struct mant_structured_result *result)
{
	uint64_t slots = 0, sidecar_bytes;
	uint32_t column, token;

	if (session->probe != NULL) {
		for (column = 0; column < session->column_count; column++) {
			if (UINT64_MAX - slots < session->columns[column].capacity) {
				slots = UINT64_MAX;
				break;
			}
			slots += session->columns[column].capacity;
		}
		sidecar_bytes = (uint64_t)session->node_capacity *
		    sizeof(*session->node_stack) +
		    (uint64_t)session->link_identity_capacity *
		    sizeof(*session->link_identities) +
		    (uint64_t)session->token_capacity * sizeof(*session->tokens) +
		    (uint64_t)session->column_capacity * sizeof(*session->columns) +
		    session->projection_peak_bytes;
		if (slots == UINT64_MAX || slots >
		    (UINT64_MAX - sidecar_bytes) / sizeof(struct structured_slot))
			sidecar_bytes = UINT64_MAX;
		else
			sidecar_bytes += slots * sizeof(struct structured_slot);
		session->probe->builder_allocated_bytes = session->allocated_bytes;
		session->probe->content_bytes = session->content_bytes;
		session->probe->source_count = result == NULL ? 0 :
		    result->source_count;
		session->probe->token_count = session->token_total;
		session->probe->slot_capacity = slots;
		session->probe->sidecar_allocated_bytes = sidecar_bytes;
	}
	for (column = 0; column < session->column_count; column++)
		free(session->columns[column].slots);
	free(session->columns);
	for (token = 0; token < session->token_slot_count; token++) {
		free(session->tokens[token].projection_bytes);
		free(session->tokens[token].projection_survived);
	}
	free(session->tokens);
}
