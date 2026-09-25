/* Private structured link construction boundary. */
#ifndef MANT_MANDOC_STRUCTURED_LINK_H
#define MANT_MANDOC_STRUCTURED_LINK_H

#include "mant_mandoc_structured_session.h"

struct roff_node;
struct mant_bytes_view;

/* Optional annotated destinations may reject only an unsupported escape.
 * Invalid UTF-8, structural input and allocation/budget failures remain
 * hard errors even when the displayed label itself is safe. */
enum mant_link_target_copy_status {
	MANT_LINK_TARGET_UNSUPPORTED = 0,
	MANT_LINK_TARGET_OK = 1,
	MANT_LINK_TARGET_INVALID = 2,
	MANT_LINK_TARGET_FAILED = 3
};

const struct roff_node *mant_structured_link_node(const struct roff_node *);
uint32_t mant_structured_existing_link(const struct structured_session *,
    const struct roff_node *);
uint32_t mant_structured_ensure_link(struct structured_session *,
    const struct roff_node *, uint32_t);
int mant_structured_finalize_links(struct structured_session *);
/* Shared, parser-alive target decoding; follows pinned html.c::print_encode. */
int mant_structured_copy_link_target(struct structured_session *,
    struct mant_bytes_view *, const struct roff_node *);
int mant_structured_copy_link_target_allow_empty(struct structured_session *,
    struct mant_bytes_view *, const struct roff_node *);
enum mant_link_target_copy_status
mant_structured_copy_link_target_allow_empty_classified(
    struct structured_session *, struct mant_bytes_view *,
    const struct roff_node *);
int mant_structured_copy_deroff_target(struct structured_session *,
    struct mant_bytes_view *, const struct roff_node *);
/* A visible .Sx macro can have no deroff() destination.  Its caller keeps
 * the occurrence with no href, as pinned html.c::html_make_id() does. */
int mant_structured_copy_deroff_target_allow_empty(struct structured_session *,
    struct mant_bytes_view *, const struct roff_node *);

#endif
