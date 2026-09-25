/* Standalone C device-stream tests. Build with:
 * cc -std=c11 -Wall -Wextra -Werror -pedantic -fsanitize=address,undefined \
 *   -Icrates/libmandoc-rs/shim \
 *   crates/libmandoc-rs/tests/annotated_display_builder.c \
 *   crates/libmandoc-rs/shim/mant_mandoc_annotated_display.c \
 *   -o target/annotated-display-builder-test
 */
#include "mant_mandoc_annotated_display.h"

#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <string.h>

static size_t
native_width(void *arg, uint32_t scalar)
{
	(void)arg;
	if (scalar == 0x754c)
		return 2;
	if (scalar == 0x301)
		return 0;
	return 1;
}

static struct mant_annotated_display_limits
limits(void)
{
	struct mant_annotated_display_limits limits = {
	    4096, 4096, 65536, 65536, 64, 128, 1024
	};
	return limits;
}

static struct mant_annotated_display_label
label(uint32_t owner, uint32_t role)
{
	struct mant_annotated_display_label label = {0};
	label.owner = owner;
	label.source = owner;
	label.role = role;
	return label;
}

static void
test_reference_overstrike(void)
{
	struct mant_annotated_display_limits cap = limits();
	struct mant_annotated_display *display;
	struct mant_annotated_display_view view;
	struct mant_annotated_display_label body = label(1,
	    MANT_ANNOTATED_BODY);

	/* The exact input `.TH T 1\n.SH D\n.nf\n\zAB\n.fi\n` was run
	 * through the fixed CVS reference: term.c::term_field emits A\bB. */
	display = mant_annotated_display_new(&cap, native_width, NULL);
	assert(display != NULL);
	assert(mant_annotated_display_write(display, "A\bB\n", 4, body));
	assert(mant_annotated_display_finish(display, &view));
	assert(view.row_count == 1 && view.run_count == 1);
	assert(view.rows[0].break_after == 1);
	assert(view.rows[0].column_count == 1);
	assert(view.byte_count == 1 && view.bytes[0] == 'B');
	assert(view.runs[0].label.owner == 1);
	assert(view.runs[0].label.style == 0);
	mant_annotated_display_free(display);

	/* The same reference with `\fBA\fP` emits A\bA. */
	display = mant_annotated_display_new(&cap, native_width, NULL);
	body.glyph_origin = 9;
	body.flags = MANT_ANNOTATED_FONT_STROKE;
	assert(mant_annotated_display_write(display, "A", 1, body));
	body.flags = 0;
	assert(mant_annotated_display_write(display, "\bA\n", 3, body));
	assert(mant_annotated_display_finish(display, &view));
	assert(view.byte_count == 1 && view.bytes[0] == 'A');
	assert(view.runs[0].label.style & MANT_ANNOTATED_STYLE_BOLD);
	mant_annotated_display_free(display);

	/* The same reference with `\fIA\fP` emits _\bA. */
	display = mant_annotated_display_new(&cap, native_width, NULL);
	body.glyph_origin = 10;
	body.flags = MANT_ANNOTATED_FONT_STROKE;
	assert(mant_annotated_display_write(display, "_", 1, body));
	body.flags = 0;
	assert(mant_annotated_display_write(display, "\bA\n", 3, body));
	assert(mant_annotated_display_finish(display, &view));
	assert(view.byte_count == 1 && view.bytes[0] == 'A');
	assert(view.runs[0].label.style & MANT_ANNOTATED_STYLE_UNDERLINE);
	mant_annotated_display_free(display);

	/* Fixed CVS `\zAA` emits the same bytes as bold, but its real
	 * overstrike is not a TERM_COLLECT_FONT stroke. */
	display = mant_annotated_display_new(&cap, native_width, NULL);
	body.glyph_origin = 11;
	assert(mant_annotated_display_write(display, "A\bA\n", 4, body));
	assert(mant_annotated_display_finish(display, &view));
	assert(view.byte_count == 1 && view.bytes[0] == 'A');
	assert(view.runs[0].label.style == 0);
	mant_annotated_display_free(display);
}

