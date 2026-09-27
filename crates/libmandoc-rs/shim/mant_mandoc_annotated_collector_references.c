/* Source-evidenced compatible references during one executed terminal word.
 * Pinned term.c::term_word()/encode() report consumed input-byte ranges;
 * buffer.c alone attaches a selected key to active slots and surviving runs.
 * This file never scans final display text to invent a candidate. */
#include "mant_mandoc_annotated_collector_private.h"

static int
ascii_alnum(unsigned char value)
{
	return (value >= 'A' && value <= 'Z') ||
	    (value >= 'a' && value <= 'z') ||
	    (value >= '0' && value <= '9');
}

int
mant_annotated_refs_literal_topic(const char *text, size_t length)
{
	size_t index;

	if (text == NULL || length == 0 || length > 256 ||
	    !ascii_alnum((unsigned char)text[0]))
		return 0;
	for (index = 0; index < length; index++)
		if (!ascii_alnum((unsigned char)text[index]) &&
		    strchr("._+:-", text[index]) == NULL)
			return 0;
	return 1;
}

int
mant_annotated_refs_literal_section(const char *text, size_t length)
{
	size_t index;

	if (text == NULL || length == 0 || length > 16 || text[0] == '0')
		return 0;
	if (text[0] == 'l' || text[0] == 'n')
		return length == 1;
	if (text[0] < '0' || text[0] > '9')
		return 0;
	for (index = 1; index < length; index++)
		if (!ascii_alnum((unsigned char)text[index]))
			return 0;
	return 1;
}

static int
append_candidate(struct mant_annotated_collector *collector,
    const struct roff_node *node, const char *word,
    size_t name_start, const uint8_t *name, size_t name_length,
    size_t section_start, size_t section_end, size_t label_end)
{
	struct annotated_compatible_candidate *grown, *candidate;
	uint32_t maximum, key;

	maximum = collector->session->limits->max_transfer_objects >
	    UINT32_MAX ? UINT32_MAX :
	    (uint32_t)collector->session->limits->max_transfer_objects;
	grown = mant_structured_grow_array(collector->session,
	    collector->compatible_candidates, collector->compatible_count,
	    &collector->compatible_capacity, maximum, sizeof(*grown),
	    collector->session->limits->max_builder_allocated_bytes, 9,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (grown == NULL)
		return 0;
	collector->compatible_candidates = grown;
	key = mant_annotated_marks_add_compatible(collector, node,
	    (const char *)name, name_length,
	    word + section_start, section_end - section_start);
	if (key == 0)
		return 0;
	candidate = grown + collector->compatible_count++;
	candidate->first_byte = name_start;
	candidate->end_byte = label_end;
	candidate->key = key;
	return 1;
}

/* The exact legacy marker is a parser-retained \% followed by visible <>.
 * Target syntax is decoded with the same pinned escape rules as explicit
 * link destinations; glyph membership is proved separately by term_word()
 * events and by the final-surface survival check. */
int
mant_annotated_refs_word_enter(struct mant_annotated_collector *collector,
    const struct term_collector_event *event)
{
	const char *word = event->word;
	const struct roff_node *node = event->node;
	uint8_t decoded[256];
	enum mant_link_target_copy_status status;
	size_t length = event->word_end, index, label_end, open, start;
	size_t decoded_length, scan;
	int overlay;

	collector->compatible_count = 0;
	collector->compatible_cursor = 0;
	if (word == NULL || node == NULL || node->type != ROFFT_TEXT ||
	    (node->flags & (NODE_NOPRT | NODE_NOFILL)) != 0 ||
	    mant_annotated_marks_source_key(node) == 0 ||
	    collector->html_nofill || collector->in_header ||
	    collector->in_footer || collector->in_margin ||
	    (collector->active_link != 0 &&
	    collector->active_link_epoch == collector->phrase_epoch))
		return 1;
	if (!mant_annotated_charge_work(collector, length))
		return 0;
	for (index = 0; index + 4 <= length; index++) {
		if (word[index] != '\\' || word[index + 1] != '%' ||
		    word[index + 2] != '<' || word[index + 3] != '>' ||
		    index < 5 || word[index - 1] != ' ' ||
		    word[index - 2] != ')')
			continue;
		label_end = index - 1;
		open = label_end - 2;
		while (open != 0 && label_end - open <= 18 &&
		    word[open] != '(')
			open--;
		if (word[open] != '(' ||
		    !mant_annotated_refs_literal_section(word + open + 1,
		    label_end - open - 2))
			continue;
		start = open;
		while (start != 0 && open - start <= 1024 &&
		    word[start - 1] != ' ' && word[start - 1] != '\t' &&
		    word[start - 1] != '\n')
			start--;
		if (start == open || open - start > 1024 ||
		    (start >= 2 && word[start - 1] == ' ' &&
		    word[start - 2] == '\\'))
			continue;
		/* Backspace/overstrike provenance is not a one-to-one visible
		 * label, even if font folding happens to leave the same spelling. */
		overlay = 0;
		for (scan = start; scan + 1 < open; scan++)
			if (word[scan] == '\\' &&
			    (word[scan + 1] == 'z' || word[scan + 1] == 'o')) {
				overlay = 1;
				break;
			}
		if (overlay)
			continue;
		/* The main forward scan was charged above.  This bounded reverse
		 * grammar check is charged separately, including a dense marker page. */
		if (!mant_annotated_charge_work(collector,
		    label_end - start))
			return 0;
		status = mant_structured_decode_link_target_slice(
		    collector->session, word + start, open - start,
		    decoded, sizeof(decoded), &decoded_length);
		if (status == MANT_LINK_TARGET_FAILED)
			return 0;
		if (status != MANT_LINK_TARGET_OK ||
		    !mant_annotated_refs_literal_topic((const char *)decoded,
		    decoded_length))
			continue;
		if (!append_candidate(collector, node, word, start,
		    decoded, decoded_length,
		    open + 1, label_end - 1, label_end))
			return 0;
		index += 3;
	}
	return 1;
}

void
mant_annotated_refs_word_leave(struct mant_annotated_collector *collector)
{
	collector->compatible_count = 0;
	collector->compatible_cursor = 0;
}

void
mant_annotated_refs_word_reject(struct mant_annotated_collector *collector)
{
	struct mant_annotated_mark *mark;
	uint32_t index, key;

	for (index = 0; index < collector->compatible_count; index++) {
		key = collector->compatible_candidates[index].key;
		if (key == 0 || key > collector->mark_count)
			continue;
		mark = collector->marks + key - 1;
		free((void *)mark->target_a.ptr);
		free((void *)mark->target_b.ptr);
		memset(&mark->target_a, 0, sizeof(mark->target_a));
		memset(&mark->target_b, 0, sizeof(mark->target_b));
		mark->target_kind = mark->target_b_present = 0;
	}
	collector->link_annotation_rejected = 1;
	mant_annotated_refs_word_leave(collector);
}

uint32_t
mant_annotated_refs_word_link(struct mant_annotated_collector *collector,
    const struct term_collector_event *event)
{
	const struct annotated_compatible_candidate *candidate;

	while (collector->compatible_cursor < collector->compatible_count &&
	    event->word_start >= collector->compatible_candidates[
	    collector->compatible_cursor].end_byte)
		collector->compatible_cursor++;
	if (collector->compatible_cursor == collector->compatible_count)
		return 0;
	candidate = collector->compatible_candidates + collector->compatible_cursor;
	return event->word_start >= candidate->first_byte &&
	    event->word_end <= candidate->end_byte ? candidate->key : 0;
}
