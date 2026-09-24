/* Bounded, per-call normalization of the pinned UTF-8 terminal device. */
#include "mant_mandoc_annotated_display.h"

#include <limits.h>
#include <stdlib.h>
#include <string.h>

struct display_component {
	struct mant_annotated_display_label label;
	struct mant_annotated_display_edge edge;
	uint32_t next;
	uint8_t bytes[4];
	uint8_t length;
};

struct display_slot {
	uint32_t start_plus_one;
	uint32_t head;
	uint32_t tail;
	uint32_t zero_head;
	uint32_t zero_tail;
	uint8_t width;
};

struct mant_annotated_display {
	struct mant_annotated_display_limits limits;
	mant_annotated_display_width width;
	void *width_arg;
	mant_annotated_display_work_charge work_charge;
	void *work_charge_arg;
	struct mant_annotated_display_row *rows;
	struct mant_annotated_display_run *runs;
	struct mant_annotated_run_endpoint *endpoints;
	struct display_slot *slots;
	struct display_component *components;
	uint8_t *bytes;
	size_t row_capacity, run_capacity, endpoint_capacity, slot_capacity;
	size_t component_capacity, byte_capacity;
	uint32_t row_count, run_count, component_count, free_component;
	uint32_t cursor, maxcol;
	uint64_t byte_count, input_bytes, work;
	uint64_t allocated_bytes, peak_allocated_bytes;
	uint8_t pending[4], pending_length, pending_need;
	struct mant_annotated_display_edge pending_edge;
	uint8_t row_touched, finished;
	uint8_t tracking_mode;
	struct mant_annotated_display_label pending_label;
	enum mant_annotated_display_status status;
	uint32_t limit_kind;
	uint64_t observed, allowed;
};

static int
fail(struct mant_annotated_display *display,
    enum mant_annotated_display_status status)
{
	if (display->status == MANT_ANNOTATED_DISPLAY_OK)
		display->status = status;
	return 0;
}

static int
budget(struct mant_annotated_display *display, uint32_t kind,
    uint64_t observed, uint64_t allowed)
{
	if (display->status == MANT_ANNOTATED_DISPLAY_OK) {
		display->limit_kind = kind;
		display->observed = observed;
		display->allowed = allowed;
	}
	return fail(display, MANT_ANNOTATED_DISPLAY_BUDGET);
}

static int
charge(struct mant_annotated_display *display, uint64_t amount)
{
	if (amount > display->limits.max_work - display->work)
		return budget(display, 8,
		    amount > UINT64_MAX - display->work ? UINT64_MAX :
		    display->work + amount, display->limits.max_work);
	if (display->work_charge != NULL &&
	    !display->work_charge(display->work_charge_arg, amount))
		return fail(display, MANT_ANNOTATED_DISPLAY_BUDGET);
	display->work += amount;
	return 1;
}

static int
reserve(struct mant_annotated_display *display, void **data,
    size_t *capacity, size_t wanted, size_t max_count, size_t size,
    uint32_t count_kind)
{
	size_t next, old_bytes, new_bytes;
	void *resized;

	if (wanted <= *capacity)
		return 1;
	if (wanted > max_count || size == 0)
		return budget(display, count_kind, wanted, max_count);
	next = *capacity == 0 ? (max_count < 8 ? max_count : 8) :
	    *capacity;
	while (next < wanted) {
		if (next > max_count / 2) {
			next = max_count;
			break;
		}
		next *= 2;
	}
	if (next < wanted || next > SIZE_MAX / size ||
	    *capacity > SIZE_MAX / size)
		return budget(display, 9, UINT64_MAX,
		    display->limits.max_allocated_bytes);
	old_bytes = *capacity * size;
	new_bytes = next * size;
	if (new_bytes - old_bytes >
	    display->limits.max_allocated_bytes - display->allocated_bytes)
		return budget(display, 9,
		    new_bytes - old_bytes > UINT64_MAX - display->allocated_bytes ?
		    UINT64_MAX : display->allocated_bytes + new_bytes - old_bytes,
		    display->limits.max_allocated_bytes);
	resized = realloc(*data, new_bytes);
	if (resized == NULL)
		return fail(display, MANT_ANNOTATED_DISPLAY_ALLOC);
	*data = resized;
	*capacity = next;
	display->allocated_bytes += new_bytes - old_bytes;
	if (display->peak_allocated_bytes < display->allocated_bytes)
		display->peak_allocated_bytes = display->allocated_bytes;
	return 1;
}