static void
test_wide_overwrite_and_utf8_boundary(void)
{
	struct mant_annotated_display_limits cap = limits();
	struct mant_annotated_display *display;
	struct mant_annotated_display_view view;
	struct mant_annotated_display_label body = label(1,
	    MANT_ANNOTATED_BODY);
	const uint8_t wide[] = {0xe7, 0x95, 0x8c};

	/* Fixed CVS `.TH T 1\n.SH D\n.nf\n\z界X\n.fi\n` emits
	 * UTF-8(界), backspace, X: term_ascii.c::utf8_letter. */
	display = mant_annotated_display_new(&cap, native_width, NULL);
	assert(mant_annotated_display_write(display, wide, 1, body));
	assert(mant_annotated_display_write(display, wide + 1, 2, body));
	assert(mant_annotated_display_write(display, "\bX\n", 3, body));
	assert(mant_annotated_display_finish(display, &view));
	assert(view.row_count == 1 && view.rows[0].column_count == 2);
	assert(view.byte_count == 2 && memcmp(view.bytes, " X", 2) == 0);
	assert(view.run_count == 2 &&
	    view.runs[0].label.role == MANT_ANNOTATED_LAYOUT &&
	    view.runs[1].label.role == MANT_ANNOTATED_BODY);
	mant_annotated_display_free(display);

	/* Fixed CVS `\fB界\fP` emits 界\b界.  A proven synthetic
	 * stroke folds at the original wide-cell start, unlike real \z. */
	display = mant_annotated_display_new(&cap, native_width, NULL);
	body.glyph_origin = 20;
	body.flags = MANT_ANNOTATED_FONT_STROKE;
	assert(mant_annotated_display_write(display, wide, 3, body));
	body.flags = 0;
	assert(mant_annotated_display_write(display, "\b", 1, body));
	assert(mant_annotated_display_write(display, wide, 3, body));
	assert(mant_annotated_display_finish(display, &view));
	assert(view.row_count == 1 && view.rows[0].column_count == 2);
	assert(view.byte_count == 3 && memcmp(view.bytes, wide, 3) == 0);
	assert(view.runs[0].label.style & MANT_ANNOTATED_STYLE_BOLD);
	mant_annotated_display_free(display);

	/* Pinned term.c::encode1 prints _\b界 for a wide italic glyph.
	 * The one-cell underscore is a proven FONT stroke, not real \z. */
	display = mant_annotated_display_new(&cap, native_width, NULL);
	body.glyph_origin = 21;
	body.flags = MANT_ANNOTATED_FONT_STROKE;
	assert(mant_annotated_display_write(display, "_", 1, body));
	body.flags = 0;
	assert(mant_annotated_display_write(display, "\b", 1, body));
	assert(mant_annotated_display_write(display, wide, 3, body));
	assert(mant_annotated_display_finish(display, &view));
	assert(view.row_count == 1 && view.rows[0].column_count == 2);
	assert(view.byte_count == 3 && memcmp(view.bytes, wide, 3) == 0);
	assert(view.runs[0].label.style & MANT_ANNOTATED_STYLE_UNDERLINE);
	mant_annotated_display_free(display);

	/* A split scalar cannot silently acquire a different output owner. */
	display = mant_annotated_display_new(&cap, native_width, NULL);
	assert(mant_annotated_display_write(display, wide, 1, body));
	assert(!mant_annotated_display_write(display, wide + 1, 2,
	    label(2, MANT_ANNOTATED_BODY)));
	assert(mant_annotated_display_status(display) ==
	    MANT_ANNOTATED_DISPLAY_INVALID);
	assert(!mant_annotated_display_finish(display, &view));
	assert(view.rows == NULL && view.bytes == NULL);
	mant_annotated_display_free(display);

	/* UTF-8 continuation may not silently change execution glyph identity. */
	display = mant_annotated_display_new(&cap, native_width, NULL);
	body.glyph_origin = 30;
	body.flags = MANT_ANNOTATED_FONT_STROKE;
	assert(mant_annotated_display_write(display, wide, 1, body));
	body.glyph_origin = 31;
	body.flags = 0;
	assert(!mant_annotated_display_write(display, wide + 1, 2, body));
	assert(mant_annotated_display_status(display) ==
	    MANT_ANNOTATED_DISPLAY_INVALID);
	mant_annotated_display_free(display);
}

