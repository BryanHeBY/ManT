/* Standalone result-level C regression.  After `cargo build --locked -p
 * libmandoc-rs --features annotated`, compile against that build's native
 * archive (not an archive from another feature combination):
 *
 * cc -std=c11 -Wall -Wextra -Werror -pedantic \
 *   -Icrates/libmandoc-rs/shim \
 *   crates/libmandoc-rs/tests/annotated_result_isolation.c \
 *   "$MANT_NATIVE_ARCHIVE" -lm -lz \
 *   -o target/annotated-result-isolation-test
 * target/annotated-result-isolation-test
 *
 * This test uses the C-owned result returned by the actual session.  It
 * mutates only the test's handle after rendering; no production fault hook
 * or second formatter is installed.
 */
#include "mant_mandoc_annotated_internal.h"
#include "mant_mandoc_structured_session.h"

#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* The production archive normally resolves this callback in Rust.  This
 * standalone ASCII-only fixture provides the same width for every scalar it
 * can emit, and fails if the fixture unexpectedly grows beyond that scope. */
size_t
mant_mandoc_utf8_width(int scalar)
{
	assert(scalar >= 0 && scalar < 128);
	return scalar >= 0x20 && scalar <= 0x7e ? 1 : 0;
}

/* The exact input below was run first with the pinned CVS mandoc using
 * `-Tutf8 -O width=78`.  man_term.c::pre_TP/post_TP keep the label and body
 * distinct; pre_UR/post_UR emit the link's label and visible destination.
 * No assertion here reconstructs or changes that upstream display output. */
static const uint8_t roff[] =
    ".TH T 1\n"
    ".SH DESCRIPTION\n"
    ".TP\n"
    ".B --foo\n"
    "Description.\n"
    ".UR https://example.test\n"
    "link\n"
    ".UE\n";
static const uint8_t source_name[] = "t.1";
static const struct mant_input_source_view source = {
    .identity_kind = MANT_IDENTITY_BUNDLE_MEMBER,
    .format = MANT_FORMAT_MAN,
    .logical_name = {source_name, sizeof(source_name) - 1},
    .resolver_name = {source_name, sizeof(source_name) - 1},
    .source_bytes = {roff, sizeof(roff) - 1}
};
static const struct mant_structured_input_view input = {
    .sources = {&source, 1, sizeof(source)},
    .root_input = 1,
    .profile = MANT_PROFILE_UTF8,
    .width = 78
};

static struct mant_structured_limits
limits(void)
{
	struct mant_structured_limits cap = {0};
	uint64_t *field = &cap.max_input_sources;
	size_t count = (offsetof(struct mant_structured_limits, reserved) -
	    offsetof(struct mant_structured_limits, max_input_sources)) /
	    sizeof(*field);

	for (size_t i = 0; i < count; i++)
		field[i] = 1000000;
	return cap;
}

static void *
snapshot(const void *bytes, size_t length)
{
	void *copy;

	assert(length != 0 && bytes != NULL);
	copy = malloc(length);
	assert(copy != NULL);
	memcpy(copy, bytes, length);
	return copy;
}

static void
assert_unchanged_surface(const struct mant_annotated_display_view *before,
    const struct mant_annotated_display_view *after, const uint8_t *bytes,
    const struct mant_annotated_display_row *rows,
    const struct mant_annotated_display_run *runs)
{
	uint32_t i;

	assert(after->bytes == before->bytes && after->rows == before->rows &&
	    after->runs == before->runs);
	assert(after->byte_count == before->byte_count &&
	    after->row_count == before->row_count &&
	    after->run_count == before->run_count &&
	    after->input_bytes == before->input_bytes &&
	    after->work == before->work &&
	    after->peak_allocated_bytes == before->peak_allocated_bytes);
	assert(memcmp(after->bytes, bytes, (size_t)after->byte_count) == 0);
	assert(memcmp(after->rows, rows,
	    (size_t)after->row_count * sizeof(*rows)) == 0);
	for (i = 0; i < after->run_count; i++) {
		const struct mant_annotated_display_run *now = after->runs + i;
		const struct mant_annotated_display_run *old = runs + i;

		assert(now->key == old->key && now->column == old->column &&
		    now->width == old->width && now->reserved == old->reserved &&
		    now->byte_start == old->byte_start &&
		    now->byte_count == old->byte_count &&
		    now->label.source == old->label.source &&
		    now->label.style == old->label.style &&
		    now->label.role == old->label.role &&
		    now->label.glyph_origin == old->label.glyph_origin &&
		    now->label.flags == old->label.flags &&
		    now->label.reserved == old->label.reserved);
		assert(now->label.owner == 0 && now->label.link == 0 &&
		    now->label.head_component == 0);
	}
}