static int
same_label(struct mant_annotated_display_label a,
    struct mant_annotated_display_label b)
{
	return a.owner == b.owner && a.link == b.link &&
	    a.head_component == b.head_component &&
	    a.source == b.source && a.style == b.style && a.role == b.role;
}

static int
same_input_label(struct mant_annotated_display_label a,
    struct mant_annotated_display_label b)
{
	return same_label(a, b) && a.glyph_origin == b.glyph_origin &&
	    a.flags == b.flags && a.reserved == b.reserved;
}

static int
same_origin(struct mant_annotated_display_label a,
    struct mant_annotated_display_label b)
{
	return a.glyph_origin != 0 &&
	    a.glyph_origin == b.glyph_origin &&
	    a.owner == b.owner && a.link == b.link &&
	    a.head_component == b.head_component &&
	    a.source == b.source && a.role == b.role &&
	    ((a.flags | b.flags) & MANT_ANNOTATED_FONT_STROKE) != 0;
}

static int
ensure_slot(struct mant_annotated_display *display, uint32_t column)
{
	size_t old = display->slot_capacity;
	if (!reserve(display, (void **)&display->slots,
	    &display->slot_capacity, (size_t)column + 1,
	    (size_t)display->limits.max_row_columns + 1,
	    sizeof(*display->slots), 10))
		return 0;
	if (old < display->slot_capacity)
		memset(display->slots + old, 0,
		    (display->slot_capacity - old) * sizeof(*display->slots));
	return 1;
}

static uint32_t
component_new(struct mant_annotated_display *display,
    const uint8_t *bytes, uint8_t length,
    struct mant_annotated_display_label label,
    struct mant_annotated_display_edge edge)
{
	struct display_component *component;
	uint32_t key;

	if (display->free_component != 0) {
		key = display->free_component;
		display->free_component =
		    display->components[key - 1].next;
	} else {
		if (display->component_count == UINT32_MAX)
			return budget(display, 29, UINT64_C(1) + UINT32_MAX,
			    UINT32_MAX);
		if (!reserve(display, (void **)&display->components,
		    &display->component_capacity,
		    (size_t)display->component_count + 1,
		    UINT32_MAX, sizeof(*display->components), 29))
			return 0;
		key = ++display->component_count;
	}
	component = display->components + key - 1;
	component->label = label;
	component->edge = edge;
	component->next = 0;
	component->length = length;
	memcpy(component->bytes, bytes, length);
	return key;
}

static void
component_discard(struct mant_annotated_display *display, uint32_t head)
{
	uint32_t next;
	while (head != 0) {
		next = display->components[head - 1].next;
		display->components[head - 1].next = display->free_component;
		display->free_component = head;
		head = next;
	}
}

static void
erase_glyph(struct mant_annotated_display *display, uint32_t start)
{
	struct display_slot *slot = display->slots + start;
	uint32_t end = start + slot->width;
	uint32_t column;

	component_discard(display, slot->head);
	for (column = start; column < end; column++) {
		display->slots[column].start_plus_one = 0;
		display->slots[column].head = 0;
		display->slots[column].tail = 0;
		display->slots[column].width = 0;
	}
}

static int
append_component(struct mant_annotated_display *display,
    uint32_t *head, uint32_t *tail, const uint8_t *bytes,
    uint8_t length, struct mant_annotated_display_label label,
    struct mant_annotated_display_edge edge)
{
	uint32_t key = component_new(display, bytes, length, label, edge);
	if (key == 0)
		return 0;
	if (*tail != 0)
		display->components[*tail - 1].next = key;
	else
		*head = key;
	*tail = key;
	return 1;
}

static int
write_scalar(struct mant_annotated_display *display, uint32_t scalar,
    const uint8_t *bytes, uint8_t length,
    struct mant_annotated_display_label label,
    struct mant_annotated_display_edge edge);