static void
test_identity_and_filtered_device_roles(void)
{
	struct mant_annotated_display_limits cap = limits();
	struct mant_annotated_display *display;
	struct mant_annotated_display_view view;
	struct mant_annotated_display_label one = label(1,
	    MANT_ANNOTATED_BODY);
	struct mant_annotated_display_label two = label(2,
	    MANT_ANNOTATED_BODY);

	/* Label partition is an adapter invariant, independent of roff syntax:
	 * later equal glyphs from another owner replace, never inherit identity. */
	display = mant_annotated_display_new(&cap, native_width, NULL);
	assert(mant_annotated_display_write(display, "TITLE\n", 6,
	    label(0, MANT_ANNOTATED_HEADER)));
	assert(mant_annotated_display_write(display, "A\b", 2, one));
	assert(mant_annotated_display_write(display, "A\n", 2, two));
	assert(mant_annotated_display_write(display, "FOOT\n", 5,
	    label(0, MANT_ANNOTATED_FOOTER)));
	assert(mant_annotated_display_finish(display, &view));
	assert(view.row_count == 1 && view.byte_count == 1);
	assert(view.bytes[0] == 'A' && view.runs[0].label.owner == 2);
	assert(view.runs[0].label.style == 0);
	mant_annotated_display_free(display);
}

static void
test_combining_survival(void)
{
	struct mant_annotated_display_limits cap = limits();
	struct mant_annotated_display *display;
	struct mant_annotated_display_view view;
	struct mant_annotated_display_label one = label(1,
	    MANT_ANNOTATED_BODY);
	struct mant_annotated_display_label two = label(2,
	    MANT_ANNOTATED_BODY);
	const uint8_t acute[] = {0xcc, 0x81};

	/* Fixed CVS `.TH T 1\n.SH D\n.nf\na\[u0301]\n.fi\n`
	 * emits a + UTF-8 combining acute; term_ascii.c::utf8_letter. */
	display = mant_annotated_display_new(&cap, native_width, NULL);
	assert(mant_annotated_display_write(display, "a", 1, one));
	assert(mant_annotated_display_write(display, acute, 2, two));
	assert(mant_annotated_display_finish(display, &view));
	assert(view.row_count == 1 && view.rows[0].column_count == 1);
	assert(view.byte_count == 3 &&
	    memcmp(view.bytes, "a\xcc\x81", 3) == 0);
	assert(view.run_count == 2 && view.runs[0].label.owner == 1 &&
	    view.runs[1].label.owner == 2);
	mant_annotated_display_free(display);

	/* A later real overlay removes the base and its attached mark. */
	display = mant_annotated_display_new(&cap, native_width, NULL);
	assert(mant_annotated_display_write(display, "a", 1, one));
	assert(mant_annotated_display_write(display, acute, 2, two));
	assert(mant_annotated_display_write(display, "\bB", 2, one));
	assert(mant_annotated_display_finish(display, &view));
	assert(view.byte_count == 1 && view.bytes[0] == 'B');
	assert(view.run_count == 1 && view.runs[0].label.owner == 1);
	mant_annotated_display_free(display);
}

