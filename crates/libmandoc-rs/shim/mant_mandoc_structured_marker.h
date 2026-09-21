/* Private man list-marker evidence scanner. */
#ifndef MANT_MANDOC_STRUCTURED_MARKER_H
#define MANT_MANDOC_STRUCTURED_MARKER_H

#include <stdint.h>

struct roff_node;

enum mant_structured_man_marker_style {
	MANT_STRUCTURED_MAN_MARKER_NONE,
	MANT_STRUCTURED_MAN_MARKER_BULLET,
	MANT_STRUCTURED_MAN_MARKER_DOT,
	MANT_STRUCTURED_MAN_MARKER_PAREN_SUFFIX,
	MANT_STRUCTURED_MAN_MARKER_PAREN_PAIR
};

int mant_structured_man_marker_reaches(const struct roff_node *,
    const struct roff_node *);
int mant_structured_man_named_bullet(const struct roff_node *);
uint32_t mant_structured_man_ordinal_start(const struct roff_node *,
    uint32_t *);

#endif
