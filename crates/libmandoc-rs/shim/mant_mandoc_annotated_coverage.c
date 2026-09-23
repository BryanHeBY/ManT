/* Bounded, explicit R01 coverage evidence; no semantic completeness claim. */
#include "mant_mandoc_annotated_internal.h"
#include "mant_mandoc_structured_session.h"

#include <limits.h>
#include <string.h>

static const uint32_t states[3][8] = {
	/* Native observes nodes and bytes, but R01 does not prove final
	 * semantic ranges, table cells, link targets or anchor points. */
	{ MANT_ANNOTATED_COVERAGE_UNVERIFIED,
	  MANT_ANNOTATED_COVERAGE_UNVERIFIED,
	  MANT_ANNOTATED_COVERAGE_NOT_APPLICABLE,
	  MANT_ANNOTATED_COVERAGE_UNVERIFIED,
	  MANT_ANNOTATED_COVERAGE_UNVERIFIED,
	  MANT_ANNOTATED_COVERAGE_UNVERIFIED,
	  MANT_ANNOTATED_COVERAGE_UNVERIFIED,
	  MANT_ANNOTATED_COVERAGE_UNVERIFIED },
	/* Declaration classification and source-neutral relations belong to
	 * the later codec, not to a hidden second native formatter. */
	{ MANT_ANNOTATED_COVERAGE_NOT_APPLICABLE,
	  MANT_ANNOTATED_COVERAGE_NOT_APPLICABLE,
	  MANT_ANNOTATED_COVERAGE_PENDING,
	  MANT_ANNOTATED_COVERAGE_NOT_APPLICABLE,
	  MANT_ANNOTATED_COVERAGE_NOT_APPLICABLE,
	  MANT_ANNOTATED_COVERAGE_PENDING,
	  MANT_ANNOTATED_COVERAGE_NOT_APPLICABLE,
	  MANT_ANNOTATED_COVERAGE_NOT_APPLICABLE },
	/* Shared validators will close references after owned transfer and
	 * logical reconstruction.  R01 does not run these checks. */
	{ MANT_ANNOTATED_COVERAGE_NOT_APPLICABLE,
	  MANT_ANNOTATED_COVERAGE_NOT_APPLICABLE,
	  MANT_ANNOTATED_COVERAGE_NOT_APPLICABLE,
	  MANT_ANNOTATED_COVERAGE_NOT_APPLICABLE,
	  MANT_ANNOTATED_COVERAGE_NOT_APPLICABLE,
	  MANT_ANNOTATED_COVERAGE_PENDING,
	  MANT_ANNOTATED_COVERAGE_PENDING,
	  MANT_ANNOTATED_COVERAGE_PENDING }
};

static int
append_issue(struct structured_session *session,
    struct mant_annotated_result *result, uint32_t dimension,
    uint32_t reason)
{
	struct mant_annotated_coverage_issue *issues, *issue;
	uint64_t maximum = session->limits->max_transfer_objects;
	uint32_t cap;

	if (!mant_structured_charge(session, &session->builder_operations, 1,
	    session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_CHECK))
		return 0;
	if (maximum > UINT32_MAX)
		maximum = UINT32_MAX;
	cap = (uint32_t)maximum;
	issues = mant_structured_grow_array(session,
	    result->coverage_issues, result->coverage_issue_count,
	    &result->coverage_issue_capacity, cap, sizeof(*issues),
	    session->limits->max_builder_allocated_bytes, 32,
	    MANT_STRUCTURED_STAGE_CHECK);
	if (issues == NULL)
		return 0;
	result->coverage_issues = issues;
	issue = issues + result->coverage_issue_count++;
	memset(issue, 0, sizeof(*issue));
	issue->producer = MANT_ANNOTATED_COVERAGE_NATIVE;
	issue->dimension = dimension;
	issue->reason = reason;
	issue->scope = MANT_ANNOTATED_COVERAGE_DOCUMENT;
	return 1;
}