static int
append_piece(struct mant_annotated_display *display, const uint8_t *bytes,
    uint8_t length, uint32_t column, uint32_t width,
    struct mant_annotated_display_label label,
    struct mant_annotated_display_edge edge, uint32_t row_first)
{
	struct mant_annotated_display_run *run;
	struct mant_annotated_run_endpoint *endpoint;
	uint64_t offset = display->byte_count;

	if (length > display->limits.max_result_bytes - offset)
		return budget(display, 10,
		    length > UINT64_MAX - offset ? UINT64_MAX : offset + length,
		    display->limits.max_result_bytes);
	if (!reserve(display, (void **)&display->bytes,
	    &display->byte_capacity, (size_t)(offset + length),
	    (size_t)display->limits.max_result_bytes, 1, 10))
		return 0;
	if (display->run_count > row_first) {
		run = display->runs + display->run_count - 1;
		endpoint = display->endpoints + display->run_count - 1;
		if (same_label(run->label, label) &&
		    run->column + run->width == column &&
		    run->byte_start + run->byte_count == offset &&
		    run->width <= UINT32_MAX - width &&
		    (display->tracking_mode != 2 ||
		    (label.glyph_origin != 0 &&
		    ((edge.same_origin_continuation != 0 &&
		    label.glyph_origin == endpoint->last_origin) ||
		    (edge.join == MANT_DISPLAY_JOIN_DIRECT &&
		    edge.separator_spaces == 0 &&
		    edge.predecessor_origin == endpoint->last_origin))) ||
		    (label.glyph_origin == 0 && endpoint->last_origin == 0 &&
		    edge.predecessor_origin == 0 && edge.join == 0 &&
		    edge.separator_spaces == 0))) {
			memcpy(display->bytes + offset, bytes, length);
			display->byte_count += length;
			run->byte_count += length;
			run->width += width;
			endpoint->last_origin = label.glyph_origin;
			return 1;
		}
	}
	if (display->run_count == display->limits.max_runs)
		return budget(display, 28, (uint64_t)display->run_count + 1,
		    display->limits.max_runs);
	if (!reserve(display, (void **)&display->runs,
	    &display->run_capacity, (size_t)display->run_count + 1,
	    display->limits.max_runs, sizeof(*display->runs), 28))
		return 0;
	if (!reserve(display, (void **)&display->endpoints,
	    &display->endpoint_capacity, (size_t)display->run_count + 1,
	    display->limits.max_runs, sizeof(*display->endpoints), 28))
		return 0;
	run = display->runs + display->run_count;
	endpoint = display->endpoints + display->run_count;
	memset(run, 0, sizeof(*run));
	memset(endpoint, 0, sizeof(*endpoint));
	run->key = ++display->run_count;
	endpoint->first_origin = endpoint->last_origin = label.glyph_origin;
	endpoint->first_edge = edge;
	run->column = column;
	run->width = width;
	run->byte_start = offset;
	run->byte_count = length;
	run->label = label;
	run->label.glyph_origin = 0;
	run->label.flags = 0;
	run->label.reserved = 0;
	memcpy(display->bytes + offset, bytes, length);
	display->byte_count += length;
	return 1;
}

static int
append_chain(struct mant_annotated_display *display, uint32_t head,
    uint32_t column, uint32_t first_width, uint32_t row_first)
{
	const struct display_component *part;
	uint32_t width = first_width;
	while (head != 0) {
		part = display->components + head - 1;
		if (!append_piece(display, part->bytes, part->length,
		    column, width, part->label, part->edge, row_first))
			return 0;
		column += width;
		width = 0;
		head = part->next;
	}
	return 1;
}

