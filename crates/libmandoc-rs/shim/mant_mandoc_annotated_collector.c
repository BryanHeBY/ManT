/* Per-render collector orchestration, accounting and resource release. */
#include "mant_mandoc_annotated_collector_private.h"

void
mant_annotated_fail_relation(struct mant_annotated_collector *collector, uint64_t observed,
    uint64_t allowed)
{
	mant_structured_set_failure(collector->session, MANT_STRUCTURED_RELATION,
	    MANT_STRUCTURED_STAGE_RENDER, 0, observed, allowed);
}

int
mant_annotated_charge_work(struct mant_annotated_collector *collector, uint64_t amount)
{
	struct structured_session *session = collector->session;

	return mant_structured_charge(session, &session->builder_operations,
	    amount, session->limits->max_builder_operations, 8,
	    MANT_STRUCTURED_STAGE_RENDER);
}

int
mant_annotated_charge_mutations(struct mant_annotated_collector *collector, uint64_t amount)
{
	struct structured_session *session = collector->session;

	return mant_structured_charge(session, &session->annotation_mutations,
	    amount, session->limits->max_annotation_mutations, 29,
	    MANT_STRUCTURED_STAGE_RENDER);
}

struct mant_annotated_collector *
mant_annotated_collector_new(struct structured_session *session,
    struct mant_annotated_display *display)
{
	struct mant_annotated_collector *collector;

	if (session == NULL || display == NULL || session->limits == NULL ||
	    session->status != MANT_STRUCTURED_OK)
		return NULL;
	collector = mant_structured_allocate(session, sizeof(*collector), 1,
	    MANT_STRUCTURED_STAGE_RENDER);
	if (collector == NULL)
		return NULL;
	collector->session = session;
	collector->display = display;
	collector->allocated_display_bytes =
	    mant_annotated_display_allocated_bytes(display);
	collector->accounted_display_work = mant_annotated_display_work(display);
	return collector;
}

int
mant_annotated_collector_account_display(
    struct mant_annotated_collector *collector)
{
	struct structured_session *session;
	uint64_t allocated, work, added_allocated;

	if (collector == NULL)
		return 0;
	session = collector->session;
	allocated = mant_annotated_display_allocated_bytes(
	    collector->display);
	work = mant_annotated_display_work(collector->display);
	if (allocated < collector->allocated_display_bytes ||
	    work < collector->accounted_display_work) {
		mant_annotated_fail_relation(collector, allocated, work);
		return 0;
	}
	added_allocated = allocated - collector->allocated_display_bytes;
	collector->allocated_display_bytes = allocated;
	collector->accounted_display_work = work;
	/* The display's work callback charged the shared counter *before* each
	 * normalization step.  This post-write account only covers allocations. */
	return mant_structured_charge(session,
	    &session->allocated_bytes, added_allocated,
	    session->limits->max_builder_allocated_bytes, 9,
	    MANT_STRUCTURED_STAGE_RENDER);
}

void
mant_annotated_collector_get_metrics(
    const struct mant_annotated_collector *collector,
    struct mant_annotated_collector_metrics *metrics)
{
	if (metrics == NULL)
		return;
	memset(metrics, 0, sizeof(*metrics));
	if (collector != NULL)
		*metrics = collector->metrics;
}

int
mant_annotated_collector_link_rejected(
    const struct mant_annotated_collector *collector)
{
	return collector != NULL && collector->link_annotation_rejected != 0;
}

void
mant_annotated_collector_get_marks(
    const struct mant_annotated_collector *collector,
    const struct mant_annotated_mark **marks, uint32_t *count)
{
	if (marks != NULL)
		*marks = collector == NULL ? NULL : collector->marks;
	if (count != NULL)
		*count = collector == NULL ? 0 : collector->mark_count;
}

void
mant_annotated_collector_take_marks(
    struct mant_annotated_collector *collector,
    struct mant_annotated_mark **marks, uint32_t *count)
{
	if (marks != NULL)
		*marks = collector == NULL ? NULL : collector->marks;
	if (count != NULL)
		*count = collector == NULL ? 0 : collector->mark_count;
	if (collector != NULL && marks != NULL && count != NULL) {
		collector->marks = NULL;
		collector->mark_count = collector->mark_capacity = 0;
	}
}

void
mant_annotated_marks_free(struct mant_annotated_mark *marks, uint32_t count)
{
	uint32_t index;

	if (marks == NULL)
		return;
	for (index = 0; index < count; index++)
	{
		free((void *)marks[index].name);
		free((void *)marks[index].target_a.ptr);
		free((void *)marks[index].target_b.ptr);
	}
	free(marks);
}

void
mant_annotated_collector_free(struct mant_annotated_collector *collector)
{
	if (collector == NULL)
		return;
	mant_annotated_buffer_release(collector);
	mant_annotated_marks_free(collector->marks, collector->mark_count);
	free(collector->compatible_candidates);
	free(collector->cells);
	free(collector->frames);
	free(collector);
}
