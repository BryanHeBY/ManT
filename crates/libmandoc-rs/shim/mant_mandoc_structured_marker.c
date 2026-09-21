/* Read-only man list-marker evidence derived from formatter head ranges. */
#include "config.h"

#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "mandoc.h"
#include "roff.h"

#include "mant_mandoc_structured_marker.h"

#define MAN_MARKER_CANDIDATE_MAX 64U

struct man_marker_candidate {
	char text[MAN_MARKER_CANDIDATE_MAX];
	size_t length;
	uint8_t valid;
	uint8_t pending_space;
};

static int scan_man_marker_node(struct man_marker_candidate *,
    const struct roff_node *, int);

static int
append_man_marker_text(struct man_marker_candidate *candidate,
    const char *text, int separated)
{
	const char *end;
	size_t before;

	if (text == NULL || text[0] == '\0') {
		candidate->valid = 0;
		return 0;
	}
	if (separated && candidate->length != 0)
		candidate->pending_space = 1;
	before = candidate->length;
	while (*text != '\0') {
		if (*text == ' ' || *text == '\t' ||
		    (text[0] == '\\' && text[1] == ' ')) {
			if (candidate->length != 0)
				candidate->pending_space = 1;
			text += *text == '\\' ? 2 : 1;
			continue;
		}
		if (text[0] == '\\' && text[1] == 'f' && text[2] != '\0') {
			if (text[2] == '[') {
				end = strchr(text + 3, ']');
				if (end == NULL) {
					candidate->valid = 0;
					return 0;
				}
				text = end + 1;
			} else
				text += 3;
			continue;
		}
		if (candidate->pending_space != 0 && candidate->length != 0) {
			if (candidate->length == MAN_MARKER_CANDIDATE_MAX - 1) {
				candidate->valid = 0;
				return 0;
			}
			candidate->text[candidate->length++] = ' ';
		}
		candidate->pending_space = 0;
		if (candidate->length == MAN_MARKER_CANDIDATE_MAX - 1) {
			candidate->valid = 0;
			return 0;
		}
		candidate->text[candidate->length++] = *text++;
		candidate->text[candidate->length] = '\0';
	}
	return candidate->length != before;
}

static int
scan_man_marker_sequence(struct man_marker_candidate *candidate,
    const struct roff_node *node, int separated, int concatenate)
{
	int emitted, node_emitted;
	size_t before;

	emitted = 0;
	for (; node != NULL && candidate->valid; node = node->next) {
		before = candidate->length;
		node_emitted = scan_man_marker_node(candidate, node,
		    separated && (emitted || before != 0));
		if (!node_emitted)
			continue;
		emitted = 1;
		separated = !concatenate;
	}
	return emitted;
}

static int
scan_man_marker_node(struct man_marker_candidate *candidate,
    const struct roff_node *node, int separated)
{
	if (node == NULL || !candidate->valid)
		return 0;
	switch (node->type) {
	case ROFFT_TEXT:
		return append_man_marker_text(candidate, node->string, separated);
	case ROFFT_COMMENT:
		return 0;
	case ROFFT_ELEM:
		break;
	default:
		candidate->valid = 0;
		return 0;
	}
	switch (node->tok) {
	case MAN_BI:
	case MAN_IB:
	case MAN_BR:
	case MAN_RB:
	case MAN_IR:
	case MAN_RI:
		return scan_man_marker_sequence(candidate, node->child,
		    separated, 1);
	case MAN_SM:
	case MAN_SB:
	case MAN_R:
	case MAN_B:
	case MAN_I:
		return scan_man_marker_sequence(candidate, node->child,
		    separated, 0);
	default:
		candidate->valid = 0;
		return 0;
	}
}

/*
 * Mirror the effective head ranges in man_term.c::pre_IP/pre_TP.  Marker
 * lowering is allowed only when every formatter-executed node in that range
 * is a supported font wrapper and the complete visible text is a marker.
 */
static int
man_marker_candidate(const struct roff_node *node,
    struct man_marker_candidate *candidate)
{
	const struct roff_node *head;

	memset(candidate, 0, sizeof(*candidate));
	candidate->valid = 1;
	head = node == NULL ? NULL : node->head;
	if (head == NULL || head->child == NULL)
		return 0;
	head = head->child;
	if (node->tok == MAN_IP)
		scan_man_marker_node(candidate, head, 0);
	else if (node->tok == MAN_TP) {
		while (head != NULL && (head->flags & NODE_LINE) == 0)
			head = head->next;
		scan_man_marker_sequence(candidate, head, 0, 0);
	} else
		return 0;
	return candidate->valid && candidate->length != 0;
}

/*
 * man_macro.c::blk_exp closes an implicit IP/TP before opening RS, leaving
 * the relative-indent block as an AST sibling.  The formatter nevertheless
 * renders consecutive RS siblings as content of the preceding visible item.
 */
int
mant_structured_man_marker_reaches(const struct roff_node *marker,
    const struct roff_node *node)
{
	if (marker == NULL || node == NULL)
		return 0;
	for (marker = marker->next; marker != NULL && marker != node;
	    marker = marker->next)
		if (marker->type != ROFFT_BLOCK || marker->tok != MAN_RS)
			return 0;
	return marker == node;
}

static const char *
skip_marker_decoration(const char *text)
{
	for (;;) {
		while (*text == ' ' || *text == '\t')
			text++;
		if (text[0] == '\\' && text[1] == ' ') {
			text += 2;
			continue;
		}
		if (text[0] == '\\' && text[1] == 'f' && text[2] != '\0') {
			if (text[2] == '[') {
				const char *end = strchr(text + 3, ']');
				if (end == NULL)
					return text;
				text = end + 1;
			} else
				text += 3;
			continue;
		}
		return text;
	}
}

int
mant_structured_man_named_bullet(const struct roff_node *node)
{
	struct man_marker_candidate candidate;
	const char *text;

	if (!man_marker_candidate(node, &candidate))
		return 0;
	text = skip_marker_decoration(candidate.text);
	if (strncmp(text, "\\(bu", 4) == 0)
		text += 4;
	else if (strncmp(text, "\\[bu]", 5) == 0)
		text += 5;
	else
		return 0;
	return *skip_marker_decoration(text) == '\0';
}

uint32_t
mant_structured_man_ordinal_start(const struct roff_node *node,
    uint32_t *style)
{
	struct man_marker_candidate candidate;
	const char *text;
	uint64_t value;
	int parenthesized;

	if (!man_marker_candidate(node, &candidate))
		return 0;
	text = skip_marker_decoration(candidate.text);
	parenthesized = *text == '(';
	if (parenthesized)
		text++;
	if (*text < '0' || *text > '9')
		return 0;
	value = 0;
	while (*text >= '0' && *text <= '9') {
		value = value * 10 + (unsigned int)(*text++ - '0');
		if (value > UINT32_MAX)
			return 0;
	}
	if (parenthesized) {
		if (*text++ != ')')
			return 0;
		*style = MANT_STRUCTURED_MAN_MARKER_PAREN_PAIR;
	} else if (*text == '.') {
		*style = MANT_STRUCTURED_MAN_MARKER_DOT;
		text++;
	} else if (*text == ')') {
		*style = MANT_STRUCTURED_MAN_MARKER_PAREN_SUFFIX;
		text++;
	} else
		return 0;
	return *skip_marker_decoration(text) == '\0' && value != 0 ?
	    (uint32_t)value : 0;
}