static int
finish_row(struct mant_annotated_display *display, int break_after)
{
	struct mant_annotated_display_row *row;
	struct mant_annotated_display_label gap = {0};
	uint32_t column, last, first = display->run_count;
	static const uint8_t space = ' ';

	if (display->row_count == display->limits.max_rows)
		return budget(display, 21, (uint64_t)display->row_count + 1,
		    display->limits.max_rows);
	last = display->maxcol;
	while (last != 0 && display->slots[last - 1].start_plus_one == 0 &&
	    display->slots[last].zero_head == 0)
		last--;
	if (!charge(display, (uint64_t)last + 1) ||
	    !ensure_slot(display, display->maxcol) ||
	    !reserve(display, (void **)&display->rows,
	    &display->row_capacity, (size_t)display->row_count + 1,
	    display->limits.max_rows, sizeof(*display->rows), 21))
		return 0;
	/* A hole in the final cell grid was never emitted by term_field(). */
	gap.role = MANT_ANNOTATED_LAYOUT;
	for (column = 0; column <= last; column++) {
		struct display_slot *slot = display->slots + column;
		if (slot->zero_head != 0 &&
		    !append_chain(display, slot->zero_head, column, 0, first))
			return 0;
		if (column == last)
			break;
		if (slot->start_plus_one == column + 1) {
			if (!append_chain(display, slot->head, column,
			    slot->width, first))
				return 0;
			column += slot->width - 1;
		} else if (slot->start_plus_one == 0 &&
		    !append_piece(display, &space, 1, column, 1, gap,
		    (struct mant_annotated_display_edge){0}, first))
			return 0;
	}
	row = display->rows + display->row_count;
	row->key = ++display->row_count;
	row->first_run = first;
	row->run_count = display->run_count - first;
	row->column_count = last;
	row->break_after = break_after != 0;
	for (column = 0; column <= display->maxcol; column++) {
		struct display_slot *slot = display->slots + column;
		if (slot->start_plus_one == column + 1)
			component_discard(display, slot->head);
		component_discard(display, slot->zero_head);
		memset(slot, 0, sizeof(*slot));
	}
	display->cursor = display->maxcol = display->row_touched = 0;
	return 1;
}

int
mant_annotated_display_trailing_owner(struct mant_annotated_display *display,
    uint32_t *owner)
{
	const struct display_slot *slot;
	const struct display_component *part;
	uint32_t column, key, chain, candidate;
	int found;

	if (display == NULL || owner == NULL || display->finished ||
	    display->status != MANT_ANNOTATED_DISPLAY_OK)
		return 0;
	*owner = 0;
	if (display->slot_capacity == 0)
		return 1;
	for (column = display->maxcol + 1; column != 0;) {
		column--;
		if (!charge(display, 1))
			return 0;
		slot = display->slots + column;
		if (slot->start_plus_one != column + 1 &&
		    slot->zero_head == 0)
			continue;
		candidate = 0;
		found = 0;
		for (chain = 0; chain < 2; chain++) {
			key = chain == 0 ? slot->zero_head : slot->head;
			for (; key != 0; key = part->next) {
				if (!charge(display, 1))
					return 0;
				part = display->components + key - 1;
				if (part->label.role == MANT_ANNOTATED_LAYOUT)
					continue;
				if (found && candidate != part->label.owner)
					return 1;
				candidate = part->label.owner;
				found = 1;
			}
		}
		if (found) {
			*owner = candidate;
			return 1;
		}
	}
	return 1;
}

static int
write_scalar(struct mant_annotated_display *display, uint32_t scalar,
    const uint8_t *bytes, uint8_t length,
    struct mant_annotated_display_label label,
    struct mant_annotated_display_edge edge)
{
	struct display_slot *slot;
	struct display_component *old;
	uint32_t start, column, key;
	size_t width;
	int fold_bold = 0, fold_underline = 0;