static void
test_hard_render_budget(void)
{
	struct mant_structured_limits cap = limits();
	struct mant_annotated_result *result = NULL;
	struct mant_structured_failure_view failure;

	cap.max_builder_operations = 1;
	assert(mant_annotated_render(&input, &cap, &result, &failure) ==
	    MANT_STRUCTURED_BUDGET);
	assert(result == NULL && failure.status == MANT_STRUCTURED_BUDGET);
}

static void
test_result_isolation(void)
{
	struct mant_structured_limits cap = limits(), small_cap;
	struct mant_annotated_result *result = NULL;
	struct mant_annotated_result_view before, after;
	struct mant_structured_failure_view failure;
	struct structured_session session = {0};
	struct mant_annotated_display_run *runs;
	struct mant_annotated_display_row *rows;
	uint8_t *bytes, *mutable_bytes, saved_byte;
	uint32_t saved_source_key, saved_style, saved_parent;
	uint64_t saved_start;
	uint32_t i, had_owner = 0, had_link = 0, had_style = 0;

	assert(mant_annotated_render(&input, &cap, &result, &failure) ==
	    MANT_STRUCTURED_OK);
	assert(result != NULL && result->checked == 1);
	assert(mant_annotated_result_check(result, &failure) ==
	    MANT_STRUCTURED_OK);
	assert(mant_annotated_result_view(result, &before) ==
	    MANT_STRUCTURED_OK);
	assert(before.annotation_degraded == 0 && before.marks.count > 0);
	assert(before.display.byte_count != 0 && before.display.row_count != 0 &&
	    before.display.run_count != 0);
	assert(before.sources.count == 1 && result->common->sources[0].key == 1);
	for (i = 0; i < before.display.run_count; i++) {
		had_owner |= before.display.runs[i].label.owner != 0;
		had_link |= before.display.runs[i].label.link != 0;
		had_style |= before.display.runs[i].label.style != 0;
	}
	assert(had_owner && had_link && had_style);
	assert(before.display.byte_count <= SIZE_MAX);
	bytes = snapshot(before.display.bytes, (size_t)before.display.byte_count);
	rows = snapshot(before.display.rows,
	    (size_t)before.display.row_count * sizeof(*rows));
	runs = snapshot(before.display.runs,
	    (size_t)before.display.run_count * sizeof(*runs));

	/* Source identity, UTF-8 and run bounds are hard display boundaries.
	 * Even a relation failure may not pass strip while any is invalid. */
	result->checked = 0;
	saved_source_key = result->common->sources[0].key;
	result->common->sources[0].key = 0;
	assert(!mant_annotated_result_is_surface_valid(result));
	assert(!mant_annotated_result_strip_annotations(result));
	result->common->sources[0].key = saved_source_key;
	mutable_bytes = (uint8_t *)before.display.bytes;
	saved_byte = mutable_bytes[0];
	mutable_bytes[0] = 0xff;
	assert(!mant_annotated_result_is_surface_valid(result));
	assert(!mant_annotated_result_strip_annotations(result));
	mutable_bytes[0] = saved_byte;
	saved_start = ((struct mant_annotated_display_run *)
	    before.display.runs)[0].byte_start;
	((struct mant_annotated_display_run *)before.display.runs)[0].byte_start =
	    before.display.byte_count + 1;
	assert(!mant_annotated_result_is_surface_valid(result));
	assert(!mant_annotated_result_strip_annotations(result));
	((struct mant_annotated_display_run *)before.display.runs)[0].byte_start =
	    saved_start;
	saved_style = ((struct mant_annotated_display_run *)
	    before.display.runs)[0].label.style;
	((struct mant_annotated_display_run *)before.display.runs)[0].label.style =
	    saved_style | 2;
	assert(!mant_annotated_result_is_surface_valid(result));
	assert(!mant_annotated_result_strip_annotations(result));
	((struct mant_annotated_display_run *)before.display.runs)[0].label.style =
	    saved_style;
	assert(mant_annotated_result_is_surface_valid(result));

	/* Break a genuine, C-owned mark relation.  The normal result is now
	 * unusable, but the already finished native body is independently safe. */
	assert(result->mark_count != 0);
	saved_parent = result->marks[0].parent;
	result->marks[0].parent = result->marks[0].key;
	assert(!mant_annotated_result_is_valid(result));
	result->checked = 1;
	assert(mant_annotated_result_check(result, &failure) ==
	    MANT_STRUCTURED_RELATION);
	assert(mant_annotated_result_view(result, &after) ==
	    MANT_STRUCTURED_RELATION);
	result->checked = 0;
	assert(mant_annotated_result_strip_annotations(result));
	assert(result->annotation_degraded == 1 && result->marks == NULL &&
	    result->mark_count == 0 && result->selection_parts == NULL &&
	    result->selection_part_count == 0 && result->join_text == NULL &&
	    result->join_text_count == 0);
	(void)saved_parent; /* The invalid mark was freed by strip. */

	/* Coverage work remains budgeted even for the body-only result.  A
	 * failed rebuild cannot be checked or exposed as a successful result. */
	small_cap = cap;
	small_cap.max_builder_operations = 23;
	session.limits = &small_cap;
	session.status = MANT_STRUCTURED_OK;
	session.annotated_section_candidate = 1;
	session.annotated_link_candidate = 1;
	assert(!mant_annotated_coverage_build(&session, result));
	assert(session.status == MANT_STRUCTURED_BUDGET);
	assert(!mant_annotated_result_is_valid(result));
	assert(mant_annotated_result_check(result, &failure) ==
	    MANT_STRUCTURED_RELATION);
	assert(mant_annotated_result_view(result, &after) ==
	    MANT_STRUCTURED_RELATION);
	memset(&session, 0, sizeof(session));
	session.limits = &cap;
	session.status = MANT_STRUCTURED_OK;
	session.annotated_section_candidate = 1;
	session.annotated_link_candidate = 1;
	assert(mant_annotated_coverage_build(&session, result));
	assert(session.status == MANT_STRUCTURED_OK);
	assert(mant_annotated_result_is_valid(result));
	result->checked = 1;
	assert(mant_annotated_result_check(result, &failure) ==
	    MANT_STRUCTURED_OK);
	assert(mant_annotated_result_view(result, &after) ==
	    MANT_STRUCTURED_OK);
	assert(after.annotation_degraded == 1 && after.marks.count == 0 &&
	    after.selection_parts.count == 0 && after.join_text.count == 0);
	assert(after.coverage_checks.count == 24 &&
	    after.coverage_issues.count != 0);
	assert_unchanged_surface(&before.display, &after.display, bytes, rows, runs);
	free(runs);
	free(rows);
	free(bytes);
	mant_annotated_result_free(result);
}