static void
test_blank_rows_and_budgets(void)
{
	struct mant_annotated_display_limits cap = limits();
	struct mant_annotated_display *display;
	struct mant_annotated_display_view view;
	struct mant_annotated_display_label body = label(1,
	    MANT_ANNOTATED_BODY);

	/* Device newlines are retained even when a row has no visible glyph. */
	display = mant_annotated_display_new(&cap, native_width, NULL);
	assert(mant_annotated_display_write(display, "\n\nA", 3, body));
	assert(mant_annotated_display_finish(display, &view));
	assert(view.row_count == 3 && view.run_count == 1);
	assert(view.rows[0].break_after && view.rows[1].break_after);
	assert(!view.rows[2].break_after);
	mant_annotated_display_free(display);

	cap.max_input_bytes = 2;
	display = mant_annotated_display_new(&cap, native_width, NULL);
	assert(mant_annotated_display_write(display, "A", 1, body));
	assert(!mant_annotated_display_write(display, "BC", 2, body));
	assert(mant_annotated_display_status(display) ==
	    MANT_ANNOTATED_DISPLAY_BUDGET);
	assert(!mant_annotated_display_finish(display, &view));
	assert(view.row_count == 0 && view.bytes == NULL);
	mant_annotated_display_free(display);

	/* ESC is not an executable control channel in a safe surface. */
	cap = limits();
	display = mant_annotated_display_new(&cap, native_width, NULL);
	assert(!mant_annotated_display_write(display, "\033[31m", 5, body));
	assert(mant_annotated_display_status(display) ==
	    MANT_ANNOTATED_DISPLAY_UNSAFE_CONTROL);
	mant_annotated_display_free(display);
}

static void
test_annotation_drop_preserves_finished_surface(void)
{
	struct mant_annotated_display_limits cap = limits();
	struct mant_annotated_display *display;
	struct mant_annotated_display_view before, after;
	struct mant_annotated_display_label body = label(2,
	    MANT_ANNOTATED_BODY);
	const uint8_t *bytes;
	const struct mant_annotated_display_row *rows;
	const struct mant_annotated_display_run *runs;
	uint64_t length;
	uint32_t row_count, run_count;

	/* This is an adapter invariant, not a roff syntax assertion. The
	 * semantic channels may be rejected only after the device text is
	 * finished; dropping them must not change any visible bytes or geometry. */
	body.source = 1;
	body.link = 3;
	body.head_component = 4;
	display = mant_annotated_display_new(&cap, native_width, NULL);
	assert(display != NULL);
	assert(!mant_annotated_display_clear_annotations(display));
	assert(mant_annotated_display_write(display, "alpha\nbeta\n", 11,
	    body));
	assert(mant_annotated_display_finish(display, &before));
	bytes = before.bytes;
	rows = before.rows;
	runs = before.runs;
	length = before.byte_count;
	row_count = before.row_count;
	run_count = before.run_count;
	assert(mant_annotated_display_clear_annotations(display));
	assert(mant_annotated_display_finish(display, &after));
	assert(after.bytes == bytes && after.rows == rows && after.runs == runs);
	assert(after.byte_count == length && after.row_count == row_count &&
	    after.run_count == run_count);
	assert(memcmp(after.bytes, "alphabeta", 9) == 0);
	assert(after.rows[0].column_count == 5 && after.rows[0].break_after);
	assert(after.rows[1].column_count == 4 && after.rows[1].break_after);
	for (uint32_t i = 0; i < after.run_count; i++) {
		assert(after.runs[i].label.owner == 0);
		assert(after.runs[i].label.link == 0);
		assert(after.runs[i].label.head_component == 0);
		assert(after.runs[i].label.source == 1);
	}
	mant_annotated_display_free(display);
}

int
main(void)
{
	test_reference_overstrike();
	test_wide_overwrite_and_utf8_boundary();
	test_identity_and_filtered_device_roles();
	test_combining_survival();
	test_blank_rows_and_budgets();
	test_annotation_drop_preserves_finished_surface();
	puts("annotated display builder: okay");
	return 0;
}