	if (!charge(display, 1))
		return 0;
	if (scalar == '\n')
		return finish_row(display, 1);
	if (scalar == '\b') {
		if (display->cursor != 0)
			display->cursor--;
		return 1;
	}
	if (scalar < 0x20 || (scalar >= 0x7f && scalar < 0xa0))
		return fail(display, MANT_ANNOTATED_DISPLAY_UNSAFE_CONTROL);
	width = display->width(display->width_arg, scalar);
	if (width > 2)
		return fail(display, MANT_ANNOTATED_DISPLAY_INVALID);
	if (width == 0) {
		if (!ensure_slot(display, display->cursor))
			return 0;
		if (display->cursor != 0 &&
		    display->slots[display->cursor - 1].start_plus_one != 0) {
			start = display->slots[display->cursor - 1].start_plus_one - 1;
			slot = display->slots + start;
			if (!append_component(display, &slot->head,
			    &slot->tail, bytes, length, label, edge))
				return 0;
		} else {
			slot = display->slots + display->cursor;
			if (!append_component(display, &slot->zero_head,
			    &slot->zero_tail, bytes, length, label, edge))
				return 0;
		}
		display->row_touched = 1;
		return 1;
	}
	if (display->cursor > display->limits.max_row_columns - width)
		return budget(display, 10,
		    (uint64_t)display->cursor + width,
		    display->limits.max_row_columns);
	if (!ensure_slot(display, display->cursor + (uint32_t)width))
		return 0;
	/* A single device backspace moves one column, even after a wide glyph.
	 * Only a proven TERM_COLLECT_FONT stroke may restore the wide glyph's
	 * logical start for font folding; real \z overstrike keeps device order. */
	if (display->cursor != 0 &&
	    display->slots[display->cursor].start_plus_one != 0 &&
	    display->slots[display->cursor].start_plus_one <= display->cursor) {
		uint32_t prior =
		    display->slots[display->cursor].start_plus_one - 1;
		struct display_slot *prior_slot = display->slots + prior;
		struct display_component *prior_part;
		if (prior_slot->head != 0 && prior_slot->width == width) {
			prior_part = display->components + prior_slot->head - 1;
			if (prior_part->next == 0 &&
			    prior_part->length == length &&
			    memcmp(prior_part->bytes, bytes, length) == 0 &&
			    same_origin(prior_part->label, label))
				display->cursor = prior;
		}
	}
	start = display->cursor;
	slot = display->slots + start;
	if (slot->start_plus_one == start + 1 &&
	    slot->head != 0) {
		old = display->components + slot->head - 1;
		if (old->next == 0 && same_origin(old->label, label)) {
			if (slot->width == width && old->length == length &&
			    memcmp(old->bytes, bytes, length) == 0)
				fold_bold = 1;
			else if (slot->width == 1 && old->length == 1 &&
			    old->bytes[0] == '_')
				fold_underline = 1;
			else if (slot->width == width && length == 1 &&
			    bytes[0] == '_') {
				old->label.style |=
				    MANT_ANNOTATED_STYLE_UNDERLINE;
				display->cursor += (uint32_t)width;
				return 1;
			}
		}
	}
	for (column = start; column < start + width; column++) {
		key = display->slots[column].start_plus_one;
		if (key != 0)
			erase_glyph(display, key - 1);
	}
	if (fold_bold)
		label.style |= MANT_ANNOTATED_STYLE_BOLD;
	if (fold_underline)
		label.style |= MANT_ANNOTATED_STYLE_UNDERLINE;
	key = component_new(display, bytes, length, label, edge);
	if (key == 0)
		return 0;
	slot = display->slots + start;
	slot->head = slot->tail = key;
	slot->width = (uint8_t)width;
	for (column = start; column < start + width; column++)
		display->slots[column].start_plus_one = start + 1;
	display->cursor += (uint32_t)width;
	if (display->maxcol < display->cursor)
		display->maxcol = display->cursor;
	display->row_touched = 1;
	return 1;
}

struct mant_annotated_display *
mant_annotated_display_new(const struct mant_annotated_display_limits *limits,
    mant_annotated_display_width width, void *width_arg)
{
	struct mant_annotated_display *display;

	if (limits == NULL || width == NULL || limits->max_input_bytes == 0 ||
	    limits->max_result_bytes == 0 || limits->max_work == 0 ||
	    limits->max_rows == 0 || limits->max_runs == 0 ||
	    limits->max_row_columns < 2 ||
	    limits->max_row_columns == UINT32_MAX ||
	    limits->max_allocated_bytes < sizeof(*display) ||
	    limits->max_result_bytes > SIZE_MAX)
		return NULL;
	display = calloc(1, sizeof(*display));
	if (display == NULL)
		return NULL;
	display->limits = *limits;
	display->width = width;
	display->width_arg = width_arg;
	display->allocated_bytes = sizeof(*display);
	display->peak_allocated_bytes = sizeof(*display);
	return display;
}

int
mant_annotated_display_set_work_charge(struct mant_annotated_display *display,
    mant_annotated_display_work_charge callback, void *argument)
{
	if (display == NULL || callback == NULL ||
	    display->status != MANT_ANNOTATED_DISPLAY_OK ||
	    display->work != 0 || display->finished)
		return 0;
	display->work_charge = callback;
	display->work_charge_arg = argument;
	return 1;
}