static void
test_one_rejected_link_retains_other_marks(void)
{
	struct mant_structured_limits cap = limits();
	struct mant_annotated_result *result = NULL;
	struct mant_annotated_result_view view;
	struct mant_structured_failure_view failure;
	struct structured_session session = {0};
	struct mant_annotated_mark *link = NULL;
	uint8_t *body;
	uint32_t i, owner_count = 0, rejected_issues = 0;
	uint8_t saved_target_byte;
	size_t body_length;

	/* Internal fault injection after the exact CVS-checked fixture rendered:
	 * only the link destination is rejected.  This does not assert that any
	 * new roff spelling triggers a decoder failure in pinned CVS. */
	assert(mant_annotated_render(&input, &cap, &result, &failure) ==
	    MANT_STRUCTURED_OK);
	assert(result != NULL && result->checked == 1);
	assert(mant_annotated_result_view(result, &view) == MANT_STRUCTURED_OK);
	assert(view.display.byte_count <= SIZE_MAX);
	body_length = (size_t)view.display.byte_count;
	body = snapshot(view.display.bytes, body_length);
	for (i = 0; i < result->mark_count; i++) {
		if (result->marks[i].kind == MANT_ANNOTATED_MARK_OWNER)
			owner_count++;
		if (result->marks[i].kind == MANT_ANNOTATED_MARK_LINK) {
			assert(link == NULL);
			link = result->marks + i;
		}
	}
	assert(owner_count != 0 && link != NULL && link->target_kind != 0);
	result->checked = 0;
	/* A malformed target string is not an isolatable semantic spelling.
	 * It fails the strict UTF-8 result check while the body remains intact. */
	assert(link->target_a.len != 0 && link->target_a.ptr != NULL);
	saved_target_byte = link->target_a.ptr[0];
	((uint8_t *)link->target_a.ptr)[0] = 0xff;
	assert(mant_annotated_result_is_surface_valid(result));
	assert(!mant_annotated_result_is_valid(result));
	((uint8_t *)link->target_a.ptr)[0] = saved_target_byte;
	assert(mant_annotated_result_is_valid(result));
	free((void *)link->target_a.ptr);
	free((void *)link->target_b.ptr);
	memset(&link->target_a, 0, sizeof(link->target_a));
	memset(&link->target_b, 0, sizeof(link->target_b));
	link->target_kind = link->target_b_present = 0;
	result->native_link_rejected = 1;
	free(result->coverage_issues);
	result->coverage_issues = NULL;
	result->coverage_issue_count = result->coverage_issue_capacity = 0;
	session.limits = &cap;
	session.status = MANT_STRUCTURED_OK;
	session.annotated_section_candidate = 1;
	session.annotated_link_candidate = 1;
	assert(mant_annotated_coverage_build(&session, result));
	assert(session.status == MANT_STRUCTURED_OK);
	assert(mant_annotated_result_is_valid(result));
	for (i = 0; i < result->coverage_issue_count; i++) {
		const struct mant_annotated_coverage_issue *issue =
		    result->coverage_issues + i;

		if (issue->producer == MANT_ANNOTATED_COVERAGE_NATIVE &&
		    issue->dimension == MANT_ANNOTATED_COVERAGE_LINK &&
		    issue->reason == MANT_ANNOTATED_COVERAGE_REJECTED)
			rejected_issues++;
	}
	assert(rejected_issues == 1);
	assert(result->mark_count == view.marks.count);
	assert(memcmp(view.display.bytes, body, body_length) == 0);
	owner_count = 0;
	for (i = 0; i < result->mark_count; i++)
		owner_count += result->marks[i].kind == MANT_ANNOTATED_MARK_OWNER;
	assert(owner_count != 0);
	result->checked = 1;
	assert(mant_annotated_result_check(result, &failure) ==
	    MANT_STRUCTURED_OK);
	assert(mant_annotated_result_view(result, &view) == MANT_STRUCTURED_OK);
	assert(view.annotation_degraded == 0 && view.marks.count != 0);
	free(body);
	mant_annotated_result_free(result);
}

int
main(void)
{
	test_hard_render_budget();
	test_result_isolation();
	test_one_rejected_link_retains_other_marks();
	puts("annotated result isolation: okay");
	return 0;
}