int
mant_annotated_coverage_is_valid(const struct mant_annotated_result *result)
{
	const struct mant_annotated_coverage_check *check;
	const struct mant_annotated_coverage_issue *issue;
	uint32_t seen[9] = {0};
	uint32_t producer, dimension, index;

	if (result == NULL || result->common == NULL ||
	    (result->coverage_issue_count != 0) !=
	    (result->coverage_issues != NULL) ||
	    result->coverage_issue_count != 7)
		return 0;
	for (producer = 0; producer < 3; producer++)
		for (dimension = 0; dimension < 8; dimension++) {
			check = &result->coverage_checks[producer * 8 + dimension];
			if (check->producer != producer + 1 ||
			    check->dimension != dimension + 1 ||
			    check->state != states[producer][dimension] ||
			    check->reserved != 0)
				return 0;
		}
	for (index = 0; index < result->coverage_issue_count; index++) {
		issue = result->coverage_issues + index;
		if (issue->producer != MANT_ANNOTATED_COVERAGE_NATIVE ||
		    issue->dimension < MANT_ANNOTATED_COVERAGE_SECTION ||
		    issue->dimension > MANT_ANNOTATED_COVERAGE_JOIN ||
		    issue->dimension == MANT_ANNOTATED_COVERAGE_DECLARATION ||
		    issue->reason < MANT_ANNOTATED_COVERAGE_NOT_OBSERVED ||
		    issue->reason > MANT_ANNOTATED_COVERAGE_AMBIGUOUS_SURVIVAL ||
		    issue->scope != MANT_ANNOTATED_COVERAGE_DOCUMENT ||
		    issue->scope_key != 0 || issue->source != 0 ||
		    issue->line != 0 || issue->column != 0)
			return 0;
		seen[issue->dimension] = 1;
	}
	for (dimension = MANT_ANNOTATED_COVERAGE_SECTION;
	    dimension <= MANT_ANNOTATED_COVERAGE_JOIN; dimension++)
		if (dimension != MANT_ANNOTATED_COVERAGE_DECLARATION &&
		    seen[dimension] == 0)
			return 0;
	return 1;
}

int
mant_annotated_coverage_build(struct structured_session *session,
    struct mant_annotated_result *result)
{
	const struct mant_annotated_mark *mark;
	uint32_t seen[9] = {0};
	uint32_t producer, dimension, index;

	if (session == NULL || result == NULL ||
	    session->status != MANT_STRUCTURED_OK ||
	    (result->mark_count != 0 && result->marks == NULL))
		return 0;
	for (producer = 0; producer < 3; producer++)
		for (dimension = 0; dimension < 8; dimension++) {
			struct mant_annotated_coverage_check *check =
			    &result->coverage_checks[producer * 8 + dimension];
			check->producer = producer + 1;
			check->dimension = dimension + 1;
			check->state = states[producer][dimension];
			check->reserved = 0;
		}
	/* Distinguish absent mark kinds without narrowing a global gap to one mark. */
	if (!mant_structured_charge(session, &session->builder_operations,
	    result->mark_count, session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_CHECK))
		return 0;
	for (index = 0; index < result->mark_count; index++) {
		mark = result->marks + index;
		switch (mark->kind) {
		case MANT_ANNOTATED_MARK_HEADING:
			dimension = MANT_ANNOTATED_COVERAGE_SECTION;
			break;
		case MANT_ANNOTATED_MARK_OWNER:
		case MANT_ANNOTATED_MARK_REGION:
			dimension = MANT_ANNOTATED_COVERAGE_OWNER_BOUNDARY;
			break;
		case MANT_ANNOTATED_MARK_LINK:
			dimension = MANT_ANNOTATED_COVERAGE_LINK;
			break;
		case MANT_ANNOTATED_MARK_ANCHOR:
			dimension = MANT_ANNOTATED_COVERAGE_ANCHOR;
			break;
		default:
			mant_structured_set_failure(session, MANT_STRUCTURED_RELATION,
			    MANT_STRUCTURED_STAGE_CHECK, 0, mark->kind, 0);
			return 0;
		}
		seen[dimension] = 1;
	}
	for (dimension = MANT_ANNOTATED_COVERAGE_SECTION;
	    dimension <= MANT_ANNOTATED_COVERAGE_ANCHOR; dimension++) {
		if (dimension == MANT_ANNOTATED_COVERAGE_DECLARATION)
			continue;
		if (!append_issue(session, result, dimension,
		    seen[dimension] != 0 ?
		    MANT_ANNOTATED_COVERAGE_REASON_UNVERIFIED :
		    MANT_ANNOTATED_COVERAGE_NOT_OBSERVED))
			return 0;
	}
	for (dimension = MANT_ANNOTATED_COVERAGE_RELATION;
	    dimension <= MANT_ANNOTATED_COVERAGE_JOIN; dimension++) {
		if (!append_issue(session, result, dimension,
		    dimension == MANT_ANNOTATED_COVERAGE_JOIN ?
		    MANT_ANNOTATED_COVERAGE_NOT_OBSERVED :
		    MANT_ANNOTATED_COVERAGE_REASON_UNVERIFIED))
			return 0;
	}
	return 1;
}