static int
write_bytes(struct mant_annotated_display *display,
    const void *data, size_t length, struct mant_annotated_display_label label,
    struct mant_annotated_display_edge edge, uint8_t mode)
{
	const uint8_t *bytes = data;
	uint32_t scalar;
	size_t index;
	uint8_t byte;

	if (display == NULL || display->status != MANT_ANNOTATED_DISPLAY_OK ||
	    display->finished)
		return 0;
	if ((data == NULL && length != 0) || label.role < MANT_ANNOTATED_BODY ||
	    label.role > MANT_ANNOTATED_LAYOUT ||
	    (label.role == MANT_ANNOTATED_LAYOUT &&
	    (label.owner != 0 || label.link != 0 || label.source != 0 ||
	    label.head_component != 0 ||
	    label.glyph_origin != 0 || label.style != 0 || label.flags != 0)) ||
	    (label.flags & ~MANT_ANNOTATED_FONT_STROKE) != 0 ||
	    label.reserved != 0 || edge.reserved != 0 ||
	    edge.same_origin_continuation > 1 ||
	    edge.join > MANT_DISPLAY_JOIN_GENERATED_SEPARATOR ||
	    (edge.join != MANT_DISPLAY_JOIN_SEPARATOR &&
	    edge.join != MANT_DISPLAY_JOIN_GENERATED_SEPARATOR &&
	    (edge.separator_spaces != 0 || edge.separator_owner != 0 ||
	    edge.separator_link != 0)) ||
	    ((edge.join == MANT_DISPLAY_JOIN_SEPARATOR ||
	    edge.join == MANT_DISPLAY_JOIN_GENERATED_SEPARATOR) &&
	    edge.separator_spaces == 0) ||
	    (display->tracking_mode != 0 && display->tracking_mode != mode))
		return fail(display, MANT_ANNOTATED_DISPLAY_INVALID);
	display->tracking_mode = mode;
	if (length > display->limits.max_input_bytes - display->input_bytes)
		return budget(display, 10,
		    length > UINT64_MAX - display->input_bytes ? UINT64_MAX :
		    display->input_bytes + length,
		    display->limits.max_input_bytes);
	if (!charge(display, length))
		return 0;
	display->input_bytes += length;
	if (label.role == MANT_ANNOTATED_HEADER ||
	    label.role == MANT_ANNOTATED_FOOTER) {
		if (display->pending_need != 0)
			return fail(display, MANT_ANNOTATED_DISPLAY_INVALID);
		return 1;
	}
	for (index = 0; index < length; index++) {
		byte = bytes[index];
		if (display->pending_need == 0) {
			if (byte < 0x80) {
				if (!write_scalar(display, byte, &byte, 1,
				    label, edge))
					return 0;
				continue;
			}
			if (byte >= 0xc2 && byte <= 0xdf)
				display->pending_need = 2;
			else if (byte >= 0xe0 && byte <= 0xef)
				display->pending_need = 3;
			else if (byte >= 0xf0 && byte <= 0xf4)
				display->pending_need = 4;
			else
				return fail(display, MANT_ANNOTATED_DISPLAY_INVALID);
			display->pending[0] = byte;
			display->pending_length = 1;
			display->pending_label = label;
			display->pending_edge = edge;
			continue;
		}
		if ((byte & 0xc0) != 0x80 ||
		    !same_input_label(label, display->pending_label) ||
		    edge.predecessor_origin !=
		    display->pending_edge.predecessor_origin ||
		    edge.separator_spaces !=
		    display->pending_edge.separator_spaces ||
		    edge.separator_owner !=
		    display->pending_edge.separator_owner ||
		    edge.separator_link !=
		    display->pending_edge.separator_link ||
		    edge.same_origin_continuation !=
		    display->pending_edge.same_origin_continuation ||
		    edge.join != display->pending_edge.join)
			return fail(display, MANT_ANNOTATED_DISPLAY_INVALID);
		display->pending[display->pending_length++] = byte;
		if (display->pending_length != display->pending_need)
			continue;
		scalar = display->pending[0] &
		    ((1U << (7 - display->pending_need)) - 1);
		for (byte = 1; byte < display->pending_need; byte++)
			scalar = (scalar << 6) |
			    (display->pending[byte] & 0x3f);
		if ((display->pending_need == 2 && scalar < 0x80) ||
		    (display->pending_need == 3 && scalar < 0x800) ||
		    (display->pending_need == 4 && scalar < 0x10000) ||
		    scalar > 0x10ffff ||
		    (scalar >= 0xd800 && scalar <= 0xdfff))
			return fail(display, MANT_ANNOTATED_DISPLAY_INVALID);
		if (!write_scalar(display, scalar, display->pending,
		    display->pending_need, display->pending_label,
		    display->pending_edge))
			return 0;
		display->pending_need = display->pending_length = 0;
	}
	return 1;
}

int
mant_annotated_display_write(struct mant_annotated_display *display,
    const void *data, size_t length, struct mant_annotated_display_label label)
{
	return write_bytes(display, data, length, label,
	    (struct mant_annotated_display_edge){0}, 1);
}

int
mant_annotated_display_write_join(struct mant_annotated_display *display,
    const void *data, size_t length, struct mant_annotated_display_label label,
    struct mant_annotated_display_edge edge)
{
	return write_bytes(display, data, length, label, edge, 2);
}

const struct mant_annotated_run_endpoint *
mant_annotated_display_endpoints(const struct mant_annotated_display *display,
    uint32_t *count)
{
	if (count != NULL)
		*count = display == NULL ? 0 : display->run_count;
	return display == NULL ? NULL : display->endpoints;
}

int
mant_annotated_display_checkpoint(const struct mant_annotated_display *display,
    uint32_t pending_advances, struct mant_annotated_display_checkpoint *point)
{
	if (display == NULL || point == NULL || display->finished ||
	    display->status != MANT_ANNOTATED_DISPLAY_OK ||
	    display->pending_need != 0 ||
	    display->cursor > display->limits.max_row_columns ||
	    pending_advances > display->limits.max_row_columns - display->cursor)
		return 0;
	point->row_before = display->row_count;
	point->column = display->cursor + pending_advances;
	point->active = display->row_touched || point->column != 0;
	return 1;
}

int
mant_annotated_display_finish(struct mant_annotated_display *display,
    struct mant_annotated_display_view *view)
{
	if (view != NULL)
		memset(view, 0, sizeof(*view));
	if (display == NULL || view == NULL ||
	    display->status != MANT_ANNOTATED_DISPLAY_OK)
		return 0;
	if (display->pending_need != 0)
		return fail(display, MANT_ANNOTATED_DISPLAY_INVALID);
	if (!display->finished && display->row_touched &&
	    !finish_row(display, 0))
		return 0;
	display->finished = 1;
	view->bytes = display->bytes;
	view->byte_count = display->byte_count;
	view->rows = display->rows;
	view->row_count = display->row_count;
	view->runs = display->runs;
	view->run_count = display->run_count;
	view->input_bytes = display->input_bytes;
	view->work = display->work;
	view->peak_allocated_bytes = display->peak_allocated_bytes;
	return 1;
}

enum mant_annotated_display_status
mant_annotated_display_status(const struct mant_annotated_display *display)
{
	return display == NULL ? MANT_ANNOTATED_DISPLAY_INVALID :
	    display->status;
}

void
mant_annotated_display_failure(const struct mant_annotated_display *display,
    uint32_t *kind, uint64_t *observed, uint64_t *allowed)
{
	if (kind != NULL)
		*kind = display == NULL ? 0 : display->limit_kind;
	if (observed != NULL)
		*observed = display == NULL ? 0 : display->observed;
	if (allowed != NULL)
		*allowed = display == NULL ? 0 : display->allowed;
}

uint64_t
mant_annotated_display_allocated_bytes(const struct mant_annotated_display *display)
{
	return display == NULL ? 0 : display->allocated_bytes;
}

uint64_t
mant_annotated_display_work(const struct mant_annotated_display *display)
{
	return display == NULL ? 0 : display->work;
}

void
mant_annotated_display_free(struct mant_annotated_display *display)
{
	if (display == NULL)
		return;
	free(display->rows);
	free(display->runs);
	free(display->endpoints);
	free(display->slots);
	free(display->components);
	free(display->bytes);
	free(display);
}
