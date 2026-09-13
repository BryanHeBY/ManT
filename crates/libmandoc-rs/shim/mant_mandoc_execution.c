/*
 * ManT-owned adapter around the pinned mandoc terminal executor.
 *
 * The upstream term implementation remains authoritative.  This file only
 * assigns report-local identities, stores typed facts, enforces budgets, and
 * transfers sealed pointer-free records to Rust.
 */
#include "config.h"

#include <sys/types.h>

#include <limits.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

#include "mandoc.h"
#include "roff.h"
#include "out.h"
#include "tbl.h"
#include "term.h"
#include "manconf.h"
#include "main.h"

#include "mant_mandoc_execution.h"
#include "mant_mandoc_table_private.h"

struct node_index {
	const struct roff_node *node;
	uint32_t key;
};

struct word_index {
	const char *word;
	uint32_t key;
};

struct buffer_origin {
	uint32_t *atoms;
	size_t capacity;
	uint32_t generation;
	uint32_t generation_record;
};

enum generation_checkpoint_kind {
	GENERATION_OPEN = 1,
	GENERATION_CLOSE,
	GENERATION_CHECK
};

struct generation_checkpoint {
	uint64_t sequence;
	uint32_t buffer;
	uint32_t generation;
	uint32_t kind;
};

#define RECORD_STORAGE(name, type) \
	struct type *name; \
	size_t name##_count; \
	size_t name##_capacity

struct mant_mandoc_execution_report {
	struct mant_mandoc_execution_limits limits;
	unsigned char *pool;
	char *error;
	struct node_index *node_index;
	struct word_index *word_index;
	struct buffer_origin *buffers;
	RECORD_STORAGE(sources, mant_mandoc_source_record);
	RECORD_STORAGE(nodes, mant_mandoc_node_record);
	RECORD_STORAGE(buffer_generations, mant_mandoc_buffer_generation_record);
	RECORD_STORAGE(words, mant_mandoc_word_record);
	RECORD_STORAGE(atoms, mant_mandoc_atom_record);
	RECORD_STORAGE(fragments, mant_mandoc_fragment_record);
	RECORD_STORAGE(fragment_atoms, mant_mandoc_fragment_atom_record);
	RECORD_STORAGE(flushes, mant_mandoc_flush_record);
	RECORD_STORAGE(boundaries, mant_mandoc_boundary_record);
	RECORD_STORAGE(controls, mant_mandoc_control_record);
	RECORD_STORAGE(geometries, mant_mandoc_geometry_record);
	RECORD_STORAGE(wrappers, mant_mandoc_wrapper_record);
	RECORD_STORAGE(references, mant_mandoc_reference_record);
	RECORD_STORAGE(anchors, mant_mandoc_anchor_record);
	RECORD_STORAGE(tables, mant_mandoc_table_record);
	RECORD_STORAGE(table_rows, mant_mandoc_table_row_record);
	RECORD_STORAGE(table_cells, mant_mandoc_table_cell_record);
	RECORD_STORAGE(diagnostics, mant_mandoc_execution_diagnostic_record);
	size_t pool_length;
	size_t pool_capacity;
	uint64_t record_count;
	uint64_t work_count;
	size_t node_count;
	size_t word_count;
	size_t buffer_count;
	uint64_t buffer_cells;
	uint64_t allocated_record_bytes;
	uint32_t current_wrapper;
	uint32_t current_reference;
	uint32_t current_word_start;
	uint32_t current_word_length;
	uint32_t current_word_node;
	uint32_t current_word;
	uint32_t current_flush;
	uint32_t current_boundary;
	uint32_t current_control;
	uint32_t current_table;
	uint32_t current_table_row;
	uint32_t current_table_cell;
	const struct roff_node *current_table_cell_node;
	const struct tbl_span *current_table_cell_span;
	const struct tbl_cell *current_table_cell_layout;
	const struct tbl_dat *current_table_cell_data;
	size_t current_table_cell_ordinal;
	size_t current_table_cell_data_ordinal;
	size_t current_table_cell_column;
	size_t current_table_cell_coloff;
	uint64_t sequence;
	int (*cancelled)(void *);
	void *cancellation_context;
	int word_active;
	int status;
};

#undef RECORD_STORAGE

static int execution_work(void *, const struct termp *,
    const struct roff_node *, size_t);
static void execution_abort(void *, const struct termp *,
    const struct roff_node *);
static int execution_node_enter(void *, const struct termp *,
    const struct roff_node *);
static int execution_node_leave(void *, const struct termp *,
    const struct roff_node *);
static int execution_word_begin(void *, const struct termp *,
    const struct roff_node *, const char *, size_t, int *);
static int execution_word_end(void *, const struct termp *,
    const struct roff_node *);
static int execution_buffer_write(void *, const struct termp *,
    const struct roff_node *, size_t, int, int, int);
static int execution_buffer_reserve(void *, const struct termp *,
    const struct roff_node *, size_t, size_t);
static int execution_buffer_rewrite(void *, const struct termp *,
    const struct roff_node *, size_t, int);
static int execution_buffer_discard(void *, const struct termp *,
    const struct roff_node *, size_t, size_t, int);
static int execution_buffer_reset(void *, const struct termp *,
    const struct roff_node *);
static int execution_flush_begin(void *, const struct termp *,
    const struct roff_node *);
static int execution_fill_scan(void *, const struct termp *,
    const struct roff_node *, size_t);
static int execution_fill_decision(void *, const struct termp *,
    const struct roff_node *, size_t, size_t, size_t, size_t);
static int execution_fill_outcome(void *, const struct termp *,
    const struct roff_node *, int);
static int execution_field_begin(void *, const struct termp *,
    const struct roff_node *, size_t, size_t, size_t);
static int execution_field_atom(void *, const struct termp *,
    const struct roff_node *, size_t, int);
static int execution_field_end(void *, const struct termp *,
    const struct roff_node *, size_t, size_t, size_t);
static int execution_flush_end(void *, const struct termp *,
    const struct roff_node *);
static int execution_boundary_enter(void *, const struct termp *,
    const struct roff_node *, int);
static int execution_boundary_leave(void *, const struct termp *,
    const struct roff_node *, int);
static int execution_control_enter(void *, const struct termp *,
    const struct roff_node *);
static int execution_control_leave(void *, const struct termp *,
    const struct roff_node *);
static int execution_device_advance(void *, const struct termp *,
    const struct roff_node *, size_t, size_t, size_t);
static int execution_device_letter(void *, const struct termp *,
    const struct roff_node *, size_t, int, size_t, size_t);
static int execution_device_endline(void *, const struct termp *,
    const struct roff_node *, size_t, size_t, size_t, size_t);
static int execution_font(void *, const struct termp *,
    const struct roff_node *, int, int, size_t, size_t);
static int execution_reference_begin(void *, const struct termp *,
    const struct roff_node *, const struct roff_node *,
    const struct roff_node *, int, const char *, size_t,
    const char *, size_t, int);
static int execution_reference_end(void *, const struct termp *,
    const struct roff_node *);
static int execution_anchor(void *, const struct termp *,
    const struct roff_node *, const char *, size_t, size_t, int);
static int execution_table_preflight(void *, const struct termp *,
    const struct roff_node *, size_t);
static int execution_table_begin(void *, const struct termp *,
    const struct roff_node *, const struct tbl_span *, size_t, size_t,
    size_t);
static int execution_table_end(void *, const struct termp *,
    const struct roff_node *, const struct tbl_span *);
static int execution_table_row_begin(void *, const struct termp *,
    const struct roff_node *, const struct tbl_span *);
static int execution_table_row_end(void *, const struct termp *,
    const struct roff_node *, const struct tbl_span *);
static int execution_table_cell_begin(void *, const struct termp *,
    const struct roff_node *, const struct tbl_span *, const struct tbl_cell *,
    const struct tbl_dat *, size_t, size_t, size_t, size_t);
static int execution_table_cell_end(void *, const struct termp *,
    const struct roff_node *, const struct tbl_span *, const struct tbl_cell *,
    const struct tbl_dat *, size_t, size_t, size_t, size_t);

static const struct term_exec_ops execution_ops = {
	execution_work,
	execution_abort,
	execution_node_enter,
	execution_node_leave,
	execution_word_begin,
	execution_word_end,
	execution_buffer_write,
	execution_buffer_reserve,
	execution_buffer_rewrite,
	execution_buffer_discard,
	execution_buffer_reset,
	execution_flush_begin,
	execution_fill_scan,
	execution_fill_decision,
	execution_fill_outcome,
	execution_field_begin,
	execution_field_atom,
	execution_field_end,
	execution_flush_end,
	execution_boundary_enter,
	execution_boundary_leave,
	execution_control_enter,
	execution_control_leave,
	execution_device_advance,
	execution_device_letter,
	execution_device_endline,
	execution_font,
	execution_reference_begin,
	execution_reference_end,
	execution_anchor,
	execution_table_preflight,
	execution_table_begin,
	execution_table_end,
	execution_table_row_begin,
	execution_table_row_end,
	execution_table_cell_begin,
	execution_table_cell_end
};

struct live_atom_location {
	uint32_t buffer_generation;
	uint32_t slot;
	uint32_t atom;
};

static void fail_report(struct mant_mandoc_execution_report *, int,
    const char *);
static int charge_work(struct mant_mandoc_execution_report *, uint64_t);
static int charge_sort_work(struct mant_mandoc_execution_report *, size_t);
static int charge_record(struct mant_mandoc_execution_report *);
static int append_pool(struct mant_mandoc_execution_report *, const void *,
    size_t, uint32_t *);
static int tree_supported(struct mant_mandoc_execution_report *,
    const struct roff_node *, uint64_t, uint64_t *);
static int finish_execution_run(struct mant_mandoc_execution_report *);
static int seal_report(struct mant_mandoc_execution_report *);
static int validate_report_storage(struct mant_mandoc_execution_report *);
static int validate_sealed_report(struct mant_mandoc_execution_report *);
static int validate_table_records(struct mant_mandoc_execution_report *);
static int valid_pool_range(const struct mant_mandoc_execution_report *,
    uint32_t, uint32_t, int);
static int valid_scalar(uint32_t);
static int terminal_tail_scalar(uint32_t);
static int size_to_report_i64(struct mant_mandoc_execution_report *, size_t,
    int64_t *);
static int size_delta_to_report_i64(struct mant_mandoc_execution_report *,
    size_t, size_t, int64_t *);
static int collect_nodes(struct mant_mandoc_execution_report *,
    const struct roff_node *, uint32_t, uint64_t);
static int compare_node_index(const void *, const void *);
static int compare_word_index(const void *, const void *);
static int compare_live_atom_location(const void *, const void *);
static int compare_generation_checkpoint(const void *, const void *);
static size_t lower_bound_live_atom(const struct live_atom_location *,
    size_t, uint32_t, uint32_t);
static uint32_t lookup_node(const struct mant_mandoc_execution_report *,
    const struct roff_node *);
static uint32_t lookup_word_node(const struct mant_mandoc_execution_report *,
    const char *);
static uint32_t stable_node_kind(enum roff_type);
static uint32_t stable_node_flags(const struct roff_node *);
static uint32_t stable_term_flags(int);
static uint32_t stable_control_request(enum roff_tok);
static const char *stable_control_name(uint32_t);
static uint32_t stable_font(int);
static uint32_t atom_kind(int);
static uint32_t table_layout_kind(enum tbl_cellt);
static uint32_t table_data_kind(enum tbl_datt);
static int ensure_buffer(struct mant_mandoc_execution_report *, size_t,
    size_t);
static int note_buffer_extent(struct mant_mandoc_execution_report *, size_t,
    size_t);
static int close_buffer_generation(struct mant_mandoc_execution_report *,
    size_t, size_t, uint32_t);
static int append_fragment(struct mant_mandoc_execution_report *,
    const struct termp *, const struct roff_node *, int, size_t, size_t,
    size_t);
static int append_boundary(struct mant_mandoc_execution_report *,
    const struct termp *, const struct roff_node *, uint32_t, uint32_t);
static int capture_control_state(struct mant_mandoc_execution_report *,
    const struct termp *, struct mant_mandoc_control_record *, int);

#define DEFINE_RESERVE(name, type) \
static int \
reserve_##name(struct mant_mandoc_execution_report *report, size_t needed) \
{ \
	struct type *records; \
	size_t capacity; \
	if (needed <= report->name##_capacity) \
		return 1; \
	capacity = report->name##_capacity == 0 ? 64 : \
	    report->name##_capacity; \
	while (capacity < needed) { \
		if (capacity > SIZE_MAX / 2) { \
			fail_report(report, MANT_MANDOC_EXECUTION_ALLOCATION, \
			    "native execution record allocation overflow"); \
			return 0; \
		} \
		capacity *= 2; \
	} \
	if (capacity > SIZE_MAX / sizeof(*records)) { \
		fail_report(report, MANT_MANDOC_EXECUTION_ALLOCATION, \
		    "native execution record allocation overflow"); \
		return 0; \
	} \
	if (report->allocated_record_bytes > report->limits.max_report_bytes || \
	    (uint64_t)(capacity - report->name##_capacity) > \
	    (report->limits.max_report_bytes - report->allocated_record_bytes) / \
	    sizeof(*records)) { \
		fail_report(report, MANT_MANDOC_EXECUTION_BUDGET, \
		    "native execution report allocation limit exceeded"); \
		return 0; \
	} \
	records = realloc(report->name, capacity * sizeof(*records)); \
	if (records == NULL) { \
		fail_report(report, MANT_MANDOC_EXECUTION_ALLOCATION, \
		    "could not allocate native execution records"); \
		return 0; \
	} \
	report->name = records; \
	report->allocated_record_bytes += (uint64_t) \
	    (capacity - report->name##_capacity) * sizeof(*records); \
	report->name##_capacity = capacity; \
	return 1; \
}

DEFINE_RESERVE(sources, mant_mandoc_source_record)
DEFINE_RESERVE(nodes, mant_mandoc_node_record)
DEFINE_RESERVE(buffer_generations, mant_mandoc_buffer_generation_record)
DEFINE_RESERVE(words, mant_mandoc_word_record)
DEFINE_RESERVE(atoms, mant_mandoc_atom_record)
DEFINE_RESERVE(fragments, mant_mandoc_fragment_record)
DEFINE_RESERVE(fragment_atoms, mant_mandoc_fragment_atom_record)
DEFINE_RESERVE(flushes, mant_mandoc_flush_record)
DEFINE_RESERVE(boundaries, mant_mandoc_boundary_record)
DEFINE_RESERVE(controls, mant_mandoc_control_record)
DEFINE_RESERVE(geometries, mant_mandoc_geometry_record)
DEFINE_RESERVE(wrappers, mant_mandoc_wrapper_record)
DEFINE_RESERVE(references, mant_mandoc_reference_record)
DEFINE_RESERVE(anchors, mant_mandoc_anchor_record)
DEFINE_RESERVE(tables, mant_mandoc_table_record)
DEFINE_RESERVE(table_rows, mant_mandoc_table_row_record)
DEFINE_RESERVE(table_cells, mant_mandoc_table_cell_record)
DEFINE_RESERVE(diagnostics, mant_mandoc_execution_diagnostic_record)
#undef DEFINE_RESERVE

struct mant_mandoc_execution_report *
mant_mandoc_execution_alloc(const char *source_path,
    const struct mant_mandoc_execution_limits *limits,
    int (*cancelled)(void *), void *cancellation_context)
{
	struct mant_mandoc_execution_report *report;
	struct mant_mandoc_source_record *source;
	uint32_t path_start;

	if (source_path == NULL || *source_path == '\0' || limits == NULL ||
	    limits->abi_version != MANT_MANDOC_EXECUTION_LIMITS_VERSION ||
	    limits->abi_size != sizeof(*limits) ||
	    limits->max_nodes == 0 || limits->max_depth == 0 ||
	    limits->max_work == 0 || limits->max_records == 0 ||
	    limits->max_pool_bytes == 0 || limits->max_buffer_cells == 0 ||
	    limits->max_report_bytes == 0 ||
	    limits->max_nodes > UINT32_MAX ||
	    limits->max_records > UINT32_MAX ||
	    limits->max_pool_bytes > UINT32_MAX ||
	    limits->max_buffer_cells > UINT32_MAX)
		return NULL;
	report = calloc(1, sizeof(*report));
	if (report == NULL)
		return NULL;
	report->limits = *limits;
	report->cancelled = cancelled;
	report->cancellation_context = cancellation_context;
	report->current_wrapper = MANT_MANDOC_EXEC_NONE;
	report->current_reference = MANT_MANDOC_EXEC_NONE;
	report->current_flush = MANT_MANDOC_EXEC_NONE;
	report->current_boundary = MANT_MANDOC_EXEC_NONE;
	report->current_control = MANT_MANDOC_EXEC_NONE;
	report->current_table = MANT_MANDOC_EXEC_NONE;
	report->current_table_row = MANT_MANDOC_EXEC_NONE;
	report->current_table_cell = MANT_MANDOC_EXEC_NONE;
	report->current_word_node = MANT_MANDOC_EXEC_NONE;
	report->current_word = MANT_MANDOC_EXEC_NONE;
	if (!append_pool(report, source_path, strlen(source_path), &path_start) ||
	    !charge_record(report) || !reserve_sources(report, 1)) {
		return report;
	}
	source = &report->sources[report->sources_count++];
	memset(source, 0, sizeof(*source));
	source->key = 0;
	source->parent = MANT_MANDOC_EXEC_NONE;
	source->include_node = MANT_MANDOC_EXEC_NONE;
	source->path_start = path_start;
	source->path_length = (uint32_t)strlen(source_path);
	return report;
}

size_t
mant_mandoc_execution_limits_size(void)
{
	return sizeof(struct mant_mandoc_execution_limits);
}

struct mant_mandoc_execution_limits_alignment {
	char prefix;
	struct mant_mandoc_execution_limits value;
};

size_t
mant_mandoc_execution_limits_align(void)
{
	return offsetof(struct mant_mandoc_execution_limits_alignment, value);
}

uint32_t
mant_mandoc_execution_limits_field_count(void)
{
	return 9;
}

size_t
mant_mandoc_execution_limits_offset(uint32_t field)
{
	static const size_t offsets[] = {
		offsetof(struct mant_mandoc_execution_limits, abi_version),
		offsetof(struct mant_mandoc_execution_limits, abi_size),
		offsetof(struct mant_mandoc_execution_limits, max_nodes),
		offsetof(struct mant_mandoc_execution_limits, max_depth),
		offsetof(struct mant_mandoc_execution_limits, max_work),
		offsetof(struct mant_mandoc_execution_limits, max_records),
		offsetof(struct mant_mandoc_execution_limits, max_pool_bytes),
		offsetof(struct mant_mandoc_execution_limits, max_buffer_cells),
		offsetof(struct mant_mandoc_execution_limits, max_report_bytes)
	};

	return field < sizeof(offsets) / sizeof(offsets[0]) ?
	    offsets[field] : SIZE_MAX;
}

int
mant_mandoc_execution_run(struct mant_mandoc_execution_report *report,
    const struct roff_meta *meta)
{
	struct manoutput options;
	struct termp *termp;
	uint64_t node_count;

	if (report == NULL || meta == NULL || meta->first == NULL ||
	    report->status != MANT_MANDOC_EXECUTION_BUILDING) {
		if (report != NULL) {
			report->cancelled = NULL;
			report->cancellation_context = NULL;
		}
		return 0;
	}
	if (meta->source_request_seen) {
		fail_report(report, MANT_MANDOC_EXECUTION_UNSUPPORTED,
		    "native execution rejects executed .so and .soquiet requests");
		return finish_execution_run(report);
	}
	node_count = 0;
	if (!tree_supported(report, meta->first, 0, &node_count))
		return finish_execution_run(report);
	if (node_count > report->limits.max_nodes) {
		fail_report(report, MANT_MANDOC_EXECUTION_BUDGET,
		    "native execution node limit exceeded");
		return finish_execution_run(report);
	}
	report->node_count = (size_t)node_count;
	report->node_index = calloc(report->node_count,
	    sizeof(*report->node_index));
	report->word_index = calloc(report->node_count,
	    sizeof(*report->word_index));
	if (report->node_index == NULL || report->word_index == NULL ||
	    !collect_nodes(report, meta->first, MANT_MANDOC_EXEC_NONE, 0)) {
		if (report->status == MANT_MANDOC_EXECUTION_BUILDING)
			fail_report(report, MANT_MANDOC_EXECUTION_ALLOCATION,
			    "could not build native execution node registry");
		return finish_execution_run(report);
	}
	if (!charge_sort_work(report, report->node_count) ||
	    !charge_sort_work(report, report->word_count))
		return finish_execution_run(report);
	qsort(report->node_index, report->node_count,
	    sizeof(*report->node_index), compare_node_index);
	qsort(report->word_index, report->word_count,
	    sizeof(*report->word_index), compare_word_index);

	memset(&options, 0, sizeof(options));
	options.width = 78;
	options.indent = 5;
	termp = utf8_alloc(&options);
	if (termp == NULL) {
		fail_report(report, MANT_MANDOC_EXECUTION_ALLOCATION,
		    "could not allocate native execution device");
		return finish_execution_run(report);
	}
	term_exec_attach(termp, &execution_ops, report);
	if (meta->macroset == MACROSET_MDOC)
		terminal_mdoc(termp, meta);
	else if (meta->macroset == MACROSET_MAN)
		terminal_man(termp, meta);
	else
		fail_report(report, MANT_MANDOC_EXECUTION_UNSUPPORTED,
		    "native execution requires a man or mdoc document");
	if (term_exec_failed(termp) &&
	    report->status == MANT_MANDOC_EXECUTION_BUILDING)
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
		    "native execution observer failed");
	term_exec_attach(termp, NULL, NULL);
	ascii_free(termp);
	if (report->status == MANT_MANDOC_EXECUTION_BUILDING)
		seal_report(report);
	return finish_execution_run(report);
}

static int
finish_execution_run(struct mant_mandoc_execution_report *report)
{
	/* The cancellation context is caller-owned for this synchronous run. */
	report->cancelled = NULL;
	report->cancellation_context = NULL;
	return report->status == MANT_MANDOC_EXECUTION_COMPLETE;
}

void
mant_mandoc_execution_free(struct mant_mandoc_execution_report *report)
{
	size_t index;

	if (report == NULL)
		return;
	for (index = 0; index < report->buffer_count; index++)
		free(report->buffers[index].atoms);
	free(report->buffers);
	free(report->sources);
	free(report->nodes);
	free(report->buffer_generations);
	free(report->words);
	free(report->atoms);
	free(report->fragments);
	free(report->fragment_atoms);
	free(report->flushes);
	free(report->boundaries);
	free(report->controls);
	free(report->geometries);
	free(report->wrappers);
	free(report->references);
	free(report->anchors);
	free(report->tables);
	free(report->table_rows);
	free(report->table_cells);
	free(report->diagnostics);
	free(report->pool);
	free(report->error);
	free(report->node_index);
	free(report->word_index);
	free(report);
}

int
mant_mandoc_execution_status(const struct mant_mandoc_execution_report *report)
{
	return report == NULL ? MANT_MANDOC_EXECUTION_INTERNAL : report->status;
}

const char *
mant_mandoc_execution_error(const struct mant_mandoc_execution_report *report)
{
	return report == NULL ? NULL : report->error;
}

size_t
mant_mandoc_execution_pool_length(
    const struct mant_mandoc_execution_report *report)
{
	return report != NULL &&
	    report->status == MANT_MANDOC_EXECUTION_COMPLETE ?
	    report->pool_length : 0;
}

uint64_t
mant_mandoc_execution_work_count(
    const struct mant_mandoc_execution_report *report)
{
	return report != NULL &&
	    report->status == MANT_MANDOC_EXECUTION_COMPLETE ?
	    report->work_count : 0;
}

uint64_t
mant_mandoc_execution_record_count(
    const struct mant_mandoc_execution_report *report)
{
	return report != NULL &&
	    report->status == MANT_MANDOC_EXECUTION_COMPLETE ?
	    report->record_count : 0;
}

uint64_t
mant_mandoc_execution_allocated_record_bytes(
    const struct mant_mandoc_execution_report *report)
{
	return report != NULL &&
	    report->status == MANT_MANDOC_EXECUTION_COMPLETE ?
	    report->allocated_record_bytes : 0;
}

uint64_t
mant_mandoc_execution_buffer_cell_count(
    const struct mant_mandoc_execution_report *report)
{
	return report != NULL &&
	    report->status == MANT_MANDOC_EXECUTION_COMPLETE ?
	    report->buffer_cells : 0;
}

int
mant_mandoc_execution_node_key(
    const struct mant_mandoc_execution_report *report,
    const struct roff_node *node, uint32_t *key)
{
	uint32_t found;

	if (report == NULL ||
	    report->status != MANT_MANDOC_EXECUTION_COMPLETE ||
	    node == NULL || key == NULL)
		return 0;
	found = lookup_node(report, node);
	if (found == MANT_MANDOC_EXEC_NONE)
		return 0;
	*key = found;
	return 1;
}

static int
execution_work(void *arg, const struct termp *p,
    const struct roff_node *node, size_t amount)
{
	(void)p;
	(void)node;
	return charge_work(arg, amount);
}

static void
execution_abort(void *arg, const struct termp *p, const struct roff_node *node)
{
	(void)p;
	(void)node;
	fail_report(arg, MANT_MANDOC_EXECUTION_BUDGET,
	    "native execution integer state overflowed");
}

static int
execution_node_enter(void *arg, const struct termp *p,
    const struct roff_node *node)
{
	struct mant_mandoc_execution_report *report = arg;
	struct mant_mandoc_wrapper_record *record;

	(void)p;
	if (!charge_work(report, 1) || !charge_record(report) ||
	    !reserve_wrappers(report, report->wrappers_count + 1))
		return 0;
	record = &report->wrappers[report->wrappers_count];
	memset(record, 0, sizeof(*record));
	record->key = (uint32_t)report->wrappers_count++;
	record->parent = report->current_wrapper;
	record->node = lookup_node(report, node);
	record->kind = MANT_MANDOC_WRAPPER_NODE;
	record->target_start = MANT_MANDOC_EXEC_NONE;
	record->target_length = 0;
	record->enter_atom = (uint32_t)report->atoms_count;
	record->leave_atom = MANT_MANDOC_EXEC_NONE;
	record->enter_sequence = report->sequence++;
	record->leave_sequence = UINT64_MAX;
	report->current_wrapper = record->key;
	return 1;
}

static int
execution_node_leave(void *arg, const struct termp *p,
    const struct roff_node *node)
{
	struct mant_mandoc_execution_report *report = arg;
	struct mant_mandoc_wrapper_record *record;

	(void)p;
	if (!charge_work(report, 1) ||
	    report->current_wrapper == MANT_MANDOC_EXEC_NONE ||
	    report->current_wrapper >= report->wrappers_count)
		return 0;
	record = &report->wrappers[report->current_wrapper];
	if (record->kind != MANT_MANDOC_WRAPPER_NODE ||
	    record->node != lookup_node(report, node))
		return 0;
	record->leave_atom = (uint32_t)report->atoms_count;
	record->leave_sequence = report->sequence++;
	report->current_wrapper = record->parent;
	return 1;
}

static int
execution_word_begin(void *arg, const struct termp *p,
    const struct roff_node *node, const char *word, size_t length, int *role)
{
	struct mant_mandoc_execution_report *report = arg;
	struct mant_mandoc_word_record *record;
	uint32_t source_node, start;

	if (role == NULL)
		return 0;
	source_node = lookup_word_node(report, word);
	*role = p->exec_table_cell_payload ?
	    MANT_MANDOC_ATOM_TABLE_CELL_PAYLOAD :
	    source_node != MANT_MANDOC_EXEC_NONE ?
	    MANT_MANDOC_ATOM_AUTHORED : node == NULL ?
	    MANT_MANDOC_ATOM_DEVICE_GENERATED : MANT_MANDOC_ATOM_MACRO_GENERATED;
	if (report->word_active || report->words_count > UINT32_MAX ||
	    report->atoms_count > UINT32_MAX ||
	    !charge_work(report, length + 1) ||
	    !append_pool(report, word, length, &start) ||
	    !charge_record(report) ||
	    !reserve_words(report, report->words_count + 1))
		return 0;
	report->current_word_start = start;
	report->current_word_length = (uint32_t)length;
	report->current_word_node = source_node != MANT_MANDOC_EXEC_NONE ?
	    source_node : lookup_node(report, node);
	record = &report->words[report->words_count];
	memset(record, 0, sizeof(*record));
	record->key = (uint32_t)report->words_count++;
	record->node = report->current_word_node;
	record->source = record->node == MANT_MANDOC_EXEC_NONE ? 0 :
	    report->nodes[record->node].source;
	record->operand_start = start;
	record->operand_length = (uint32_t)length;
	record->role = (uint32_t)*role;
	record->wrapper = report->current_wrapper;
	record->enter_atom = (uint32_t)report->atoms_count;
	record->leave_atom = MANT_MANDOC_EXEC_NONE;
	record->enter_sequence = report->sequence++;
	record->leave_sequence = record->enter_sequence;
	report->current_word = record->key;
	report->word_active = 1;
	return 1;
}

static int
execution_word_end(void *arg, const struct termp *p,
    const struct roff_node *node)
{
	struct mant_mandoc_execution_report *report = arg;
	struct mant_mandoc_word_record *record;

	(void)p;
	(void)node;
	if (!report->word_active ||
	    report->current_word == MANT_MANDOC_EXEC_NONE ||
	    report->current_word >= report->words_count ||
	    report->atoms_count > UINT32_MAX || !charge_work(report, 1))
		return 0;
	record = &report->words[report->current_word];
	record->leave_atom = (uint32_t)report->atoms_count;
	record->leave_sequence = report->sequence++;
	report->word_active = 0;
	report->current_word_start = 0;
	report->current_word_length = 0;
	report->current_word_node = MANT_MANDOC_EXEC_NONE;
	report->current_word = MANT_MANDOC_EXEC_NONE;
	return 1;
}

static int
execution_buffer_write(void *arg, const struct termp *p,
    const struct roff_node *node, size_t slot, int scalar,
    int stored, int role)
{
	struct mant_mandoc_execution_report *report = arg;
	struct mant_mandoc_atom_record *record;
	struct buffer_origin *buffer;
	size_t buffer_key;
	uint32_t replaced;
	int64_t width_bu;

	if (!size_to_report_i64(report, (*p->getwidth)(p, scalar), &width_bu) ||
	    !charge_work(report, 1))
		return 0;
	if (role < MANT_MANDOC_ATOM_AUTHORED ||
	    role > MANT_MANDOC_ATOM_TABLE_CELL_PAYLOAD) {
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
		    "native execution reported an unknown atom role");
		return 0;
	}
	buffer_key = (size_t)(p->tcol - p->tcols);
	if (slot == SIZE_MAX || !ensure_buffer(report, buffer_key, slot + 1) ||
	    !note_buffer_extent(report, buffer_key, slot + 1))
		return 0;
	buffer = &report->buffers[buffer_key];
	if (!stored)
		return 1;
	if (!charge_record(report) ||
	    !reserve_atoms(report, report->atoms_count + 1))
		return 0;
	replaced = buffer->atoms[slot];
	record = &report->atoms[report->atoms_count];
	memset(record, 0, sizeof(*record));
	record->key = (uint32_t)report->atoms_count++;
	record->buffer = (uint32_t)buffer_key;
	record->generation = buffer->generation;
	record->buffer_generation = buffer->generation_record;
	record->slot = (uint32_t)slot;
	record->kind = atom_kind(scalar);
	record->role = (uint32_t)role;
	record->input_scalar = (uint32_t)scalar;
	record->display_scalar = (uint32_t)scalar;
	record->width_bu = width_bu;
	record->node = report->word_active ? report->current_word_node :
	    lookup_node(report, node);
	record->source = record->node != MANT_MANDOC_EXEC_NONE ?
	    report->nodes[record->node].source : 0;
	record->operand_start = report->word_active ?
	    report->current_word_start : MANT_MANDOC_EXEC_NONE;
	record->operand_length = report->word_active ?
	    report->current_word_length : 0;
	record->font = stable_font(p->fontq[p->fonti]);
	record->wrapper = report->current_wrapper;
	record->replaced_by = MANT_MANDOC_EXEC_NONE;
	record->disposition = MANT_MANDOC_ATOM_BUFFERED;
	record->sequence = report->sequence++;
	if (replaced != MANT_MANDOC_EXEC_NONE && replaced < report->atoms_count) {
		report->atoms[replaced].replaced_by = record->key;
		report->atoms[replaced].disposition = MANT_MANDOC_ATOM_REPLACED;
	}
	buffer->atoms[slot] = record->key;
	return 1;
}

static int
execution_buffer_reserve(void *arg, const struct termp *p,
    const struct roff_node *node, size_t before, size_t after)
{
	struct mant_mandoc_execution_report *report = arg;
	uint64_t added;
	size_t key;

	(void)node;
	if (after < before) {
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
		    "native execution buffer capacity regressed");
		return 0;
	}
	added = (uint64_t)(after - before);
	if (added > report->limits.max_buffer_cells - report->buffer_cells) {
		fail_report(report, MANT_MANDOC_EXECUTION_BUDGET,
		    "native execution working-buffer limit exceeded");
		return 0;
	}
	if (!charge_work(report, added))
		return 0;
	report->buffer_cells += added;
	key = (size_t)(p->tcol - p->tcols);
	return ensure_buffer(report, key, after);
}

static int
execution_buffer_rewrite(void *arg, const struct termp *p,
    const struct roff_node *node, size_t slot, int scalar)
{
	struct mant_mandoc_execution_report *report = arg;
	struct buffer_origin *buffer;
	size_t key;
	uint32_t atom;
	int64_t width_bu;

	(void)node;
	if (!size_to_report_i64(report, (*p->getwidth)(p, scalar), &width_bu) ||
	    !charge_work(report, 1))
		return 0;
	key = (size_t)(p->tcol - p->tcols);
	if (key >= report->buffer_count ||
	    slot >= report->buffers[key].capacity) {
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
		    "native execution rewrote an unknown buffer slot");
		return 0;
	}
	buffer = &report->buffers[key];
	atom = buffer->atoms[slot];
	if (atom == MANT_MANDOC_EXEC_NONE || atom >= report->atoms_count) {
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
		    "native execution rewrote a slot without provenance");
		return 0;
	}
	report->atoms[atom].display_scalar = (uint32_t)scalar;
	report->atoms[atom].kind = atom_kind(scalar);
	report->atoms[atom].width_bu = width_bu;
	return 1;
}

static int
execution_buffer_discard(void *arg, const struct termp *p,
    const struct roff_node *node, size_t start, size_t end, int disposition)
{
	struct mant_mandoc_execution_report *report = arg;
	struct buffer_origin *buffer;
	size_t key, slot;
	uint32_t atom;

	(void)node;
	if (disposition < MANT_MANDOC_ATOM_EMITTED ||
	    disposition > MANT_MANDOC_ATOM_TRAILING_DISCARD || start > end ||
	    !charge_work(report, (uint64_t)(end - start)))
		return 0;
	key = (size_t)(p->tcol - p->tcols);
	if (key >= report->buffer_count || end > report->buffers[key].capacity) {
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
		    "native execution discarded an unknown buffer range");
		return 0;
	}
	buffer = &report->buffers[key];
	for (slot = start; slot < end; slot++) {
		atom = buffer->atoms[slot];
		if (atom == MANT_MANDOC_EXEC_NONE)
			continue;
		if (atom >= report->atoms_count) {
			fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
			    "native execution buffer provenance is invalid");
			return 0;
		}
		if (report->atoms[atom].disposition ==
		    MANT_MANDOC_ATOM_BUFFERED)
			report->atoms[atom].disposition = (uint32_t)disposition;
		if (disposition != MANT_MANDOC_ATOM_EMITTED)
			buffer->atoms[slot] = MANT_MANDOC_EXEC_NONE;
	}
	return 1;
}

static int
execution_buffer_reset(void *arg, const struct termp *p,
    const struct roff_node *node)
{
	struct mant_mandoc_execution_report *report = arg;
	struct buffer_origin *buffer;
	size_t key, slot;
	uint32_t atom;

	(void)node;
	key = (size_t)(p->tcol - p->tcols);
	if (key >= report->buffer_count)
		return charge_work(report, 1);
	buffer = &report->buffers[key];
	if (p->tcol->lastcol > buffer->capacity ||
	    !charge_work(report, (uint64_t)p->tcol->lastcol + 1))
		return 0;
	if (!close_buffer_generation(report, key, p->tcol->lastcol,
	    MANT_MANDOC_BUFFER_RESET))
		return 0;
	for (slot = 0; slot < p->tcol->lastcol; slot++) {
		atom = buffer->atoms[slot];
		if (atom != MANT_MANDOC_EXEC_NONE && atom < report->atoms_count &&
		    report->atoms[atom].disposition == MANT_MANDOC_ATOM_BUFFERED)
			report->atoms[atom].disposition =
			    MANT_MANDOC_ATOM_TRAILING_DISCARD;
		buffer->atoms[slot] = MANT_MANDOC_EXEC_NONE;
	}
	if (buffer->generation == UINT32_MAX) {
		fail_report(report, MANT_MANDOC_EXECUTION_BUDGET,
		    "native execution buffer generation limit exceeded");
		return 0;
	}
	buffer->generation++;
	buffer->generation_record = MANT_MANDOC_EXEC_NONE;
	return 1;
}

static int
execution_flush_begin(void *arg, const struct termp *p,
    const struct roff_node *node)
{
	struct mant_mandoc_execution_report *report = arg;

	(void)p;
	(void)node;
	report->current_flush = MANT_MANDOC_EXEC_NONE;
	return charge_work(report, 1);
}

static int
execution_fill_scan(void *arg, const struct termp *p,
    const struct roff_node *node, size_t slot)
{
	(void)p;
	(void)node;
	(void)slot;
	return charge_work(arg, 1);
}

static int
execution_fill_decision(void *arg, const struct termp *p,
    const struct roff_node *node, size_t scan_end, size_t accepted,
    size_t content_width, size_t target_width)
{
	struct mant_mandoc_execution_report *report = arg;
	struct mant_mandoc_flush_record *record;
	size_t buffer_key;
	int64_t content_bu, target_bu, taboff_bu, visual_bu;

	if (!size_to_report_i64(report, content_width, &content_bu) ||
	    !size_to_report_i64(report, target_width, &target_bu) ||
	    !size_to_report_i64(report, p->tcol->taboff, &taboff_bu) ||
	    !size_to_report_i64(report, p->viscol, &visual_bu) ||
	    !charge_record(report) ||
	    !reserve_flushes(report, report->flushes_count + 1))
		return 0;
	buffer_key = (size_t)(p->tcol - p->tcols);
	if (!note_buffer_extent(report, buffer_key, p->tcol->lastcol))
		return 0;
	record = &report->flushes[report->flushes_count];
	memset(record, 0, sizeof(*record));
	record->key = (uint32_t)report->flushes_count++;
	record->node = lookup_node(report, node);
	record->buffer = (uint32_t)buffer_key;
	record->generation = buffer_key < report->buffer_count ?
	    report->buffers[buffer_key].generation : 0;
	record->buffer_generation = buffer_key < report->buffer_count ?
	    report->buffers[buffer_key].generation_record :
	    MANT_MANDOC_EXEC_NONE;
	record->scan_start = (uint32_t)p->tcol->col;
	record->scan_end = (uint32_t)scan_end;
	record->accepted_start = (uint32_t)p->tcol->col;
	record->accepted_end = (uint32_t)accepted;
	record->consumed_start = (uint32_t)p->tcol->col;
	record->consumed_end = (uint32_t)p->tcol->col;
	record->tail_discarded_start = (uint32_t)accepted;
	record->tail_discarded_end = (uint32_t)accepted;
	record->remaining_start = (uint32_t)accepted;
	record->remaining_end = (uint32_t)p->tcol->lastcol;
	record->fragment_start = (uint32_t)report->fragments_count;
	record->fragment_length = 0;
	record->flags_before = stable_term_flags(p->flags);
	record->flags_after = record->flags_before;
	record->boundary = report->current_boundary;
	record->content_bu = content_bu;
	record->target_bu = target_bu;
	record->taboff_before = taboff_bu;
	record->taboff_after = taboff_bu;
	record->visual_before = visual_bu;
	record->visual_after = visual_bu;
	record->sequence = report->sequence++;
	record->outcome_sequence = UINT64_MAX;
	report->current_flush = record->key;
	return 1;
}

static int
execution_fill_outcome(void *arg, const struct termp *p,
    const struct roff_node *node, int outcome)
{
	struct mant_mandoc_execution_report *report = arg;
	struct mant_mandoc_flush_record *record;
	int64_t taboff_bu, visual_bu;

	(void)p;
	(void)node;
	if (!size_to_report_i64(report, p->tcol->taboff, &taboff_bu) ||
	    !size_to_report_i64(report, p->viscol, &visual_bu) ||
	    !charge_work(report, 1) ||
	    report->current_flush == MANT_MANDOC_EXEC_NONE ||
	    report->current_flush >= report->flushes_count ||
	    outcome < MANT_MANDOC_FLUSH_NO_CONTENT ||
	    outcome > MANT_MANDOC_FLUSH_DEFERRED_COLUMN)
		return 0;
	record = &report->flushes[report->current_flush];
	if (record->outcome != 0 || record->outcome_sequence != UINT64_MAX) {
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
		    "native execution reported a duplicate flush outcome");
		return 0;
	}
	record->outcome = (uint32_t)outcome;
	record->tail_discarded_start = record->accepted_end;
	if (outcome == MANT_MANDOC_FLUSH_NO_CONTENT ||
	    outcome == MANT_MANDOC_FLUSH_EXHAUSTED) {
		record->tail_discarded_end = record->remaining_end;
		record->remaining_start = record->remaining_end;
	} else {
		record->tail_discarded_end = (uint32_t)p->tcol->col;
		record->remaining_start = (uint32_t)p->tcol->col;
	}
	record->taboff_after = taboff_bu;
	record->visual_after = visual_bu;
	record->flags_after = stable_term_flags(p->flags);
	record->outcome_sequence = report->sequence++;
	return 1;
}

static int
execution_field_begin(void *arg, const struct termp *p,
    const struct roff_node *node, size_t leading, size_t accepted,
    size_t visual)
{
	struct mant_mandoc_execution_report *report = arg;
	struct mant_mandoc_flush_record *record;
	size_t available;
	int64_t field_bu, leading_bu, visual_bu;

	(void)node;
	available = p->tcol->rmargin > visual ?
	    p->tcol->rmargin - visual : 0;
	available = available > leading ? available - leading : 0;
	if (!size_to_report_i64(report, leading, &leading_bu) ||
	    !size_to_report_i64(report, available, &field_bu) ||
	    !size_to_report_i64(report, visual, &visual_bu) ||
	    !charge_work(report, 1) ||
	    report->current_flush == MANT_MANDOC_EXEC_NONE ||
	    report->current_flush >= report->flushes_count)
		return 0;
	record = &report->flushes[report->current_flush];
	record->leading_bu = leading_bu;
	record->field_bu = field_bu;
	record->accepted_end = (uint32_t)accepted;
	record->fragment_start = (uint32_t)report->fragments_count;
	record->visual_before = visual_bu;
	return 1;
}

static int
execution_field_atom(void *arg, const struct termp *p,
    const struct roff_node *node, size_t slot, int scalar)
{
	(void)p;
	(void)node;
	(void)slot;
	(void)scalar;
	return charge_work(arg, 1);
}

static int
execution_field_end(void *arg, const struct termp *p,
    const struct roff_node *node, size_t leading, size_t accepted,
    size_t visual)
{
	struct mant_mandoc_execution_report *report = arg;
	struct mant_mandoc_flush_record *record;
	int64_t taboff_bu, visual_bu;

	(void)node;
	(void)leading;
	if (!size_to_report_i64(report, p->tcol->taboff, &taboff_bu) ||
	    !size_to_report_i64(report, visual, &visual_bu) ||
	    !charge_work(report, 1) ||
	    report->current_flush == MANT_MANDOC_EXEC_NONE ||
	    report->current_flush >= report->flushes_count)
		return 0;
	record = &report->flushes[report->current_flush];
	record->consumed_end = (uint32_t)accepted;
	record->remaining_start = (uint32_t)accepted;
	record->fragment_length = (uint32_t)(report->fragments_count -
	    record->fragment_start);
	record->visual_after = visual_bu;
	record->taboff_after = taboff_bu;
	record->flags_after = stable_term_flags(p->flags);
	return 1;
}

static int
execution_flush_end(void *arg, const struct termp *p,
    const struct roff_node *node)
{
	struct mant_mandoc_execution_report *report = arg;
	size_t buffer_key;

	(void)node;
	if (!charge_work(report, 1))
		return 0;
	/*
	 * A flush record describes one term_fill()/term_field() segment.
	 * execution_fill_outcome() is its only after-state checkpoint.  In the
	 * single-column terminal path, term_flushln() calls endline() after that
	 * outcome and before reaching this hook; overwriting the record here
	 * would therefore give terminal segments a different time point from
	 * wrapped and deferred segments.
	 */
	buffer_key = (size_t)(p->tcol - p->tcols);
	(void)buffer_key;
	report->current_flush = MANT_MANDOC_EXEC_NONE;
	return 1;
}

static int
execution_boundary_enter(void *arg, const struct termp *p,
    const struct roff_node *node, int request)
{
	uint32_t stable;

	switch (request) {
	case 1: stable = MANT_MANDOC_BOUNDARY_NEWLINE; break;
	case 2: stable = MANT_MANDOC_BOUNDARY_VERTICAL_SPACE; break;
	case 3: stable = MANT_MANDOC_BOUNDARY_ENDLINE; break;
	default:
		fail_report(arg, MANT_MANDOC_EXECUTION_INTERNAL,
		    "native execution reported an unknown boundary request");
		return 0;
	}
	return append_boundary(arg, p, node, stable,
	    MANT_MANDOC_BOUNDARY_NO_OUTPUT);
}

static int
execution_boundary_leave(void *arg, const struct termp *p,
    const struct roff_node *node, int request)
{
	struct mant_mandoc_execution_report *report = arg;
	struct mant_mandoc_boundary_record *record;
	uint32_t expected;
	int64_t line_after, visual_after;

	(void)node;
	expected = request == 1 ? MANT_MANDOC_BOUNDARY_NEWLINE :
	    request == 2 ? MANT_MANDOC_BOUNDARY_VERTICAL_SPACE :
	    request == 3 ? MANT_MANDOC_BOUNDARY_ENDLINE : MANT_MANDOC_EXEC_NONE;
	if (!size_to_report_i64(report, p->line, &line_after) ||
	    !size_to_report_i64(report, p->viscol, &visual_after) ||
	    !charge_work(report, 1) ||
	    report->current_boundary == MANT_MANDOC_EXEC_NONE ||
	    report->current_boundary >= report->boundaries_count)
		return 0;
	record = &report->boundaries[report->current_boundary];
	if (record->request != expected) {
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
		    "native execution boundary stack is unbalanced");
		return 0;
	}
	record->flags_after = stable_term_flags(p->flags);
	record->line_after = line_after;
	record->visual_after = visual_after;
	if (request == 2 && record->direct_device_lines != 0)
		record->effect = MANT_MANDOC_BOUNDARY_ADDED_VERTICAL_SPACE;
	else if (record->line_after > record->line_before)
		record->effect = MANT_MANDOC_BOUNDARY_ENDED_LINE;
	else if (record->visual_after != record->visual_before)
		record->effect = MANT_MANDOC_BOUNDARY_FLUSHED;
	record->leave_sequence = report->sequence++;
	report->current_boundary = record->parent;
	return 1;
}

static int
execution_control_enter(void *arg, const struct termp *p,
    const struct roff_node *node)
{
	struct mant_mandoc_execution_report *report = arg;
	struct mant_mandoc_control_record *record;
	uint32_t origin, request;

	request = node == NULL ? MANT_MANDOC_EXEC_NONE :
	    stable_control_request(node->tok);
	if (request == MANT_MANDOC_EXEC_NONE ||
	    report->controls_count > UINT32_MAX ||
	    report->atoms_count > UINT32_MAX ||
	    report->fragments_count > UINT32_MAX ||
	    report->flushes_count > UINT32_MAX ||
	    report->boundaries_count > UINT32_MAX ||
	    report->geometries_count > UINT32_MAX ||
	    report->wrappers_count > UINT32_MAX ||
	    !charge_work(report, 1) || !charge_record(report) ||
	    !reserve_controls(report, report->controls_count + 1))
		return 0;
	record = &report->controls[report->controls_count];
	memset(record, 0, sizeof(*record));
	record->key = (uint32_t)report->controls_count++;
	record->node = lookup_node(report, node);
	record->parent = report->current_control;
	record->wrapper = report->current_wrapper;
	record->request = request;
	if (record->node == MANT_MANDOC_EXEC_NONE ||
	    record->wrapper == MANT_MANDOC_EXEC_NONE ||
	    record->wrapper >= report->wrappers_count ||
	    report->wrappers[record->wrapper].kind != MANT_MANDOC_WRAPPER_NODE) {
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
		    "native control request has no matching node wrapper");
		return 0;
	}
	origin = record->node;
	while (origin != MANT_MANDOC_EXEC_NONE &&
	    origin != report->wrappers[record->wrapper].node)
		origin = report->nodes[origin].parent;
	if (origin != report->wrappers[record->wrapper].node) {
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
		    "native control request escapes its node wrapper");
		return 0;
	}
	record->atom_start = (uint32_t)report->atoms_count;
	record->fragment_start = (uint32_t)report->fragments_count;
	record->flush_start = (uint32_t)report->flushes_count;
	record->boundary_start = (uint32_t)report->boundaries_count;
	record->geometry_start = (uint32_t)report->geometries_count;
	record->wrapper_start = (uint32_t)report->wrappers_count;
	record->enter_sequence = report->sequence++;
	record->leave_sequence = UINT64_MAX;
	if (!capture_control_state(report, p, record, 0))
		return 0;
	report->current_control = record->key;
	return 1;
}

static int
execution_control_leave(void *arg, const struct termp *p,
    const struct roff_node *node)
{
	struct mant_mandoc_execution_report *report = arg;
	struct mant_mandoc_control_record *record;

	if (!charge_work(report, 1) ||
	    report->current_control == MANT_MANDOC_EXEC_NONE ||
	    report->current_control >= report->controls_count ||
	    report->atoms_count > UINT32_MAX ||
	    report->fragments_count > UINT32_MAX ||
	    report->flushes_count > UINT32_MAX ||
	    report->boundaries_count > UINT32_MAX ||
	    report->geometries_count > UINT32_MAX ||
	    report->wrappers_count > UINT32_MAX)
		return 0;
	record = &report->controls[report->current_control];
	if (record->node != lookup_node(report, node) ||
	    record->request != stable_control_request(node->tok) ||
	    record->leave_sequence != UINT64_MAX)
		return 0;
	record->atom_length = (uint32_t)report->atoms_count - record->atom_start;
	record->fragment_length =
	    (uint32_t)report->fragments_count - record->fragment_start;
	record->flush_length =
	    (uint32_t)report->flushes_count - record->flush_start;
	record->boundary_length =
	    (uint32_t)report->boundaries_count - record->boundary_start;
	record->geometry_length =
	    (uint32_t)report->geometries_count - record->geometry_start;
	record->wrapper_length =
	    (uint32_t)report->wrappers_count - record->wrapper_start;
	if (!capture_control_state(report, p, record, 1))
		return 0;
	record->leave_sequence = report->sequence++;
	report->current_control = record->parent;
	return 1;
}

static int
execution_device_advance(void *arg, const struct termp *p,
    const struct roff_node *node, size_t requested, size_t before,
    size_t after)
{
	struct mant_mandoc_execution_report *report = arg;
	struct mant_mandoc_geometry_record *record;
	int64_t requested_bu, effective_bu, before_bu, after_bu;
	uint32_t flush_origin;

	if (!size_to_report_i64(report, requested, &requested_bu) ||
	    !size_delta_to_report_i64(report, before, after, &effective_bu) ||
	    !size_to_report_i64(report, before, &before_bu) ||
	    !size_to_report_i64(report, after, &after_bu) ||
	    !charge_work(report, (after >= before ? after - before : 0) / 24 + 1) ||
	    !charge_record(report) ||
	    !reserve_geometries(report, report->geometries_count + 1))
		return 0;
	record = &report->geometries[report->geometries_count];
	memset(record, 0, sizeof(*record));
	flush_origin = report->current_flush != MANT_MANDOC_EXEC_NONE &&
	    report->current_flush < report->flushes_count &&
	    report->flushes[report->current_flush].outcome_sequence == UINT64_MAX ?
	    report->current_flush : MANT_MANDOC_EXEC_NONE;
	record->key = (uint32_t)report->geometries_count++;
	record->node = lookup_node(report, node);
	record->related = flush_origin;
	record->kind = MANT_MANDOC_GEOMETRY_ADVANCE;
	record->unit = MANT_MANDOC_UNIT_BASIC;
	record->requested = requested_bu;
	record->effective = effective_bu;
	record->before = before_bu;
	record->after = after_bu;
	record->origin_kind = flush_origin == MANT_MANDOC_EXEC_NONE ?
	    MANT_MANDOC_GEOMETRY_ORIGIN_NONE :
	    MANT_MANDOC_GEOMETRY_ORIGIN_FLUSH;
	record->origin_key = flush_origin;
	record->sequence = report->sequence++;
	return 1;
}

static int
execution_device_letter(void *arg, const struct termp *p,
    const struct roff_node *node, size_t slot, int scalar, size_t before,
    size_t after)
{
	return append_fragment(arg, p, node, scalar, slot, before, after);
}

static int
execution_device_endline(void *arg, const struct termp *p,
    const struct roff_node *node, size_t line_before, size_t line_after,
    size_t visual_before, size_t visual_after)
{
	struct mant_mandoc_execution_report *report = arg;
	struct mant_mandoc_geometry_record *record;
	struct mant_mandoc_boundary_record *boundary;
	uint32_t parent;
	int64_t line_before_value, line_after_value;
	int64_t visual_before_value, visual_after_value;

	parent = report->current_boundary;
	if (!size_to_report_i64(report, line_before, &line_before_value) ||
	    !size_to_report_i64(report, line_after, &line_after_value) ||
	    !size_to_report_i64(report, visual_before, &visual_before_value) ||
	    !size_to_report_i64(report, visual_after, &visual_after_value) ||
	    !append_boundary(report, p, node,
	    MANT_MANDOC_BOUNDARY_DEVICE_ENDLINE,
	    MANT_MANDOC_BOUNDARY_ENDED_LINE))
		return 0;
	boundary = &report->boundaries[report->current_boundary];
	boundary->line_before = line_before_value;
	boundary->line_after = line_after_value;
	boundary->visual_before = visual_before_value;
	boundary->visual_after = visual_after_value;
	boundary->flags_after = stable_term_flags(p->flags);
	if (boundary->parent != MANT_MANDOC_EXEC_NONE &&
	    boundary->parent < report->boundaries_count) {
		if (report->boundaries[boundary->parent].direct_device_lines ==
		    UINT32_MAX) {
			fail_report(report, MANT_MANDOC_EXECUTION_BUDGET,
			    "native execution boundary line count overflowed");
			return 0;
		}
		report->boundaries[boundary->parent].direct_device_lines++;
	}
	if (!charge_work(report, 1) || !charge_record(report) ||
	    !reserve_geometries(report, report->geometries_count + 1))
		return 0;
	record = &report->geometries[report->geometries_count];
	memset(record, 0, sizeof(*record));
	record->key = (uint32_t)report->geometries_count++;
	record->node = lookup_node(report, node);
	record->related = boundary->key;
	record->kind = MANT_MANDOC_GEOMETRY_ENDLINE;
	record->unit = MANT_MANDOC_UNIT_DEVICE_LINE;
	record->requested = 1;
	record->effective = 1;
	record->origin_kind = MANT_MANDOC_GEOMETRY_ORIGIN_BOUNDARY;
	record->origin_key = boundary->key;
	record->before = line_before_value;
	record->after = line_after_value;
	record->sequence = report->sequence++;
	boundary->leave_sequence = report->sequence++;
	report->current_boundary = parent;
	(void)p;
	(void)visual_before;
	return 1;
}

static int
execution_font(void *arg, const struct termp *p,
    const struct roff_node *node, int before, int after,
    size_t depth_before, size_t depth_after)
{
	struct mant_mandoc_execution_report *report = arg;
	struct mant_mandoc_wrapper_record *record;

	(void)p;
	if (!charge_work(report, 1) || !charge_record(report) ||
	    !reserve_wrappers(report, report->wrappers_count + 1))
		return 0;
	record = &report->wrappers[report->wrappers_count];
	memset(record, 0, sizeof(*record));
	record->key = (uint32_t)report->wrappers_count++;
	record->parent = report->current_wrapper;
	record->node = lookup_node(report, node);
	record->kind = MANT_MANDOC_WRAPPER_FONT;
	record->target_start = MANT_MANDOC_EXEC_NONE;
	record->enter_atom = (uint32_t)report->atoms_count;
	record->leave_atom = record->enter_atom;
	record->state_before = stable_font(before);
	record->state_after = stable_font(after);
	record->depth_before = (uint32_t)depth_before;
	record->depth_after = (uint32_t)depth_after;
	record->enter_sequence = report->sequence++;
	record->leave_sequence = record->enter_sequence;
	return 1;
}

static int
execution_reference_begin(void *arg, const struct termp *p,
    const struct roff_node *current, const struct roff_node *owner,
    const struct roff_node *target_node, int kind, const char *primary,
    size_t primary_length, const char *secondary, size_t secondary_length,
    int affinity)
{
	struct mant_mandoc_execution_report *report = arg;
	struct mant_mandoc_reference_record *record;
	uint32_t stable_kind, stable_affinity, primary_start, secondary_start;
	uint32_t owner_key, target_key;

	(void)p;
	(void)current;
	switch (kind) {
	case TERM_EXEC_REFERENCE_EXTERNAL_URI:
		stable_kind = MANT_MANDOC_REFERENCE_EXTERNAL_URI;
		break;
	case TERM_EXEC_REFERENCE_EMAIL:
		stable_kind = MANT_MANDOC_REFERENCE_EMAIL;
		break;
	case TERM_EXEC_REFERENCE_MANUAL:
		stable_kind = MANT_MANDOC_REFERENCE_MANUAL;
		break;
	case TERM_EXEC_REFERENCE_SECTION:
		stable_kind = MANT_MANDOC_REFERENCE_SECTION;
		break;
	default:
		return 0;
	}
	switch (affinity) {
	case TERM_EXEC_AFFINITY_INLINE:
		stable_affinity = MANT_MANDOC_AFFINITY_INLINE;
		break;
	case TERM_EXEC_AFFINITY_BEFORE_OUTPUT:
		stable_affinity = MANT_MANDOC_AFFINITY_BEFORE_OUTPUT;
		break;
	default:
		return 0;
	}
	owner_key = lookup_node(report, owner);
	target_key = lookup_node(report, target_node);
	if (primary == NULL || owner_key == MANT_MANDOC_EXEC_NONE ||
	    target_key == MANT_MANDOC_EXEC_NONE ||
	    report->atoms_count > UINT32_MAX ||
	    primary_length > UINT32_MAX || secondary_length > UINT32_MAX ||
	    (secondary == NULL && secondary_length != 0) ||
	    !charge_work(report, primary_length) ||
	    !charge_work(report, 1) ||
	    !charge_work(report, secondary_length) ||
	    !append_pool(report, primary, primary_length, &primary_start) ||
	    (secondary != NULL && !append_pool(report, secondary,
	    secondary_length, &secondary_start)) ||
	    !charge_record(report) ||
	    !reserve_references(report, report->references_count + 1))
		return 0;
	record = &report->references[report->references_count];
	memset(record, 0, sizeof(*record));
	record->key = (uint32_t)report->references_count++;
	record->parent = report->current_reference;
	record->owner_node = owner_key;
	record->target_node = target_key;
	record->kind = stable_kind;
	record->primary_start = primary_start;
	record->primary_length = (uint32_t)primary_length;
	record->secondary_start = secondary == NULL ?
	    MANT_MANDOC_EXEC_NONE : secondary_start;
	record->secondary_length = (uint32_t)secondary_length;
	record->enter_atom = (uint32_t)report->atoms_count;
	record->label_start_atom = record->enter_atom;
	record->leave_atom = MANT_MANDOC_EXEC_NONE;
	record->affinity = stable_affinity;
	record->enter_sequence = report->sequence++;
	record->leave_sequence = UINT64_MAX;
	report->current_reference = record->key;
	return 1;
}

static int
execution_reference_end(void *arg, const struct termp *p,
    const struct roff_node *current)
{
	struct mant_mandoc_execution_report *report = arg;
	struct mant_mandoc_reference_record *record;

	(void)p;
	(void)current;
	if (!charge_work(report, 1) ||
	    report->current_reference == MANT_MANDOC_EXEC_NONE ||
	    report->current_reference >= report->references_count ||
	    report->atoms_count > UINT32_MAX)
		return 0;
	record = &report->references[report->current_reference];
	if (record->leave_atom != MANT_MANDOC_EXEC_NONE)
		return 0;
	record->leave_atom = (uint32_t)report->atoms_count;
	while (record->label_start_atom < record->leave_atom &&
	    report->atoms[record->label_start_atom].role ==
	    MANT_MANDOC_ATOM_IMPLICIT_SPACE)
		record->label_start_atom++;
	record->leave_sequence = report->sequence++;
	report->current_reference = record->parent;
	return 1;
}

static int
execution_anchor(void *arg, const struct termp *p,
    const struct roff_node *node, const char *target, size_t target_length,
    size_t device_line, int affinity)
{
	struct mant_mandoc_execution_report *report = arg;
	struct mant_mandoc_anchor_record *record;
	uint32_t target_start, stable_affinity;

	(void)p;
	if (affinity != TERM_EXEC_AFFINITY_BEFORE_OUTPUT || target == NULL ||
	    target_length > UINT32_MAX || device_line == 0 ||
	    device_line > UINT32_MAX || report->atoms_count > UINT32_MAX ||
	    report->fragments_count > UINT32_MAX ||
	    lookup_node(report, node) == MANT_MANDOC_EXEC_NONE)
		return 0;
	stable_affinity = MANT_MANDOC_AFFINITY_BEFORE_OUTPUT;
	if (!charge_work(report, target_length) ||
	    !charge_work(report, 1) ||
	    !append_pool(report, target, target_length, &target_start) ||
	    !charge_record(report) ||
	    !reserve_anchors(report, report->anchors_count + 1))
		return 0;
	record = &report->anchors[report->anchors_count];
	memset(record, 0, sizeof(*record));
	record->key = (uint32_t)report->anchors_count++;
	record->node = lookup_node(report, node);
	record->target_start = target_start;
	record->target_length = (uint32_t)target_length;
	record->device_line = (uint32_t)device_line;
	record->atom_cursor = (uint32_t)report->atoms_count;
	record->fragment_cursor = (uint32_t)report->fragments_count;
	record->affinity = stable_affinity;
	record->sequence = report->sequence++;
	return 1;
}

static int
execution_table_preflight(void *arg, const struct termp *p,
    const struct roff_node *node, size_t amount)
{
	(void)p;
	(void)node;
	return charge_work(arg, amount);
}

static int
execution_table_begin(void *arg, const struct termp *p,
    const struct roff_node *node, const struct tbl_span *span, size_t rows,
    size_t layout_cells, size_t data_cells)
{
	struct mant_mandoc_execution_report *report = arg;
	struct mant_mandoc_table_record *record;

	(void)p;
	if (span == NULL || span->opts == NULL || rows == 0 ||
	    rows > UINT32_MAX || layout_cells > UINT32_MAX ||
	    data_cells > UINT32_MAX || report->current_table !=
	    MANT_MANDOC_EXEC_NONE || report->atoms_count > UINT32_MAX ||
	    report->fragments_count > UINT32_MAX ||
	    report->flushes_count > UINT32_MAX ||
	    !charge_record(report) ||
	    !reserve_tables(report, report->tables_count + 1))
		return 0;
	record = &report->tables[report->tables_count];
	memset(record, 0, sizeof(*record));
	record->key = (uint32_t)report->tables_count++;
	record->first_row_node = lookup_node(report, node);
	record->row_start = (uint32_t)report->table_rows_count;
	record->row_length = (uint32_t)rows;
	record->cell_start = (uint32_t)report->table_cells_count;
	record->cell_length = (uint32_t)data_cells;
	record->logical_columns = (uint32_t)span->opts->cols;
	record->enter_atom = (uint32_t)report->atoms_count;
	record->leave_atom = MANT_MANDOC_EXEC_NONE;
	record->enter_fragment = (uint32_t)report->fragments_count;
	record->leave_fragment = MANT_MANDOC_EXEC_NONE;
	record->enter_flush = (uint32_t)report->flushes_count;
	record->leave_flush = MANT_MANDOC_EXEC_NONE;
	record->enter_sequence = report->sequence++;
	record->leave_sequence = UINT64_MAX;
	report->current_table = record->key;
	return 1;
}

static int
execution_table_end(void *arg, const struct termp *p,
    const struct roff_node *node, const struct tbl_span *span)
{
	struct mant_mandoc_execution_report *report = arg;
	struct mant_mandoc_table_record *record;

	(void)p;
	(void)node;
	if (span == NULL || span->next != NULL || report->current_table ==
	    MANT_MANDOC_EXEC_NONE || report->current_table >=
	    report->tables_count || report->current_table_row !=
	    MANT_MANDOC_EXEC_NONE || report->current_table_cell !=
	    MANT_MANDOC_EXEC_NONE || !charge_work(report, 1) ||
	    report->atoms_count > UINT32_MAX ||
	    report->fragments_count > UINT32_MAX ||
	    report->flushes_count > UINT32_MAX)
		return 0;
	record = &report->tables[report->current_table];
	record->leave_atom = (uint32_t)report->atoms_count;
	record->leave_fragment = (uint32_t)report->fragments_count;
	record->leave_flush = (uint32_t)report->flushes_count;
	record->leave_sequence = report->sequence++;
	report->current_table = MANT_MANDOC_EXEC_NONE;
	return 1;
}

static int
execution_table_row_begin(void *arg, const struct termp *p,
    const struct roff_node *node, const struct tbl_span *span)
{
	struct mant_mandoc_execution_report *report = arg;
	struct mant_mandoc_table_record *table;
	struct mant_mandoc_table_row_record *record;
	uint32_t kind;

	(void)p;
	if (span == NULL || report->current_table == MANT_MANDOC_EXEC_NONE ||
	    report->current_table >= report->tables_count ||
	    report->current_table_row != MANT_MANDOC_EXEC_NONE ||
	    report->table_rows_count > UINT32_MAX ||
	    report->table_cells_count > UINT32_MAX ||
	    report->atoms_count > UINT32_MAX ||
	    report->fragments_count > UINT32_MAX ||
	    report->flushes_count > UINT32_MAX)
		return 0;
	switch (span->pos) {
	case TBL_SPAN_DATA: kind = MANT_MANDOC_EXEC_TABLE_ROW_DATA; break;
	case TBL_SPAN_HORIZ: kind = MANT_MANDOC_EXEC_TABLE_ROW_SINGLE_RULE; break;
	case TBL_SPAN_DHORIZ: kind = MANT_MANDOC_EXEC_TABLE_ROW_DOUBLE_RULE; break;
	default: return 0;
	}
	if (!charge_record(report) ||
	    !reserve_table_rows(report, report->table_rows_count + 1))
		return 0;
	table = &report->tables[report->current_table];
	record = &report->table_rows[report->table_rows_count];
	memset(record, 0, sizeof(*record));
	record->key = (uint32_t)report->table_rows_count++;
	record->table = table->key;
	record->node = lookup_node(report, node);
	record->ordinal = record->key - table->row_start;
	record->kind = kind;
	record->logical_columns = (uint32_t)span->opts->cols;
	record->cell_start = (uint32_t)report->table_cells_count;
	record->enter_atom = (uint32_t)report->atoms_count;
	record->leave_atom = MANT_MANDOC_EXEC_NONE;
	record->enter_fragment = (uint32_t)report->fragments_count;
	record->leave_fragment = MANT_MANDOC_EXEC_NONE;
	record->enter_flush = (uint32_t)report->flushes_count;
	record->leave_flush = MANT_MANDOC_EXEC_NONE;
	record->enter_sequence = report->sequence++;
	record->leave_sequence = UINT64_MAX;
	report->current_table_row = record->key;
	return 1;
}

static int
execution_table_row_end(void *arg, const struct termp *p,
    const struct roff_node *node, const struct tbl_span *span)
{
	struct mant_mandoc_execution_report *report = arg;
	struct mant_mandoc_table_row_record *record;

	(void)p;
	(void)node;
	(void)span;
	if (report->current_table_row == MANT_MANDOC_EXEC_NONE ||
	    report->current_table_row >= report->table_rows_count ||
	    report->current_table_cell != MANT_MANDOC_EXEC_NONE ||
	    !charge_work(report, 1) || report->atoms_count > UINT32_MAX ||
	    report->fragments_count > UINT32_MAX ||
	    report->flushes_count > UINT32_MAX)
		return 0;
	record = &report->table_rows[report->current_table_row];
	record->cell_length = (uint32_t)report->table_cells_count -
	    record->cell_start;
	record->leave_atom = (uint32_t)report->atoms_count;
	record->leave_fragment = (uint32_t)report->fragments_count;
	record->leave_flush = (uint32_t)report->flushes_count;
	record->leave_sequence = report->sequence++;
	report->current_table_row = MANT_MANDOC_EXEC_NONE;
	return 1;
}

static uint32_t
table_layout_kind(enum tbl_cellt kind)
{
	switch (kind) {
	case TBL_CELL_CENTRE: return MANT_MANDOC_EXEC_TABLE_LAYOUT_CENTER;
	case TBL_CELL_RIGHT: return MANT_MANDOC_EXEC_TABLE_LAYOUT_RIGHT;
	case TBL_CELL_LEFT: return MANT_MANDOC_EXEC_TABLE_LAYOUT_LEFT;
	case TBL_CELL_NUMBER: return MANT_MANDOC_EXEC_TABLE_LAYOUT_NUMERIC;
	case TBL_CELL_SPAN: return MANT_MANDOC_EXEC_TABLE_LAYOUT_SPAN;
	case TBL_CELL_LONG: return MANT_MANDOC_EXEC_TABLE_LAYOUT_LONG;
	case TBL_CELL_DOWN: return MANT_MANDOC_EXEC_TABLE_LAYOUT_DOWN;
	case TBL_CELL_HORIZ: return MANT_MANDOC_EXEC_TABLE_LAYOUT_SINGLE_RULE;
	case TBL_CELL_DHORIZ:
		return MANT_MANDOC_EXEC_TABLE_LAYOUT_DOUBLE_RULE;
	default: return MANT_MANDOC_EXEC_NONE;
	}
}

static uint32_t
table_data_kind(enum tbl_datt kind)
{
	switch (kind) {
	case TBL_DATA_NONE: return MANT_MANDOC_EXEC_TABLE_DATA_NONE;
	case TBL_DATA_DATA: return MANT_MANDOC_EXEC_TABLE_DATA_TEXT;
	case TBL_DATA_HORIZ: return MANT_MANDOC_EXEC_TABLE_DATA_SINGLE_RULE;
	case TBL_DATA_DHORIZ:
		return MANT_MANDOC_EXEC_TABLE_DATA_DOUBLE_RULE;
	case TBL_DATA_NHORIZ:
		return MANT_MANDOC_EXEC_TABLE_DATA_ISOLATED_SINGLE_RULE;
	case TBL_DATA_NDHORIZ:
		return MANT_MANDOC_EXEC_TABLE_DATA_ISOLATED_DOUBLE_RULE;
	default: return MANT_MANDOC_EXEC_NONE;
	}
}

static uint32_t
table_alignment(enum tbl_cellt kind)
{
	switch (kind) {
	case TBL_CELL_LEFT: return MANT_MANDOC_EXEC_TABLE_ALIGN_LEFT;
	case TBL_CELL_CENTRE: return MANT_MANDOC_EXEC_TABLE_ALIGN_CENTER;
	case TBL_CELL_RIGHT: return MANT_MANDOC_EXEC_TABLE_ALIGN_RIGHT;
	case TBL_CELL_NUMBER: return MANT_MANDOC_EXEC_TABLE_ALIGN_NUMERIC;
	case TBL_CELL_LONG: return MANT_MANDOC_EXEC_TABLE_ALIGN_LONG;
	default: return MANT_MANDOC_EXEC_TABLE_ALIGN_NONE;
	}
}

static uint32_t
table_cell_flags(const struct tbl_cell *cell, const struct tbl_dat *data)
{
	uint32_t flags = 0;

	if (cell->flags & TBL_CELL_TALIGN) flags |= MANT_MANDOC_EXEC_TABLE_CELL_TOP_ALIGN;
	if (cell->flags & TBL_CELL_UP) flags |= MANT_MANDOC_EXEC_TABLE_CELL_UP;
	if (cell->flags & TBL_CELL_BALIGN) flags |= MANT_MANDOC_EXEC_TABLE_CELL_BOTTOM_ALIGN;
	if (cell->flags & TBL_CELL_WIGN) flags |= MANT_MANDOC_EXEC_TABLE_CELL_ZERO_WIDTH;
	if (cell->flags & TBL_CELL_EQUAL) flags |= MANT_MANDOC_EXEC_TABLE_CELL_EQUAL_WIDTH;
	if (cell->flags & TBL_CELL_WMAX) flags |= MANT_MANDOC_EXEC_TABLE_CELL_MAX_WIDTH;
	if (data->block) flags |= MANT_MANDOC_EXEC_TABLE_CELL_TEXT_BLOCK;
	if (data->source_safe) flags |= MANT_MANDOC_EXEC_TABLE_CELL_SOURCE_SAFE;
	if (mant_mandoc_tbl_cell_is_vertical_continuation(data))
		flags |= MANT_MANDOC_EXEC_TABLE_CELL_VERTICAL_CONTINUATION;
	return flags;
}

static int
execution_table_cell_begin(void *arg, const struct termp *p,
    const struct roff_node *node, const struct tbl_span *span,
    const struct tbl_cell *cell, const struct tbl_dat *data, size_t ordinal,
    size_t data_ordinal, size_t coloff_before, size_t coloff_after)
{
	struct mant_mandoc_execution_report *report = arg;
	struct mant_mandoc_table_cell_record *record;
	uint32_t layout_kind, data_kind, font;
	size_t buffer;
	int64_t offset_bu, rmargin_bu, coloff_before_bu;

	(void)span;
	(void)coloff_after;
	if (cell == NULL || data == NULL || data->layout != cell ||
	    ordinal > UINT32_MAX || data_ordinal > UINT32_MAX ||
	    cell->col < 0 || report->current_table_row ==
	    MANT_MANDOC_EXEC_NONE || report->current_table_cell !=
	    MANT_MANDOC_EXEC_NONE || report->atoms_count > UINT32_MAX ||
	    !charge_record(report) ||
	    !reserve_table_cells(report, report->table_cells_count + 1))
		return 0;
	if (p->tcol < p->tcols || p->tcol >= p->tcols + p->maxtcol)
		return 0;
	buffer = (size_t)(p->tcol - p->tcols);
	layout_kind = table_layout_kind(cell->pos);
	data_kind = table_data_kind(data->pos);
	font = stable_font(cell->font == ESCAPE_FONTBI ? TERMFONT_BI :
	    cell->font == ESCAPE_FONTBOLD || cell->font == ESCAPE_FONTCB ?
	    TERMFONT_BOLD : cell->font == ESCAPE_FONTITALIC ||
	    cell->font == ESCAPE_FONTCI ? TERMFONT_UNDER : TERMFONT_NONE);
	if (!size_to_report_i64(report, p->tcol->offset, &offset_bu) ||
	    !size_to_report_i64(report, p->tcol->rmargin, &rmargin_bu) ||
	    !size_to_report_i64(report, coloff_before, &coloff_before_bu) ||
	    layout_kind == MANT_MANDOC_EXEC_NONE ||
	    data_kind == MANT_MANDOC_EXEC_NONE ||
	    font == MANT_MANDOC_EXEC_NONE)
		return 0;
	record = &report->table_cells[report->table_cells_count];
	memset(record, 0, sizeof(*record));
	record->key = (uint32_t)report->table_cells_count++;
	record->row = report->current_table_row;
	record->node = lookup_node(report, node);
	record->ordinal = (uint32_t)ordinal;
	record->data_ordinal = (uint32_t)data_ordinal;
	record->logical_column = (uint32_t)cell->col;
	record->column_span = (uint32_t)data->hspans + 1;
	record->row_span = (uint32_t)data->vspans + 1;
	record->layout_kind = layout_kind;
	record->data_kind = data_kind;
	record->alignment = table_alignment(cell->pos);
	record->font = font;
	record->flags = table_cell_flags(cell, data);
	record->buffer = MANT_MANDOC_EXEC_NONE;
	record->buffer_generation = MANT_MANDOC_EXEC_NONE;
	record->enter_atom = (uint32_t)report->atoms_count;
	record->leave_atom = MANT_MANDOC_EXEC_NONE;
	record->offset_bu = offset_bu;
	record->rmargin_bu = rmargin_bu;
	record->coloff_before_bu = coloff_before_bu;
	record->coloff_after_bu = coloff_before_bu;
	record->enter_sequence = report->sequence++;
	record->leave_sequence = UINT64_MAX;
	report->current_table_cell = record->key;
	report->current_table_cell_node = node;
	report->current_table_cell_span = span;
	report->current_table_cell_layout = cell;
	report->current_table_cell_data = data;
	report->current_table_cell_ordinal = ordinal;
	report->current_table_cell_data_ordinal = data_ordinal;
	report->current_table_cell_column = buffer;
	report->current_table_cell_coloff = coloff_before;
	return 1;
}

static int
execution_table_cell_end(void *arg, const struct termp *p,
    const struct roff_node *node, const struct tbl_span *span,
    const struct tbl_cell *cell, const struct tbl_dat *data, size_t ordinal,
    size_t data_ordinal, size_t coloff_before, size_t coloff_after)
{
	struct mant_mandoc_execution_report *report = arg;
	struct mant_mandoc_table_cell_record *record;
	size_t buffer;
	int64_t coloff_after_bu;

	if (!size_to_report_i64(report, coloff_after, &coloff_after_bu) ||
	    report->current_table_cell == MANT_MANDOC_EXEC_NONE ||
	    report->current_table_cell >= report->table_cells_count ||
	    report->atoms_count > UINT32_MAX || !charge_work(report, 1))
		return 0;
	record = &report->table_cells[report->current_table_cell];
	if (node != report->current_table_cell_node ||
	    span != report->current_table_cell_span ||
	    cell != report->current_table_cell_layout ||
	    data != report->current_table_cell_data ||
	    ordinal != report->current_table_cell_ordinal ||
	    data_ordinal != report->current_table_cell_data_ordinal ||
	    coloff_before != report->current_table_cell_coloff ||
	    p->tcol < p->tcols || p->tcol >= p->tcols + p->maxtcol)
		return 0;
	buffer = (size_t)(p->tcol - p->tcols);
	if (buffer != report->current_table_cell_column)
		return 0;
	if (buffer < report->buffer_count &&
	    report->buffers[buffer].generation_record != MANT_MANDOC_EXEC_NONE) {
		record->buffer = (uint32_t)buffer;
		record->buffer_generation = report->buffers[buffer].generation_record;
	}
	record->leave_atom = (uint32_t)report->atoms_count;
	record->coloff_after_bu = coloff_after_bu;
	record->leave_sequence = report->sequence++;
	report->current_table_cell = MANT_MANDOC_EXEC_NONE;
	report->current_table_cell_node = NULL;
	report->current_table_cell_span = NULL;
	report->current_table_cell_layout = NULL;
	report->current_table_cell_data = NULL;
	return 1;
}

static int
validate_table_records(struct mant_mandoc_execution_report *report)
{
	struct mant_mandoc_buffer_generation_record *generation;
	struct mant_mandoc_atom_record *atom;
	struct mant_mandoc_table_record *table;
	struct mant_mandoc_table_row_record *row;
	struct mant_mandoc_table_cell_record *cell;
	struct mant_mandoc_table_row_record *previous_row;
	struct mant_mandoc_table_cell_record *previous_cell;
	struct mant_mandoc_flush_record *flush;
	struct mant_mandoc_fragment_record *fragment;
	size_t index, inner, row_cursor, cell_cursor, payload_cell, owner;
	uint32_t next_data_ordinal, next_column, *generation_cell;

	row_cursor = cell_cursor = 0;
	generation_cell = report->buffer_generations_count == 0 ? NULL :
	    malloc(report->buffer_generations_count * sizeof(*generation_cell));
	if (report->buffer_generations_count != 0 && generation_cell == NULL) {
		fail_report(report, MANT_MANDOC_EXECUTION_ALLOCATION,
		    "could not validate native table generation ownership");
		return 0;
	}
	for (index = 0; index < report->buffer_generations_count; index++)
		generation_cell[index] = MANT_MANDOC_EXEC_NONE;

	for (index = 0; index < report->tables_count; index++) {
		table = &report->tables[index];
		if (table->key != index ||
		    table->first_row_node >= report->nodes_count ||
		    table->row_start != row_cursor || table->row_length == 0 ||
		    table->row_start > report->table_rows_count ||
		    table->row_length > report->table_rows_count - table->row_start ||
		    table->cell_start != cell_cursor ||
		    table->cell_start > report->table_cells_count ||
		    table->cell_length > report->table_cells_count - table->cell_start ||
		    table->logical_columns == 0 || table->flags != 0 ||
		    table->enter_atom > table->leave_atom ||
		    table->leave_atom > report->atoms_count ||
		    table->enter_fragment > table->leave_fragment ||
		    table->leave_fragment > report->fragments_count ||
		    table->enter_flush > table->leave_flush ||
		    table->leave_flush > report->flushes_count ||
		    table->enter_sequence >= table->leave_sequence)
			goto invalid;
		previous_row = NULL;
		for (inner = 0; inner < table->row_length; inner++) {
			row = &report->table_rows[table->row_start + inner];
			if (row->key != table->row_start + inner ||
			    row->table != table->key || row->ordinal != inner ||
			    row->node >= report->nodes_count ||
			    row->kind < MANT_MANDOC_EXEC_TABLE_ROW_DATA ||
			    row->kind > MANT_MANDOC_EXEC_TABLE_ROW_DOUBLE_RULE ||
			    row->logical_columns != table->logical_columns ||
			    row->cell_start != cell_cursor ||
			    row->cell_start > report->table_cells_count ||
			    row->cell_length > report->table_cells_count -
			    row->cell_start || row->enter_atom > row->leave_atom ||
			    row->enter_atom < table->enter_atom ||
			    row->leave_atom > table->leave_atom ||
			    row->enter_fragment > row->leave_fragment ||
			    row->enter_fragment < table->enter_fragment ||
			    row->leave_fragment > table->leave_fragment ||
			    row->enter_flush > row->leave_flush ||
			    row->enter_flush < table->enter_flush ||
			    row->leave_flush > table->leave_flush ||
			    row->enter_sequence <= table->enter_sequence ||
			    row->leave_sequence >= table->leave_sequence ||
			    row->enter_sequence >= row->leave_sequence ||
			    (row->kind != MANT_MANDOC_EXEC_TABLE_ROW_DATA &&
			    row->cell_length != 0))
				goto invalid;
			if (previous_row == NULL) {
				if (row->node != table->first_row_node ||
				    row->enter_atom != table->enter_atom ||
				    row->enter_fragment != table->enter_fragment ||
				    row->enter_flush != table->enter_flush)
					goto invalid;
			} else if (row->enter_atom != previous_row->leave_atom ||
			    row->enter_fragment != previous_row->leave_fragment ||
			    row->enter_flush != previous_row->leave_flush ||
			    row->enter_sequence <= previous_row->leave_sequence)
				goto invalid;

			previous_cell = NULL;
			next_data_ordinal = next_column = 0;
			for (cell_cursor = row->cell_start;
			    cell_cursor < row->cell_start + row->cell_length;
			    cell_cursor++) {
				cell = &report->table_cells[cell_cursor];
				if (cell->key != cell_cursor || cell->row != row->key ||
				    cell->node != row->node ||
				    cell->ordinal != cell_cursor - row->cell_start ||
				    cell->data_ordinal != next_data_ordinal ||
				    cell->logical_column < next_column ||
				    cell->logical_column >= row->logical_columns ||
				    cell->column_span == 0 ||
				    cell->column_span > row->logical_columns -
				    cell->logical_column || cell->row_span == 0 ||
				    cell->layout_kind <
				    MANT_MANDOC_EXEC_TABLE_LAYOUT_CENTER ||
				    cell->layout_kind >
				    MANT_MANDOC_EXEC_TABLE_LAYOUT_DOUBLE_RULE ||
				    cell->data_kind < MANT_MANDOC_EXEC_TABLE_DATA_NONE ||
				    cell->data_kind >
				    MANT_MANDOC_EXEC_TABLE_DATA_ISOLATED_DOUBLE_RULE ||
				    cell->alignment > MANT_MANDOC_EXEC_TABLE_ALIGN_LONG ||
				    cell->font > MANT_MANDOC_FONT_BOLD_UNDERLINE ||
				    cell->flags & ~0x01ffU || cell->reserved != 0 ||
				    cell->enter_atom > cell->leave_atom ||
				    cell->enter_atom < row->enter_atom ||
				    cell->leave_atom > row->leave_atom ||
				    cell->enter_sequence <= row->enter_sequence ||
				    cell->leave_sequence >= row->leave_sequence ||
				    cell->enter_sequence >= cell->leave_sequence ||
				    cell->offset_bu < 0 ||
				    cell->rmargin_bu < cell->offset_bu ||
				    cell->coloff_before_bu < 0 ||
				    cell->coloff_after_bu < cell->coloff_before_bu ||
				    ((cell->buffer == MANT_MANDOC_EXEC_NONE) !=
				    (cell->buffer_generation == MANT_MANDOC_EXEC_NONE)))
					goto invalid;
				if (previous_cell != NULL &&
				    (cell->enter_atom < previous_cell->leave_atom ||
				    cell->enter_sequence <= previous_cell->leave_sequence))
					goto invalid;
				if ((cell->layout_kind ==
				    MANT_MANDOC_EXEC_TABLE_LAYOUT_LEFT &&
				    cell->alignment != MANT_MANDOC_EXEC_TABLE_ALIGN_LEFT) ||
				    (cell->layout_kind ==
				    MANT_MANDOC_EXEC_TABLE_LAYOUT_CENTER &&
				    cell->alignment != MANT_MANDOC_EXEC_TABLE_ALIGN_CENTER) ||
				    (cell->layout_kind ==
				    MANT_MANDOC_EXEC_TABLE_LAYOUT_RIGHT &&
				    cell->alignment != MANT_MANDOC_EXEC_TABLE_ALIGN_RIGHT) ||
				    (cell->layout_kind ==
				    MANT_MANDOC_EXEC_TABLE_LAYOUT_NUMERIC &&
				    cell->alignment != MANT_MANDOC_EXEC_TABLE_ALIGN_NUMERIC) ||
				    (cell->layout_kind ==
				    MANT_MANDOC_EXEC_TABLE_LAYOUT_LONG &&
				    cell->alignment != MANT_MANDOC_EXEC_TABLE_ALIGN_LONG) ||
				    ((cell->layout_kind ==
				    MANT_MANDOC_EXEC_TABLE_LAYOUT_SPAN ||
				    cell->layout_kind ==
				    MANT_MANDOC_EXEC_TABLE_LAYOUT_DOWN ||
				    cell->layout_kind ==
				    MANT_MANDOC_EXEC_TABLE_LAYOUT_SINGLE_RULE ||
				    cell->layout_kind ==
				    MANT_MANDOC_EXEC_TABLE_LAYOUT_DOUBLE_RULE) &&
				    cell->alignment != MANT_MANDOC_EXEC_TABLE_ALIGN_NONE))
					goto invalid;
				if (cell->buffer_generation != MANT_MANDOC_EXEC_NONE) {
					if (cell->buffer_generation >=
					    report->buffer_generations_count)
						goto invalid;
					generation = &report->buffer_generations[
					    cell->buffer_generation];
					if (generation->buffer != cell->buffer ||
					    generation->open_sequence <=
					    cell->enter_sequence ||
					    generation->open_sequence >=
					    cell->leave_sequence ||
					    generation->close_sequence >=
					    row->leave_sequence ||
					    generation_cell[cell->buffer_generation] !=
					    MANT_MANDOC_EXEC_NONE ||
					    cell->enter_atom == cell->leave_atom)
						goto invalid;
					generation_cell[cell->buffer_generation] = cell->key;
				} else if (cell->enter_atom != cell->leave_atom)
					goto invalid;
				if ((cell->enter_atom != 0 &&
				    report->atoms[cell->enter_atom - 1].sequence >=
				    cell->enter_sequence) ||
				    (cell->enter_atom < cell->leave_atom &&
				    report->atoms[cell->enter_atom].sequence <=
				    cell->enter_sequence) ||
				    (cell->enter_atom < cell->leave_atom &&
				    report->atoms[cell->leave_atom - 1].sequence >=
				    cell->leave_sequence) ||
				    (cell->leave_atom < report->atoms_count &&
				    report->atoms[cell->leave_atom].sequence <=
				    cell->leave_sequence))
					goto invalid;
				for (owner = cell->enter_atom;
				    owner < cell->leave_atom; owner++) {
					atom = &report->atoms[owner];
					if (atom->node != cell->node ||
					    atom->buffer != cell->buffer ||
					    atom->buffer_generation !=
					    cell->buffer_generation)
						goto invalid;
				}
				next_data_ordinal++;
				next_column = cell->logical_column + cell->column_span;
				previous_cell = cell;
			}
			cell_cursor = row->cell_start + row->cell_length;
			previous_row = row;
		}
		if (previous_row == NULL ||
		    previous_row->leave_atom != table->leave_atom ||
		    previous_row->leave_fragment != table->leave_fragment ||
		    previous_row->leave_flush != table->leave_flush ||
		    previous_row->leave_sequence >= table->leave_sequence ||
		    cell_cursor != table->cell_start + table->cell_length)
			goto invalid;
		row_cursor = table->row_start + table->row_length;
	}
	if (row_cursor != report->table_rows_count ||
	    cell_cursor != report->table_cells_count)
		goto invalid;

	payload_cell = 0;
	for (index = 0; index < report->atoms_count; index++) {
		atom = &report->atoms[index];
		if (atom->role != MANT_MANDOC_ATOM_TABLE_CELL_PAYLOAD)
			continue;
		while (payload_cell < report->table_cells_count &&
		    report->table_cells[payload_cell].leave_atom <= index)
			payload_cell++;
		if (payload_cell == report->table_cells_count)
			goto invalid;
		cell = &report->table_cells[payload_cell];
		if (index < cell->enter_atom || index >= cell->leave_atom ||
		    cell->buffer_generation == MANT_MANDOC_EXEC_NONE ||
		    atom->buffer != cell->buffer || atom->buffer_generation !=
		    cell->buffer_generation)
			goto invalid;
	}
	for (index = 0; index < report->flushes_count; index++) {
		flush = &report->flushes[index];
		if (flush->buffer_generation >= report->buffer_generations_count)
			continue;
		owner = generation_cell[flush->buffer_generation];
		if (owner == MANT_MANDOC_EXEC_NONE)
			continue;
		cell = &report->table_cells[owner];
		row = &report->table_rows[cell->row];
		table = &report->tables[row->table];
		if (flush->key < row->enter_flush || flush->key >= row->leave_flush ||
		    flush->key < table->enter_flush ||
		    flush->key >= table->leave_flush)
			goto invalid;
	}
	for (index = 0; index < report->fragments_count; index++) {
		fragment = &report->fragments[index];
		if (fragment->buffer_generation == MANT_MANDOC_EXEC_NONE ||
		    fragment->buffer_generation >= report->buffer_generations_count)
			continue;
		owner = generation_cell[fragment->buffer_generation];
		if (owner == MANT_MANDOC_EXEC_NONE)
			continue;
		cell = &report->table_cells[owner];
		row = &report->table_rows[cell->row];
		table = &report->tables[row->table];
		if (fragment->key < row->enter_fragment ||
		    fragment->key >= row->leave_fragment ||
		    fragment->key < table->enter_fragment ||
		    fragment->key >= table->leave_fragment)
			goto invalid;
	}
	free(generation_cell);
	return 1;

invalid:
	free(generation_cell);
	fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
	    "native execution table relationship is inconsistent");
	return 0;
}

static int
append_fragment(struct mant_mandoc_execution_report *report,
    const struct termp *p, const struct roff_node *node, int scalar,
    size_t slot, size_t before, size_t after)
{
	struct mant_mandoc_fragment_record *fragment;
	struct mant_mandoc_fragment_atom_record *reference;
	struct mant_mandoc_geometry_record *geometry;
	struct mant_mandoc_atom_record *atom;
	struct buffer_origin *buffer;
	size_t buffer_key;
	uint32_t atom_key;
	int64_t start_bu, end_bu, width_bu;

	if (!size_to_report_i64(report, before, &start_bu) ||
	    !size_to_report_i64(report, after, &end_bu) ||
	    !size_delta_to_report_i64(report, before, after, &width_bu) ||
	    !charge_work(report, 1))
		return 0;
	buffer_key = (size_t)(p->tcol - p->tcols);
	atom_key = MANT_MANDOC_EXEC_NONE;
	if (slot != SIZE_MAX && p->exec_field_active &&
	    buffer_key < report->buffer_count &&
	    slot < report->buffers[buffer_key].capacity) {
		buffer = &report->buffers[buffer_key];
		atom_key = buffer->atoms[slot];
	}
	if (atom_key == MANT_MANDOC_EXEC_NONE) {
		if (!charge_record(report) ||
		    !reserve_atoms(report, report->atoms_count + 1))
			return 0;
		atom = &report->atoms[report->atoms_count];
		memset(atom, 0, sizeof(*atom));
		atom->key = (uint32_t)report->atoms_count++;
		atom->buffer = MANT_MANDOC_EXEC_NONE;
		atom->generation = MANT_MANDOC_EXEC_NONE;
		atom->buffer_generation = MANT_MANDOC_EXEC_NONE;
		atom->slot = MANT_MANDOC_EXEC_NONE;
		atom->kind = atom_kind(scalar);
		atom->role = report->word_active ? p->exec_write_role :
		    MANT_MANDOC_ATOM_DEVICE_GENERATED;
		atom->input_scalar = (uint32_t)scalar;
		atom->display_scalar = (uint32_t)scalar;
		atom->width_bu = width_bu;
		atom->node = report->word_active ? report->current_word_node :
		    lookup_node(report, node);
		atom->source = atom->node != MANT_MANDOC_EXEC_NONE ?
		    report->nodes[atom->node].source : 0;
		atom->operand_start = report->word_active ?
		    report->current_word_start : MANT_MANDOC_EXEC_NONE;
		atom->operand_length = report->word_active ?
		    report->current_word_length : 0;
		atom->font = stable_font(p->fontq[p->fonti]);
		atom->wrapper = report->current_wrapper;
		atom->replaced_by = MANT_MANDOC_EXEC_NONE;
		atom->disposition = MANT_MANDOC_ATOM_EMITTED;
		atom->sequence = report->sequence++;
		atom_key = atom->key;
	} else if (atom_key < report->atoms_count) {
		if (report->atoms[atom_key].generation !=
		    report->buffers[buffer_key].generation) {
			fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
			    "native execution emitted a stale buffer atom");
			return 0;
		}
		report->atoms[atom_key].display_scalar = (uint32_t)scalar;
		report->atoms[atom_key].kind = atom_kind(scalar);
		report->atoms[atom_key].width_bu = width_bu;
		report->atoms[atom_key].disposition = MANT_MANDOC_ATOM_EMITTED;
	} else
		return 0;

	if (!charge_record(report) ||
	    !reserve_fragments(report, report->fragments_count + 1) ||
	    !charge_record(report) ||
	    !reserve_fragment_atoms(report, report->fragment_atoms_count + 1) ||
	    !charge_record(report) ||
	    !reserve_geometries(report, report->geometries_count + 1))
		return 0;
	fragment = &report->fragments[report->fragments_count];
	memset(fragment, 0, sizeof(*fragment));
	fragment->key = (uint32_t)report->fragments_count++;
	/*
	 * Buffered glyphs can be emitted only after print_*_node() has
	 * unwound to an enclosing node.  Their source owner is the atom that
	 * entered the terminal buffer, not the node active at the later flush.
	 * Direct device output has an atom synthesized from the current node
	 * above, so the same rule covers both paths.
	 */
	fragment->node = report->atoms[atom_key].node;
	fragment->buffer = report->atoms[atom_key].buffer;
	fragment->generation = report->atoms[atom_key].generation;
	fragment->buffer_generation =
	    report->atoms[atom_key].buffer_generation;
	fragment->atom_ref_start = (uint32_t)report->fragment_atoms_count;
	fragment->atom_ref_length = 1;
	fragment->device_line = (uint32_t)p->line;
	if (p->exec_fragment_role == TERM_EXEC_FRAGMENT_MARGIN)
		fragment->role = MANT_MANDOC_FRAGMENT_MARGIN_DECORATION;
	else if (p->exec_fragment_role == TERM_EXEC_FRAGMENT_PAGE)
		fragment->role = MANT_MANDOC_FRAGMENT_PAGE_DECORATION;
	else if (p->exec_fragment_role != TERM_EXEC_FRAGMENT_CONTENT) {
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
		    "native execution reported an unknown fragment role");
		return 0;
	} else if (report->atoms[atom_key].role ==
	    MANT_MANDOC_ATOM_FONT_DECORATION)
		fragment->role = MANT_MANDOC_FRAGMENT_FONT_DECORATION;
	else
		fragment->role = MANT_MANDOC_FRAGMENT_CONTENT;
	fragment->wrapper = report->current_wrapper;
	fragment->start_bu = start_bu;
	fragment->end_bu = end_bu;
	fragment->sequence = report->sequence++;
	reference = &report->fragment_atoms[report->fragment_atoms_count++];
	reference->fragment = fragment->key;
	reference->atom = atom_key;
	geometry = &report->geometries[report->geometries_count];
	memset(geometry, 0, sizeof(*geometry));
	geometry->key = (uint32_t)report->geometries_count++;
	geometry->node = fragment->node;
	geometry->related = fragment->key;
	geometry->kind = MANT_MANDOC_GEOMETRY_GLYPH;
	geometry->unit = MANT_MANDOC_UNIT_BASIC;
	geometry->origin_kind = MANT_MANDOC_GEOMETRY_ORIGIN_ATOM;
	geometry->origin_key = atom_key;
	geometry->requested = width_bu;
	geometry->effective = geometry->requested;
	geometry->before = fragment->start_bu;
	geometry->after = fragment->end_bu;
	geometry->sequence = report->sequence++;
	return 1;
}

static int
append_boundary(struct mant_mandoc_execution_report *report,
    const struct termp *p, const struct roff_node *node, uint32_t request,
    uint32_t effect)
{
	struct mant_mandoc_boundary_record *record;
	int64_t line, visual;

	if (!size_to_report_i64(report, p->line, &line) ||
	    !size_to_report_i64(report, p->viscol, &visual) ||
	    !charge_work(report, 1) || !charge_record(report) ||
	    !reserve_boundaries(report, report->boundaries_count + 1))
		return 0;
	record = &report->boundaries[report->boundaries_count];
	memset(record, 0, sizeof(*record));
	record->key = (uint32_t)report->boundaries_count++;
	record->node = lookup_node(report, node);
	record->parent = report->current_boundary;
	record->request = request;
	record->effect = effect;
	record->flags_before = stable_term_flags(p->flags);
	record->flags_after = record->flags_before;
	record->control = report->current_control;
	record->wrapper = report->current_wrapper;
	record->line_before = line;
	record->line_after = line;
	record->visual_before = visual;
	record->visual_after = visual;
	record->enter_sequence = report->sequence++;
	record->leave_sequence = UINT64_MAX;
	report->current_boundary = record->key;
	return 1;
}

static int
capture_control_state(struct mant_mandoc_execution_report *report,
    const struct termp *p, struct mant_mandoc_control_record *record,
    int after)
{
	size_t buffer_key;
	uint32_t generation;
	int64_t line, visual, column, extent, offset, rmargin, maxrmargin;
	int64_t taboff, minbl, trailspace;

	if (p == NULL || p->tcols == NULL || p->tcol == NULL ||
	    p->tcol < p->tcols || p->tcol >= p->tcols + p->maxtcol) {
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
		    "native execution control has an invalid terminal column");
		return 0;
	}
	buffer_key = (size_t)(p->tcol - p->tcols);
	if (buffer_key > UINT32_MAX || buffer_key >= report->buffer_count ||
	    !size_to_report_i64(report, p->line, &line) ||
	    !size_to_report_i64(report, p->viscol, &visual) ||
	    !size_to_report_i64(report, p->tcol->col, &column) ||
	    !size_to_report_i64(report, p->tcol->lastcol, &extent) ||
	    !size_to_report_i64(report, p->tcol->offset, &offset) ||
	    !size_to_report_i64(report, p->tcol->rmargin, &rmargin) ||
	    !size_to_report_i64(report, p->maxrmargin, &maxrmargin) ||
	    !size_to_report_i64(report, p->tcol->taboff, &taboff) ||
	    !size_to_report_i64(report, p->minbl, &minbl) ||
	    !size_to_report_i64(report, p->trailspace, &trailspace))
		return 0;
	generation = report->buffers[buffer_key].generation_record;
	if (generation != MANT_MANDOC_EXEC_NONE &&
	    generation >= report->buffer_generations_count) {
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
		    "native execution control refers to an invalid buffer generation");
		return 0;
	}
	record->buffer = (uint32_t)buffer_key;
	if (!after) {
		record->generation_before = generation;
		record->flags_before = stable_term_flags(p->flags);
		record->line_before = line;
		record->visual_before = visual;
		record->column_before = column;
		record->extent_before = extent;
		record->offset_before = offset;
		record->rmargin_before = rmargin;
		record->maxrmargin_before = maxrmargin;
		record->taboff_before = taboff;
		record->temporary_indent_before = p->ti;
		record->skip_vertical_before = p->skipvsp;
		record->minimum_blank_before = minbl;
		record->trailing_blank_before = trailspace;
	} else {
		record->generation_after = generation;
		record->flags_after = stable_term_flags(p->flags);
		record->line_after = line;
		record->visual_after = visual;
		record->column_after = column;
		record->extent_after = extent;
		record->offset_after = offset;
		record->rmargin_after = rmargin;
		record->maxrmargin_after = maxrmargin;
		record->taboff_after = taboff;
		record->temporary_indent_after = p->ti;
		record->skip_vertical_after = p->skipvsp;
		record->minimum_blank_after = minbl;
		record->trailing_blank_after = trailspace;
	}
	return 1;
}

static int
charge_work(struct mant_mandoc_execution_report *report, uint64_t amount)
{
	if (report == NULL ||
	    report->status != MANT_MANDOC_EXECUTION_BUILDING)
		return 0;
	if (report->cancelled != NULL &&
	    report->cancelled(report->cancellation_context)) {
		fail_report(report, MANT_MANDOC_EXECUTION_CANCELLED,
		    "native execution was cancelled");
		return 0;
	}
	if (report->work_count > report->limits.max_work ||
	    amount > report->limits.max_work - report->work_count) {
		fail_report(report, MANT_MANDOC_EXECUTION_BUDGET,
		    "native execution work limit exceeded");
		return 0;
	}
	report->work_count += amount;
	return 1;
}

static int
charge_record(struct mant_mandoc_execution_report *report)
{
	if (report == NULL ||
	    report->status != MANT_MANDOC_EXECUTION_BUILDING)
		return 0;
	if (report->record_count == report->limits.max_records) {
		fail_report(report, MANT_MANDOC_EXECUTION_BUDGET,
		    "native execution record limit exceeded");
		return 0;
	}
	report->record_count++;
	return 1;
}

static int
charge_sort_work(struct mant_mandoc_execution_report *report, size_t count)
{
	uint64_t levels;
	size_t value;

	if (count < 2)
		return charge_work(report, count);
	levels = 0;
	for (value = count - 1; value != 0; value >>= 1)
		levels++;
	if ((uint64_t)count > UINT64_MAX / levels) {
		fail_report(report, MANT_MANDOC_EXECUTION_BUDGET,
		    "native execution sort work overflowed");
		return 0;
	}
	return charge_work(report, (uint64_t)count * levels);
}

static int
append_pool(struct mant_mandoc_execution_report *report, const void *bytes,
    size_t length, uint32_t *start)
{
	unsigned char *pool;
	size_t capacity;

	if (report == NULL || start == NULL || (bytes == NULL && length != 0) ||
	    length > report->limits.max_pool_bytes - report->pool_length) {
		if (report != NULL)
			fail_report(report, MANT_MANDOC_EXECUTION_BUDGET,
			    "native execution byte-pool limit exceeded");
		return 0;
	}
	*start = (uint32_t)report->pool_length;
	if (length == 0)
		return 1;
	if (length > SIZE_MAX - report->pool_length) {
		fail_report(report, MANT_MANDOC_EXECUTION_ALLOCATION,
		    "native execution byte-pool allocation overflow");
		return 0;
	}
	if (report->pool_length + length > report->pool_capacity) {
		capacity = report->pool_capacity == 0 ? 256 :
		    report->pool_capacity;
		while (capacity - report->pool_length < length) {
			if (capacity > report->limits.max_pool_bytes / 2) {
				capacity = (size_t)report->limits.max_pool_bytes;
				break;
			}
			capacity *= 2;
		}
		pool = realloc(report->pool, capacity);
		if (pool == NULL) {
			fail_report(report, MANT_MANDOC_EXECUTION_ALLOCATION,
			    "could not allocate native execution byte pool");
			return 0;
		}
		report->pool = pool;
		report->pool_capacity = capacity;
	}
	memcpy(report->pool + report->pool_length, bytes, length);
	report->pool_length += length;
	return 1;
}

static int
seal_report(struct mant_mandoc_execution_report *report)
{
	uint64_t expected;
	size_t index;

	if (!validate_report_storage(report))
		return 0;
	if (!charge_work(report, report->record_count))
		return 0;
	if (report->current_wrapper != MANT_MANDOC_EXEC_NONE ||
	    report->current_boundary != MANT_MANDOC_EXEC_NONE ||
	    report->current_control != MANT_MANDOC_EXEC_NONE ||
	    report->current_flush != MANT_MANDOC_EXEC_NONE ||
	    report->current_table != MANT_MANDOC_EXEC_NONE ||
	    report->current_table_row != MANT_MANDOC_EXEC_NONE ||
	    report->current_table_cell != MANT_MANDOC_EXEC_NONE ||
	    report->word_active) {
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
		    "native execution ended with an active scope");
		return 0;
	}
	for (index = 0; index < report->buffer_count; index++) {
		if (report->buffers[index].generation_record !=
		    MANT_MANDOC_EXEC_NONE &&
		    !close_buffer_generation(report, index, 0,
		    MANT_MANDOC_BUFFER_REPORT_END))
			return 0;
	}
	expected = 0;
#define ADD_RECORD_COUNT(name) do { \
	if ((uint64_t)report->name##_count > UINT64_MAX - expected) { \
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL, \
		    "native execution record accounting overflowed"); \
		return 0; \
	} \
	expected += (uint64_t)report->name##_count; \
} while (0)
	ADD_RECORD_COUNT(sources);
	ADD_RECORD_COUNT(nodes);
	ADD_RECORD_COUNT(buffer_generations);
	ADD_RECORD_COUNT(words);
	ADD_RECORD_COUNT(atoms);
	ADD_RECORD_COUNT(fragments);
	ADD_RECORD_COUNT(fragment_atoms);
	ADD_RECORD_COUNT(flushes);
	ADD_RECORD_COUNT(boundaries);
	ADD_RECORD_COUNT(controls);
	ADD_RECORD_COUNT(geometries);
	ADD_RECORD_COUNT(wrappers);
	ADD_RECORD_COUNT(references);
	ADD_RECORD_COUNT(anchors);
	ADD_RECORD_COUNT(tables);
	ADD_RECORD_COUNT(table_rows);
	ADD_RECORD_COUNT(table_cells);
	ADD_RECORD_COUNT(diagnostics);
#undef ADD_RECORD_COUNT
	if (expected != report->record_count) {
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
		    "native execution record accounting is inconsistent");
		return 0;
	}
	for (index = 0; index < report->atoms_count; index++) {
		if (report->atoms[index].disposition ==
		    MANT_MANDOC_ATOM_BUFFERED) {
			fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
			    "native execution left a buffered atom at seal");
			return 0;
		}
	}
	for (index = 0; index < report->wrappers_count; index++) {
		if (report->wrappers[index].kind == MANT_MANDOC_WRAPPER_NODE &&
		    report->wrappers[index].leave_atom == MANT_MANDOC_EXEC_NONE) {
			fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
			    "native execution left a node wrapper open");
			return 0;
		}
	}
	if (report->current_reference != MANT_MANDOC_EXEC_NONE) {
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
		    "native execution left a semantic reference open");
		return 0;
	}
	if (!validate_table_records(report) || !validate_sealed_report(report))
		return 0;
	report->status = MANT_MANDOC_EXECUTION_COMPLETE;
	return 1;
}

static int
validate_report_storage(struct mant_mandoc_execution_report *report)
{
	if (report == NULL)
		return 0;
	if (report->pool_length > report->pool_capacity ||
	    (report->pool_length != 0 && report->pool == NULL) ||
	    (report->buffer_count != 0 && report->buffers == NULL)) {
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
		    "native execution report storage is inconsistent");
		return 0;
	}
#define VALIDATE_RECORD_STORAGE(name) do { \
	if (report->name##_count > report->name##_capacity || \
	    (report->name##_count != 0 && report->name == NULL)) { \
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL, \
		    "native execution report storage is inconsistent"); \
		return 0; \
	} \
} while (0)
	VALIDATE_RECORD_STORAGE(sources);
	VALIDATE_RECORD_STORAGE(nodes);
	VALIDATE_RECORD_STORAGE(buffer_generations);
	VALIDATE_RECORD_STORAGE(words);
	VALIDATE_RECORD_STORAGE(atoms);
	VALIDATE_RECORD_STORAGE(fragments);
	VALIDATE_RECORD_STORAGE(fragment_atoms);
	VALIDATE_RECORD_STORAGE(flushes);
	VALIDATE_RECORD_STORAGE(boundaries);
	VALIDATE_RECORD_STORAGE(controls);
	VALIDATE_RECORD_STORAGE(geometries);
	VALIDATE_RECORD_STORAGE(wrappers);
	VALIDATE_RECORD_STORAGE(references);
	VALIDATE_RECORD_STORAGE(anchors);
	VALIDATE_RECORD_STORAGE(tables);
	VALIDATE_RECORD_STORAGE(table_rows);
	VALIDATE_RECORD_STORAGE(table_cells);
	VALIDATE_RECORD_STORAGE(diagnostics);
#undef VALIDATE_RECORD_STORAGE
	return 1;
}

static int
valid_pool_range(const struct mant_mandoc_execution_report *report,
    uint32_t start, uint32_t length, int optional)
{
	if (start == MANT_MANDOC_EXEC_NONE)
		return optional && length == 0;
	return start <= report->pool_length &&
	    length <= report->pool_length - start;
}

static int
valid_scalar(uint32_t scalar)
{
	return scalar <= 0x10ffffU &&
	    (scalar < 0xd800U || scalar > 0xdfffU);
}

static int
terminal_tail_scalar(uint32_t scalar)
{
	return scalar == '\t' || scalar == ' ' || scalar == '\n' ||
	    scalar == ASCII_NBRZW || scalar == ASCII_BREAK ||
	    scalar == ASCII_TABREF;
}

static int
size_to_report_i64(struct mant_mandoc_execution_report *report,
    size_t value, int64_t *converted)
{
	if (converted == NULL || value > (size_t)INT64_MAX) {
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
		    "native execution coordinate exceeds the report width");
		return 0;
	}
	*converted = (int64_t)value;
	return 1;
}

static int
size_delta_to_report_i64(struct mant_mandoc_execution_report *report,
    size_t before, size_t after, int64_t *converted)
{
	size_t magnitude;

	magnitude = after >= before ? after - before : before - after;
	if (converted == NULL || magnitude > (size_t)INT64_MAX) {
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
		    "native execution delta exceeds the report width");
		return 0;
	}
	*converted = after >= before ? (int64_t)magnitude : -(int64_t)magnitude;
	return 1;
}

static int
validate_sealed_report(struct mant_mandoc_execution_report *report)
{
	struct mant_mandoc_source_record *source;
	struct mant_mandoc_node_record *node;
	struct mant_mandoc_buffer_generation_record *generation;
	struct mant_mandoc_word_record *word;
	struct mant_mandoc_atom_record *atom;
	struct mant_mandoc_fragment_record *fragment;
	struct mant_mandoc_fragment_atom_record *reference;
	struct mant_mandoc_flush_record *flush, *next_flush;
	struct mant_mandoc_boundary_record *boundary;
	struct mant_mandoc_control_record *control;
	struct mant_mandoc_geometry_record *geometry;
	struct mant_mandoc_wrapper_record *wrapper;
	struct mant_mandoc_reference_record *semantic_reference;
	struct mant_mandoc_anchor_record *anchor;
	struct mant_mandoc_execution_diagnostic_record *diagnostic;
	struct live_atom_location *live_atoms;
	struct generation_checkpoint *generation_checkpoints;
	uint32_t *next_generation, *last_capacity, *last_close_reason, origin;
	uint32_t *pending_flush, *direct_boundary_lines, *subtree_boundary_lines;
	uint64_t *last_close_sequence, *last_reference_child_leave;
	uint64_t *last_wrapper_child_leave, *last_control_child_leave;
	uint64_t last_root_reference_leave, last_root_wrapper_leave;
	uint64_t last_root_control_leave;
	unsigned char *covered_refs, *referenced_atoms, *replaced_atoms;
	unsigned char *word_atoms, *margin_atoms, *margin_control_nodes;
	unsigned char *covered_fragments;
	unsigned char *covered_glyph_geometry;
	unsigned char *terminal_flush;
	uint64_t *fragment_flush_outcome;
	uint64_t capacity_total;
	size_t index, inner, end, live_atom_count, live_index, control_cursor;
	size_t wrapper_cursor, generation_checkpoint_count;
	uint32_t active_control, active_wrapper;
	uint64_t last_boundary_enter;
	int first_flush;

	if (!validate_report_storage(report))
		return 0;
	generation_checkpoints = NULL;
	generation_checkpoint_count = 0;

	next_generation = report->buffer_count == 0 ? NULL :
	    calloc(report->buffer_count, sizeof(*next_generation));
	last_capacity = report->buffer_count == 0 ? NULL :
	    calloc(report->buffer_count, sizeof(*last_capacity));
	last_close_reason = report->buffer_count == 0 ? NULL :
	    calloc(report->buffer_count, sizeof(*last_close_reason));
	last_close_sequence = report->buffer_count == 0 ? NULL :
	    calloc(report->buffer_count, sizeof(*last_close_sequence));
	covered_refs = report->fragment_atoms_count == 0 ? NULL :
	    calloc(report->fragment_atoms_count, 1);
	referenced_atoms = report->atoms_count == 0 ? NULL :
	    calloc(report->atoms_count, 1);
	replaced_atoms = report->atoms_count == 0 ? NULL :
	    calloc(report->atoms_count, 1);
	word_atoms = report->atoms_count == 0 ? NULL :
	    calloc(report->atoms_count, 1);
	margin_atoms = report->atoms_count == 0 ? NULL :
	    calloc(report->atoms_count, 1);
	margin_control_nodes = report->nodes_count == 0 ? NULL :
	    calloc(report->nodes_count, 1);
	covered_fragments = report->fragments_count == 0 ? NULL :
	    calloc(report->fragments_count, 1);
	covered_glyph_geometry = report->fragments_count == 0 ? NULL :
	    calloc(report->fragments_count, 1);
	fragment_flush_outcome = report->fragments_count == 0 ? NULL :
	    calloc(report->fragments_count, sizeof(*fragment_flush_outcome));
	last_reference_child_leave = report->references_count == 0 ? NULL :
	    calloc(report->references_count,
	    sizeof(*last_reference_child_leave));
	last_wrapper_child_leave = report->wrappers_count == 0 ? NULL :
	    calloc(report->wrappers_count,
	    sizeof(*last_wrapper_child_leave));
	last_control_child_leave = report->controls_count == 0 ? NULL :
	    calloc(report->controls_count,
	    sizeof(*last_control_child_leave));
	pending_flush = report->buffer_generations_count == 0 ? NULL :
	    calloc(report->buffer_generations_count, sizeof(*pending_flush));
	terminal_flush = report->buffer_generations_count == 0 ? NULL :
	    calloc(report->buffer_generations_count, 1);
	direct_boundary_lines = report->boundaries_count == 0 ? NULL :
	    calloc(report->boundaries_count, sizeof(*direct_boundary_lines));
	subtree_boundary_lines = report->boundaries_count == 0 ? NULL :
	    calloc(report->boundaries_count, sizeof(*subtree_boundary_lines));
	live_atoms = report->atoms_count == 0 ? NULL :
	    calloc(report->atoms_count, sizeof(*live_atoms));
	if (report->buffer_generations_count > SIZE_MAX - report->controls_count ||
	    report->buffer_generations_count + report->controls_count >
	    SIZE_MAX / 2 / sizeof(*generation_checkpoints)) {
		fail_report(report, MANT_MANDOC_EXECUTION_ALLOCATION,
		    "native execution generation checkpoint allocation overflow");
		goto fail;
	}
	generation_checkpoint_count =
	    2 * (report->buffer_generations_count + report->controls_count);
	if (!charge_sort_work(report, generation_checkpoint_count))
		goto fail;
	generation_checkpoints = generation_checkpoint_count == 0 ? NULL :
	    calloc(generation_checkpoint_count, sizeof(*generation_checkpoints));
	if ((report->buffer_count != 0 &&
	    (next_generation == NULL || last_capacity == NULL ||
	    last_close_reason == NULL || last_close_sequence == NULL)) ||
	    (report->fragment_atoms_count != 0 && covered_refs == NULL) ||
	    (report->atoms_count != 0 &&
	    (referenced_atoms == NULL || replaced_atoms == NULL ||
	    word_atoms == NULL || margin_atoms == NULL || live_atoms == NULL)) ||
	    (report->nodes_count != 0 && margin_control_nodes == NULL) ||
	    (report->fragments_count != 0 && (covered_fragments == NULL ||
	    covered_glyph_geometry == NULL || fragment_flush_outcome == NULL)) ||
	    (report->references_count != 0 &&
	    last_reference_child_leave == NULL) ||
	    (report->wrappers_count != 0 &&
	    last_wrapper_child_leave == NULL) ||
	    (report->controls_count != 0 &&
	    last_control_child_leave == NULL) ||
	    (report->buffer_generations_count != 0 &&
	    (pending_flush == NULL || terminal_flush == NULL)) ||
	    (generation_checkpoint_count != 0 && generation_checkpoints == NULL) ||
	    (report->boundaries_count != 0 &&
	    (direct_boundary_lines == NULL || subtree_boundary_lines == NULL))) {
		fail_report(report, MANT_MANDOC_EXECUTION_ALLOCATION,
		    "could not validate native execution relationships");
		goto fail;
	}
	last_root_reference_leave = 0;
	last_root_wrapper_leave = 0;
	last_root_control_leave = 0;
	live_atom_count = 0;
	for (index = 0; index < report->buffer_generations_count; index++)
		pending_flush[index] = MANT_MANDOC_EXEC_NONE;
	if (report->sources_count != 1 || report->nodes_count == 0 ||
	    report->node_count != report->nodes_count)
		goto invalid_origin;
	for (index = 0; index < report->sources_count; index++) {
		source = &report->sources[index];
		if (source->key != index || source->flags != 0 ||
		    source->parent != MANT_MANDOC_EXEC_NONE ||
		    source->include_node != MANT_MANDOC_EXEC_NONE ||
		    !valid_pool_range(report, source->path_start,
		    source->path_length, 0) || source->path_length == 0)
			goto invalid_origin;
	}
	for (index = 0; index < report->nodes_count; index++) {
		node = &report->nodes[index];
		if (node->key != index || node->source >= report->sources_count ||
		    (node->parent != MANT_MANDOC_EXEC_NONE &&
		    node->parent >= index) || node->kind > 9 ||
		    node->flags & ~0x03ffU ||
		    !valid_pool_range(report, node->macro_start,
		    node->macro_length, 1))
			goto invalid_origin;
	}
	capacity_total = 0;
	for (index = 0; index < report->buffer_count; index++) {
		if (report->buffers[index].capacity > UINT32_MAX ||
		    capacity_total > UINT64_MAX - report->buffers[index].capacity) {
			fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
			    "native execution buffer capacity accounting overflowed");
			goto fail;
		}
		capacity_total += report->buffers[index].capacity;
	}
	if (capacity_total != report->buffer_cells) {
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
		    "native execution buffer capacity accounting is inconsistent");
		goto fail;
	}
	for (index = 0; index < report->buffer_generations_count; index++) {
		generation = &report->buffer_generations[index];
		if (generation->key != index ||
		    generation->buffer >= report->buffer_count ||
		    generation->generation != next_generation[generation->buffer] ||
		    generation->capacity < last_capacity[generation->buffer] ||
		    generation->capacity > report->buffers[generation->buffer].capacity ||
		    generation->extent > generation->capacity ||
		    (generation->close_reason != MANT_MANDOC_BUFFER_RESET &&
		    generation->close_reason != MANT_MANDOC_BUFFER_REPORT_END) ||
		    generation->reserved != 0 ||
		    generation->open_sequence >= generation->close_sequence ||
		    (next_generation[generation->buffer] != 0 &&
		    (last_close_reason[generation->buffer] !=
		    MANT_MANDOC_BUFFER_RESET ||
		    generation->open_sequence <=
		    last_close_sequence[generation->buffer]))) {
			fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
			    "native execution buffer generation is inconsistent");
			goto fail;
		}
		next_generation[generation->buffer]++;
		last_capacity[generation->buffer] = generation->capacity;
		last_close_reason[generation->buffer] = generation->close_reason;
		last_close_sequence[generation->buffer] =
		    generation->close_sequence;
	}
	for (index = 0; index < report->buffer_count; index++)
		if (next_generation[index] == 0 &&
		    (report->buffers[index].capacity != 0 ||
		    report->buffers[index].generation_record !=
		    MANT_MANDOC_EXEC_NONE)) {
			fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
			    "native execution buffer lifetime is not sealed");
			goto fail;
		}
	for (index = 0; index < report->words_count; index++) {
		word = &report->words[index];
		if (word->key != index ||
		    (index != 0 &&
		    (report->words[index - 1].leave_sequence >=
		    word->enter_sequence ||
		    report->words[index - 1].leave_atom > word->enter_atom)) ||
		    (word->node == MANT_MANDOC_EXEC_NONE ? word->source != 0 :
		    word->node >= report->nodes_count ||
		    word->source != report->nodes[word->node].source) ||
		    !valid_pool_range(report, word->operand_start,
		    word->operand_length, 0) ||
		    (word->role != MANT_MANDOC_ATOM_AUTHORED &&
		    word->role != MANT_MANDOC_ATOM_MACRO_GENERATED &&
		    word->role != MANT_MANDOC_ATOM_DEVICE_GENERATED &&
		    word->role != MANT_MANDOC_ATOM_TABLE_CELL_PAYLOAD) ||
		    word->enter_atom > word->leave_atom ||
		    word->leave_atom > report->atoms_count ||
		    word->reserved != 0 ||
		    word->enter_sequence >= word->leave_sequence ||
		    (word->enter_atom != 0 &&
		    report->atoms[word->enter_atom - 1].sequence >=
		    word->enter_sequence) ||
		    (word->enter_atom < report->atoms_count &&
		    report->atoms[word->enter_atom].sequence <=
		    word->enter_sequence) ||
		    (word->leave_atom != 0 &&
		    report->atoms[word->leave_atom - 1].sequence >=
		    word->leave_sequence) ||
		    (word->leave_atom < report->atoms_count &&
		    report->atoms[word->leave_atom].sequence <=
		    word->leave_sequence))
			goto invalid_word;
		for (inner = word->enter_atom; inner < word->leave_atom; inner++) {
			atom = &report->atoms[inner];
			if (word_atoms[inner] || atom->node != word->node ||
			    atom->source != word->source ||
			    atom->operand_start != word->operand_start ||
			    atom->operand_length != word->operand_length ||
			    atom->wrapper != word->wrapper ||
			    (atom->role != word->role &&
			    atom->role != MANT_MANDOC_ATOM_IMPLICIT_SPACE &&
			    atom->role != MANT_MANDOC_ATOM_FONT_DECORATION))
				goto invalid_word;
			word_atoms[inner] = 1;
		}
	}
	for (index = 0; index < report->atoms_count; index++) {
		atom = &report->atoms[index];
		if (atom->key != index ||
		    (index != 0 && report->atoms[index - 1].sequence >=
		    atom->sequence) ||
		    atom->kind < MANT_MANDOC_ATOM_GLYPH ||
		    atom->kind > MANT_MANDOC_ATOM_BREAK_POINT ||
		    atom->role < MANT_MANDOC_ATOM_AUTHORED ||
		    atom->role > MANT_MANDOC_ATOM_TABLE_CELL_PAYLOAD ||
		    atom->font > MANT_MANDOC_FONT_BOLD_UNDERLINE ||
		    atom->disposition < MANT_MANDOC_ATOM_EMITTED ||
		    atom->disposition > MANT_MANDOC_ATOM_TRAILING_DISCARD ||
		    !valid_scalar(atom->input_scalar) ||
		    !valid_scalar(atom->display_scalar) ||
		    !valid_pool_range(report, atom->operand_start,
		    atom->operand_length, 1) ||
		    (atom->node == MANT_MANDOC_EXEC_NONE ? atom->source != 0 :
		    atom->node >= report->nodes_count ||
		    atom->source != report->nodes[atom->node].source) ||
		    ((atom->operand_start != MANT_MANDOC_EXEC_NONE) !=
		    (word_atoms[index] != 0)) ||
		    ((atom->disposition == MANT_MANDOC_ATOM_REPLACED) !=
		    (atom->replaced_by != MANT_MANDOC_EXEC_NONE)))
			goto invalid_atom;
		if (atom->replaced_by != MANT_MANDOC_EXEC_NONE) {
			if (atom->replaced_by <= atom->key ||
			    atom->replaced_by >= report->atoms_count ||
			    replaced_atoms[atom->replaced_by])
				goto invalid_atom;
			replaced_atoms[atom->replaced_by] = 1;
			if (report->atoms[atom->replaced_by].buffer != atom->buffer ||
			    report->atoms[atom->replaced_by].generation !=
			    atom->generation ||
			    report->atoms[atom->replaced_by].buffer_generation !=
			    atom->buffer_generation ||
			    report->atoms[atom->replaced_by].slot != atom->slot ||
			    report->atoms[atom->replaced_by].sequence <=
			    atom->sequence)
				goto invalid_atom;
		}
		if (atom->buffer == MANT_MANDOC_EXEC_NONE ||
		    atom->generation == MANT_MANDOC_EXEC_NONE ||
		    atom->buffer_generation == MANT_MANDOC_EXEC_NONE ||
		    atom->slot == MANT_MANDOC_EXEC_NONE) {
			if (atom->buffer != MANT_MANDOC_EXEC_NONE ||
			    atom->generation != MANT_MANDOC_EXEC_NONE ||
			    atom->buffer_generation != MANT_MANDOC_EXEC_NONE ||
			    atom->slot != MANT_MANDOC_EXEC_NONE)
				goto invalid_atom;
			if (atom->disposition != MANT_MANDOC_ATOM_EMITTED)
				goto invalid_atom;
			continue;
		}
		if (atom->buffer_generation >= report->buffer_generations_count)
			goto invalid_atom;
		generation = &report->buffer_generations[atom->buffer_generation];
		if (generation->buffer != atom->buffer ||
		    generation->generation != atom->generation ||
		    atom->slot >= generation->extent ||
		    atom->sequence <= generation->open_sequence ||
		    atom->sequence >= generation->close_sequence)
			goto invalid_atom;
		live_atoms[live_atom_count].buffer_generation =
		    atom->buffer_generation;
		live_atoms[live_atom_count].slot = atom->slot;
		live_atoms[live_atom_count].atom = atom->key;
		live_atom_count++;
		continue;
invalid_atom:
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
		    "native execution atom buffer relationship is inconsistent");
		goto fail;
	}
	if (live_atom_count > 1 &&
	    !charge_sort_work(report, live_atom_count))
		goto fail;
	if (live_atom_count > 1)
		qsort(live_atoms, live_atom_count, sizeof(*live_atoms),
		    compare_live_atom_location);
	/*
	 * Validate the complete write history of each slot.  A replacement must
	 * point to the immediately following write.  Consumed or discarded atoms
	 * instead end one occupancy epoch, allowing a later write to reuse the
	 * cleared slot without a replacement edge.  The final write is the current
	 * occupant used by field validation below.
	 */
	inner = 0;
	for (index = 0; index < live_atom_count;) {
		size_t group_end, previous;

		group_end = index + 1;
		while (group_end < live_atom_count &&
		    live_atoms[group_end].buffer_generation ==
		    live_atoms[index].buffer_generation &&
		    live_atoms[group_end].slot == live_atoms[index].slot)
			group_end++;
		for (previous = index; previous + 1 < group_end; previous++) {
			atom = &report->atoms[live_atoms[previous].atom];
			if (atom->disposition == MANT_MANDOC_ATOM_REPLACED) {
				if (atom->replaced_by !=
				    live_atoms[previous + 1].atom)
					goto invalid_atom;
			} else if (atom->disposition !=
			    MANT_MANDOC_ATOM_CONSUMED && atom->disposition !=
			    MANT_MANDOC_ATOM_TRAILING_DISCARD)
				goto invalid_atom;
		}
		if (report->atoms[live_atoms[group_end - 1].atom].disposition ==
		    MANT_MANDOC_ATOM_REPLACED)
			goto invalid_atom;
		live_atoms[inner++] = live_atoms[group_end - 1];
		index = group_end;
	}
	live_atom_count = inner;
	for (index = 0; index < report->fragments_count; index++) {
		fragment = &report->fragments[index];
		if (fragment->key != index ||
		    (index != 0 && report->fragments[index - 1].sequence >=
		    fragment->sequence) || fragment->atom_ref_length != 1 ||
		    fragment->atom_ref_start > report->fragment_atoms_count ||
		    fragment->atom_ref_length > report->fragment_atoms_count -
		    fragment->atom_ref_start || fragment->reserved != 0 ||
		    fragment->role < MANT_MANDOC_FRAGMENT_CONTENT ||
		    fragment->role > MANT_MANDOC_FRAGMENT_PAGE_DECORATION ||
		    fragment->start_bu < 0 || fragment->end_bu < 0 ||
		    (fragment->node != MANT_MANDOC_EXEC_NONE &&
		    fragment->node >= report->nodes_count))
			goto invalid_fragment;
		end = fragment->atom_ref_start + fragment->atom_ref_length;
		for (inner = fragment->atom_ref_start; inner < end; inner++) {
			reference = &report->fragment_atoms[inner];
			if (covered_refs[inner] || reference->fragment != index ||
			    reference->atom >= report->atoms_count ||
			    referenced_atoms[reference->atom])
				goto invalid_fragment;
			covered_refs[inner] = 1;
			referenced_atoms[reference->atom] = 1;
			atom = &report->atoms[reference->atom];
			if (atom->disposition != MANT_MANDOC_ATOM_EMITTED ||
			    atom->node != fragment->node ||
			    atom->buffer != fragment->buffer ||
			    atom->generation != fragment->generation ||
			    atom->buffer_generation != fragment->buffer_generation)
				goto invalid_fragment;
		}
		if (fragment->buffer == MANT_MANDOC_EXEC_NONE ||
		    fragment->generation == MANT_MANDOC_EXEC_NONE ||
		    fragment->buffer_generation == MANT_MANDOC_EXEC_NONE) {
			if (fragment->buffer != MANT_MANDOC_EXEC_NONE ||
			    fragment->generation != MANT_MANDOC_EXEC_NONE ||
			    fragment->buffer_generation != MANT_MANDOC_EXEC_NONE)
				goto invalid_fragment;
		} else {
			if (fragment->buffer_generation >=
			    report->buffer_generations_count)
				goto invalid_fragment;
			generation = &report->buffer_generations[
			    fragment->buffer_generation];
			if (generation->buffer != fragment->buffer ||
			    generation->generation != fragment->generation ||
			    fragment->sequence <= generation->open_sequence ||
			    fragment->sequence >= generation->close_sequence)
				goto invalid_fragment;
		}
		continue;
invalid_fragment:
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
		    "native execution fragment relationship is inconsistent");
		goto fail;
	}
	for (index = 0; index < report->fragment_atoms_count; index++) {
		if (!covered_refs[index]) {
			fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
			    "native execution fragment reference is unclaimed");
			goto fail;
		}
	}
	for (index = 0; index < report->atoms_count; index++)
		if (report->atoms[index].buffer == MANT_MANDOC_EXEC_NONE &&
		    !referenced_atoms[index]) {
			fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
			    "native direct execution atom has no fragment");
			goto fail;
		}
	for (index = 0; index < report->flushes_count; index++) {
		flush = &report->flushes[index];
		if (flush->key != index ||
		    (index != 0 && report->flushes[index - 1].outcome_sequence >=
		    flush->sequence) ||
		    (flush->node != MANT_MANDOC_EXEC_NONE &&
		    flush->node >= report->nodes_count) ||
		    flush->buffer_generation >= report->buffer_generations_count ||
		    flush->outcome < MANT_MANDOC_FLUSH_NO_CONTENT ||
		    flush->outcome > MANT_MANDOC_FLUSH_DEFERRED_COLUMN ||
		    flush->flags_before & ~0x7fffffU ||
		    flush->flags_after & ~0x7fffffU ||
		    (flush->boundary != MANT_MANDOC_EXEC_NONE &&
		    flush->boundary >= report->boundaries_count) ||
		    flush->sequence >= flush->outcome_sequence)
			goto invalid_flush;
		generation = &report->buffer_generations[flush->buffer_generation];
		if (terminal_flush[flush->buffer_generation])
			goto invalid_flush;
		first_flush = pending_flush[flush->buffer_generation] ==
		    MANT_MANDOC_EXEC_NONE;
		if (pending_flush[flush->buffer_generation] !=
		    MANT_MANDOC_EXEC_NONE) {
			next_flush = &report->flushes[
			    pending_flush[flush->buffer_generation]];
			if (flush->scan_start != next_flush->remaining_start ||
			    flush->remaining_end != next_flush->remaining_end ||
			    next_flush->outcome_sequence >= flush->sequence ||
			    flush->taboff_before != next_flush->taboff_after)
				goto invalid_flush;
			pending_flush[flush->buffer_generation] =
			    MANT_MANDOC_EXEC_NONE;
		} else if (!terminal_flush[flush->buffer_generation] &&
		    flush->scan_start != 0)
			goto invalid_flush;
		if (generation->buffer != flush->buffer ||
		    generation->generation != flush->generation ||
		    flush->sequence <= generation->open_sequence ||
		    flush->outcome_sequence >= generation->close_sequence ||
		    flush->leading_bu < 0 || flush->content_bu < 0 ||
		    flush->field_bu < 0 || flush->target_bu < 0 ||
		    flush->taboff_before < 0 || flush->taboff_after < 0 ||
		    flush->visual_before < 0 || flush->visual_after < 0 ||
		    flush->scan_start != flush->accepted_start ||
		    flush->scan_start != flush->consumed_start ||
		    flush->accepted_end != flush->consumed_end ||
		    flush->accepted_end != flush->tail_discarded_start ||
		    flush->accepted_start > flush->accepted_end ||
		    flush->accepted_end > flush->scan_end ||
		    flush->scan_end > flush->remaining_end ||
		    flush->tail_discarded_start >
		    flush->tail_discarded_end ||
		    flush->tail_discarded_end != flush->remaining_start ||
		    flush->remaining_start > flush->remaining_end ||
		    flush->remaining_end > generation->extent ||
		    flush->fragment_start > report->fragments_count ||
		    flush->fragment_length > report->fragments_count -
		    flush->fragment_start)
			goto invalid_flush;
		end = flush->fragment_start + flush->fragment_length;
		for (inner = flush->fragment_start; inner < end; inner++) {
			fragment = &report->fragments[inner];
			if (covered_fragments[inner] ||
			    fragment->buffer_generation != flush->buffer_generation ||
			    fragment->sequence <= flush->sequence ||
			    fragment->sequence >= flush->outcome_sequence ||
			    fragment->atom_ref_length != 1)
				goto invalid_flush;
			reference = &report->fragment_atoms[
			    fragment->atom_ref_start];
			if (reference->atom >= report->atoms_count)
				goto invalid_flush;
			atom = &report->atoms[reference->atom];
			if (atom->slot == MANT_MANDOC_EXEC_NONE ||
			    atom->slot < flush->accepted_start ||
			    atom->slot >= flush->accepted_end)
				goto invalid_flush;
			covered_fragments[inner] = 1;
			fragment_flush_outcome[inner] = flush->outcome_sequence;
		}
		if (flush->outcome == MANT_MANDOC_FLUSH_NO_CONTENT) {
			if (flush->accepted_start != flush->accepted_end ||
			    flush->consumed_start != flush->consumed_end ||
			    flush->remaining_start != flush->remaining_end ||
			    flush->fragment_length != 0)
				goto invalid_flush;
		} else if (flush->accepted_start == flush->accepted_end)
			goto invalid_flush;
		if ((flush->outcome == MANT_MANDOC_FLUSH_WRAPPED ||
		    flush->outcome == MANT_MANDOC_FLUSH_DEFERRED_COLUMN) &&
		    flush->remaining_start == flush->remaining_end)
			goto invalid_flush;
		if ((flush->outcome == MANT_MANDOC_FLUSH_NO_CONTENT ||
		    flush->outcome == MANT_MANDOC_FLUSH_EXHAUSTED) &&
		    (flush->remaining_start != flush->remaining_end ||
		    generation->close_reason != MANT_MANDOC_BUFFER_RESET))
			goto invalid_flush;
		end = flush->accepted_start;
		live_index = lower_bound_live_atom(live_atoms, live_atom_count,
		    flush->buffer_generation, flush->accepted_start);
		for (; live_index < live_atom_count; live_index++) {
			struct live_atom_location *location =
			    &live_atoms[live_index];

			if (location->buffer_generation !=
			    flush->buffer_generation ||
			    location->slot >= flush->tail_discarded_end)
				break;
			atom = &report->atoms[location->atom];
			if (atom->sequence >= flush->sequence)
				goto invalid_flush;
			if (location->slot < flush->accepted_end) {
				if (location->slot != end ||
				    (atom->disposition != MANT_MANDOC_ATOM_EMITTED &&
				    atom->disposition != MANT_MANDOC_ATOM_CONSUMED &&
				    atom->disposition !=
				    MANT_MANDOC_ATOM_TRAILING_DISCARD))
					goto invalid_flush;
				end++;
			} else if (flush->outcome ==
			    MANT_MANDOC_FLUSH_NO_CONTENT || flush->outcome ==
			    MANT_MANDOC_FLUSH_EXHAUSTED) {
				if (location->slot != end ||
				    !terminal_tail_scalar(atom->display_scalar))
					goto invalid_flush;
				if (atom->disposition !=
				    MANT_MANDOC_ATOM_TRAILING_DISCARD)
					goto invalid_flush;
				end++;
			} else {
				if (location->slot != end || atom->display_scalar != ' ' ||
				    atom->disposition != MANT_MANDOC_ATOM_CONSUMED)
					goto invalid_flush;
				end++;
			}
		}
		if (end != flush->tail_discarded_end)
			goto invalid_flush;
		if (first_flush) {
			end = 0;
			live_index = lower_bound_live_atom(live_atoms,
			    live_atom_count, flush->buffer_generation, 0);
			for (; live_index < live_atom_count; live_index++) {
				struct live_atom_location *location =
				    &live_atoms[live_index];

				if (location->buffer_generation !=
				    flush->buffer_generation ||
				    location->slot >= flush->remaining_end)
					break;
				atom = &report->atoms[location->atom];
				if (location->slot != end ||
				    atom->sequence >= flush->sequence)
					goto invalid_flush;
				end++;
			}
			if (end != flush->remaining_end)
				goto invalid_flush;
		}
		if (flush->outcome == MANT_MANDOC_FLUSH_WRAPPED) {
			if (index + 1 >= report->flushes_count)
				goto invalid_flush;
			next_flush = &report->flushes[index + 1];
			if (next_flush->buffer_generation != flush->buffer_generation ||
			    next_flush->scan_start != flush->remaining_start ||
			    next_flush->remaining_end != flush->remaining_end ||
			    flush->outcome_sequence >= next_flush->sequence ||
			    next_flush->taboff_before != flush->taboff_after)
				goto invalid_flush;
		}
		if (flush->outcome == MANT_MANDOC_FLUSH_WRAPPED ||
		    flush->outcome == MANT_MANDOC_FLUSH_DEFERRED_COLUMN)
			pending_flush[flush->buffer_generation] = flush->key;
		else
			terminal_flush[flush->buffer_generation] = 1;
		if (flush->boundary != MANT_MANDOC_EXEC_NONE &&
		    (report->boundaries[flush->boundary].enter_sequence >=
		    flush->sequence || report->boundaries[flush->boundary].leave_sequence <=
		    flush->outcome_sequence))
			goto invalid_flush;
		continue;
invalid_flush:
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
		    "native execution flush relationship is inconsistent");
		goto fail;
	}
	for (index = 0; index < report->buffer_generations_count; index++)
		if (pending_flush[index] != MANT_MANDOC_EXEC_NONE)
			goto invalid_flush_after_loop;
	for (index = 0; index < report->fragments_count; index++) {
		fragment = &report->fragments[index];
		if ((fragment->buffer_generation != MANT_MANDOC_EXEC_NONE) !=
		    covered_fragments[index]) {
			fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
			    "native execution fragments are not partitioned by flushes");
			goto fail;
		}
	}
	for (index = 0; index < report->boundaries_count; index++) {
		boundary = &report->boundaries[index];
		if (boundary->key != index ||
		    (boundary->node != MANT_MANDOC_EXEC_NONE &&
		    boundary->node >= report->nodes_count) ||
		    (boundary->parent != MANT_MANDOC_EXEC_NONE &&
		    boundary->parent >= index) ||
		    boundary->request < MANT_MANDOC_BOUNDARY_NEWLINE ||
		    boundary->request > MANT_MANDOC_BOUNDARY_DEVICE_ENDLINE ||
		    boundary->effect > MANT_MANDOC_BOUNDARY_ADDED_VERTICAL_SPACE ||
		    boundary->flags_before & ~0x7fffffU ||
		    boundary->flags_after & ~0x7fffffU ||
		    (boundary->control != MANT_MANDOC_EXEC_NONE &&
		    boundary->control >= report->controls_count) ||
		    (boundary->wrapper != MANT_MANDOC_EXEC_NONE &&
		    boundary->wrapper >= report->wrappers_count) ||
		    boundary->enter_sequence >= boundary->leave_sequence ||
		    boundary->line_before < 0 || boundary->line_after < 0 ||
		    boundary->line_after < boundary->line_before ||
		    boundary->visual_before < 0 || boundary->visual_after < 0)
			goto invalid_boundary;
		if ((boundary->wrapper == MANT_MANDOC_EXEC_NONE) !=
		    (boundary->node == MANT_MANDOC_EXEC_NONE))
			goto invalid_boundary;
		if (boundary->wrapper != MANT_MANDOC_EXEC_NONE) {
			wrapper = &report->wrappers[boundary->wrapper];
			if (wrapper->kind != MANT_MANDOC_WRAPPER_NODE ||
			    wrapper->node != boundary->node ||
			    wrapper->enter_sequence >= boundary->enter_sequence ||
			    wrapper->leave_sequence <= boundary->leave_sequence)
				goto invalid_boundary;
		}
		if (boundary->control != MANT_MANDOC_EXEC_NONE &&
		    (report->controls[boundary->control].enter_sequence >=
		    boundary->enter_sequence ||
		    report->controls[boundary->control].leave_sequence <=
		    boundary->leave_sequence))
			goto invalid_boundary;
		if (boundary->parent != MANT_MANDOC_EXEC_NONE &&
		    (report->boundaries[boundary->parent].enter_sequence >=
		    boundary->enter_sequence ||
		    report->boundaries[boundary->parent].leave_sequence <=
		    boundary->leave_sequence))
			goto invalid_boundary;
		if (boundary->request == MANT_MANDOC_BOUNDARY_DEVICE_ENDLINE &&
		    boundary->parent != MANT_MANDOC_EXEC_NONE) {
			if (direct_boundary_lines[boundary->parent] == UINT32_MAX)
				goto invalid_boundary;
			direct_boundary_lines[boundary->parent]++;
		}
	}
	for (index = report->boundaries_count; index > 0; index--) {
		uint32_t lines;

		boundary = &report->boundaries[index - 1];
		lines = boundary->request == MANT_MANDOC_BOUNDARY_DEVICE_ENDLINE ?
		    1 : subtree_boundary_lines[index - 1];
		if (boundary->parent != MANT_MANDOC_EXEC_NONE) {
			if (subtree_boundary_lines[boundary->parent] >
			    UINT32_MAX - lines)
				goto invalid_boundary;
			subtree_boundary_lines[boundary->parent] += lines;
		}
	}
	for (index = 0; index < report->boundaries_count; index++) {
		boundary = &report->boundaries[index];
		if (boundary->direct_device_lines != direct_boundary_lines[index])
			goto invalid_boundary;
		if (boundary->request == MANT_MANDOC_BOUNDARY_DEVICE_ENDLINE) {
			if (boundary->effect != MANT_MANDOC_BOUNDARY_ENDED_LINE ||
			    boundary->line_before == INT64_MAX ||
			    boundary->line_after != boundary->line_before + 1)
				goto invalid_boundary;
		} else if ((uint64_t)(boundary->line_after - boundary->line_before) !=
		    subtree_boundary_lines[index])
			goto invalid_boundary;
		else if (boundary->request == MANT_MANDOC_BOUNDARY_VERTICAL_SPACE &&
		    boundary->direct_device_lines != 0) {
			if (boundary->effect !=
			    MANT_MANDOC_BOUNDARY_ADDED_VERTICAL_SPACE)
				goto invalid_boundary;
		} else if (boundary->line_after > boundary->line_before) {
			if (boundary->effect != MANT_MANDOC_BOUNDARY_ENDED_LINE)
				goto invalid_boundary;
		} else if (boundary->visual_after != boundary->visual_before) {
			if (boundary->effect != MANT_MANDOC_BOUNDARY_FLUSHED)
				goto invalid_boundary;
		} else if (boundary->effect != MANT_MANDOC_BOUNDARY_NO_OUTPUT)
			goto invalid_boundary;
	}
	active_control = MANT_MANDOC_EXEC_NONE;
	for (index = 0; index < report->controls_count; index++) {
		uint64_t *last_sibling_leave;
		const char *control_name;

		control = &report->controls[index];
		if (control->key != index || control->node >= report->nodes_count ||
		    control->wrapper >= report->wrappers_count ||
		    control->request < MANT_MANDOC_CONTROL_BREAK ||
		    control->request > MANT_MANDOC_CONTROL_TEMPORARY_INDENT ||
		    control->buffer >= report->buffer_count ||
		    control->reserved != 0 ||
		    control->flags_before & ~0x7fffffU ||
		    control->flags_after & ~0x7fffffU ||
		    control->line_before < 0 || control->line_after < 0 ||
		    control->visual_before < 0 || control->visual_after < 0 ||
		    control->column_before < 0 || control->column_after < 0 ||
		    control->extent_before < 0 || control->extent_after < 0 ||
		    control->offset_before < 0 || control->offset_after < 0 ||
		    control->rmargin_before < 0 || control->rmargin_after < 0 ||
		    control->maxrmargin_before < 0 || control->maxrmargin_after < 0 ||
		    control->taboff_before < 0 || control->taboff_after < 0 ||
		    control->minimum_blank_before < 0 ||
		    control->minimum_blank_after < 0 ||
		    control->trailing_blank_before < 0 ||
		    control->trailing_blank_after < 0 ||
		    control->enter_sequence >= control->leave_sequence ||
		    control->atom_start > report->atoms_count ||
		    control->atom_length > report->atoms_count - control->atom_start ||
		    control->fragment_start > report->fragments_count ||
		    control->fragment_length > report->fragments_count -
		    control->fragment_start ||
		    control->flush_start > report->flushes_count ||
		    control->flush_length > report->flushes_count - control->flush_start ||
		    control->boundary_start > report->boundaries_count ||
		    control->boundary_length > report->boundaries_count -
		    control->boundary_start ||
		    control->geometry_start > report->geometries_count ||
		    control->geometry_length > report->geometries_count -
		    control->geometry_start ||
		    control->wrapper_start > report->wrappers_count ||
		    control->wrapper_length > report->wrappers_count -
		    control->wrapper_start)
			goto invalid_control;
		control_name = stable_control_name(control->request);
		if (control_name == NULL ||
		    report->nodes[control->node].macro_length != strlen(control_name) ||
		    memcmp(report->pool + report->nodes[control->node].macro_start,
		    control_name, strlen(control_name)) != 0)
			goto invalid_control;
		wrapper = &report->wrappers[control->wrapper];
		origin = control->node;
		while (origin != MANT_MANDOC_EXEC_NONE && origin != wrapper->node)
			origin = report->nodes[origin].parent;
		if (wrapper->kind != MANT_MANDOC_WRAPPER_NODE ||
		    origin != wrapper->node ||
		    wrapper->enter_sequence >= control->enter_sequence ||
		    wrapper->leave_sequence <= control->leave_sequence ||
		    wrapper->enter_atom > control->atom_start ||
		    control->atom_start > wrapper->leave_atom ||
		    control->atom_length > wrapper->leave_atom -
		    control->atom_start)
			goto invalid_control;
		if ((control->generation_before != MANT_MANDOC_EXEC_NONE &&
		    (control->generation_before >= report->buffer_generations_count ||
		    report->buffer_generations[control->generation_before].buffer !=
		    control->buffer ||
		    report->buffer_generations[control->generation_before].open_sequence >=
		    control->enter_sequence ||
		    report->buffer_generations[control->generation_before].close_sequence <=
		    control->enter_sequence)) ||
		    (control->generation_after != MANT_MANDOC_EXEC_NONE &&
		    (control->generation_after >= report->buffer_generations_count ||
		    report->buffer_generations[control->generation_after].buffer !=
		    control->buffer ||
		    report->buffer_generations[control->generation_after].open_sequence >=
		    control->leave_sequence ||
		    report->buffer_generations[control->generation_after].close_sequence <=
		    control->leave_sequence)))
			goto invalid_control;
		while (active_control != MANT_MANDOC_EXEC_NONE &&
		    report->controls[active_control].leave_sequence <=
		    control->enter_sequence)
			active_control = report->controls[active_control].parent;
		if (control->parent != active_control)
			goto invalid_control;
		if (control->parent == MANT_MANDOC_EXEC_NONE)
			last_sibling_leave = &last_root_control_leave;
		else {
			if (control->parent >= index ||
			    report->controls[control->parent].enter_sequence >=
			    control->enter_sequence ||
			    report->controls[control->parent].leave_sequence <=
			    control->leave_sequence)
				goto invalid_control;
			last_sibling_leave = &last_control_child_leave[control->parent];
		}
		if (*last_sibling_leave != 0 &&
		    control->enter_sequence <= *last_sibling_leave)
			goto invalid_control;
		*last_sibling_leave = control->leave_sequence;
#define VALIDATE_CONTROL_RANGE(table, table_count, field, sequence_field) do { \
	size_t range_start = control->field##_start; \
	size_t range_end = range_start + control->field##_length; \
	if ((range_start != 0 && \
	    report->table[range_start - 1].sequence_field >= \
	    control->enter_sequence) || \
	    (range_start < report->table_count && \
	    report->table[range_start].sequence_field <= \
	    control->enter_sequence) || \
	    (range_end != 0 && \
	    report->table[range_end - 1].sequence_field >= \
	    control->leave_sequence) || \
	    (range_end < report->table_count && \
	    report->table[range_end].sequence_field <= \
	    control->leave_sequence)) \
		goto invalid_control; \
} while (0)
		VALIDATE_CONTROL_RANGE(atoms, atoms_count, atom, sequence);
		VALIDATE_CONTROL_RANGE(fragments, fragments_count, fragment, sequence);
		VALIDATE_CONTROL_RANGE(flushes, flushes_count, flush, sequence);
		VALIDATE_CONTROL_RANGE(boundaries, boundaries_count, boundary,
		    enter_sequence);
		VALIDATE_CONTROL_RANGE(geometries, geometries_count, geometry, sequence);
		VALIDATE_CONTROL_RANGE(wrappers, wrappers_count, wrapper, enter_sequence);
#undef VALIDATE_CONTROL_RANGE
		if (control->request == MANT_MANDOC_CONTROL_MARGIN_CHARACTER)
			margin_control_nodes[control->node] = 1;
		active_control = control->key;
	}
	inner = 0;
	for (index = 0; index < report->buffer_generations_count; index++) {
		generation = &report->buffer_generations[index];
		generation_checkpoints[inner].sequence = generation->open_sequence;
		generation_checkpoints[inner].buffer = generation->buffer;
		generation_checkpoints[inner].generation = generation->key;
		generation_checkpoints[inner++].kind = GENERATION_OPEN;
		generation_checkpoints[inner].sequence = generation->close_sequence;
		generation_checkpoints[inner].buffer = generation->buffer;
		generation_checkpoints[inner].generation = generation->key;
		generation_checkpoints[inner++].kind = GENERATION_CLOSE;
	}
	for (index = 0; index < report->controls_count; index++) {
		control = &report->controls[index];
		generation_checkpoints[inner].sequence = control->enter_sequence;
		generation_checkpoints[inner].buffer = control->buffer;
		generation_checkpoints[inner].generation = control->generation_before;
		generation_checkpoints[inner++].kind = GENERATION_CHECK;
		generation_checkpoints[inner].sequence = control->leave_sequence;
		generation_checkpoints[inner].buffer = control->buffer;
		generation_checkpoints[inner].generation = control->generation_after;
		generation_checkpoints[inner++].kind = GENERATION_CHECK;
	}
	if (inner != generation_checkpoint_count)
		goto invalid_control;
	qsort(generation_checkpoints, generation_checkpoint_count,
	    sizeof(*generation_checkpoints), compare_generation_checkpoint);
	for (index = 0; index < report->buffer_count; index++)
		next_generation[index] = MANT_MANDOC_EXEC_NONE;
	for (index = 0; index < generation_checkpoint_count; index++) {
		struct generation_checkpoint *checkpoint =
		    &generation_checkpoints[index];

		if ((index != 0 && generation_checkpoints[index - 1].sequence >=
		    checkpoint->sequence) || checkpoint->buffer >= report->buffer_count)
			goto invalid_control;
		switch (checkpoint->kind) {
		case GENERATION_OPEN:
			if (next_generation[checkpoint->buffer] !=
			    MANT_MANDOC_EXEC_NONE)
				goto invalid_control;
			next_generation[checkpoint->buffer] = checkpoint->generation;
			break;
		case GENERATION_CLOSE:
			if (next_generation[checkpoint->buffer] != checkpoint->generation)
				goto invalid_control;
			next_generation[checkpoint->buffer] = MANT_MANDOC_EXEC_NONE;
			break;
		case GENERATION_CHECK:
			if (next_generation[checkpoint->buffer] != checkpoint->generation)
				goto invalid_control;
			break;
		default:
			goto invalid_control;
		}
	}
	active_control = MANT_MANDOC_EXEC_NONE;
	control_cursor = 0;
	last_boundary_enter = 0;
	for (index = 0; index < report->boundaries_count; index++) {
		boundary = &report->boundaries[index];
		if (index != 0 && boundary->enter_sequence <= last_boundary_enter)
			goto invalid_boundary;
		while (control_cursor < report->controls_count &&
		    report->controls[control_cursor].enter_sequence <
		    boundary->enter_sequence) {
			control = &report->controls[control_cursor];
			while (active_control != MANT_MANDOC_EXEC_NONE &&
			    report->controls[active_control].leave_sequence <=
			    control->enter_sequence)
				active_control =
				    report->controls[active_control].parent;
			active_control = control->key;
			control_cursor++;
		}
		while (active_control != MANT_MANDOC_EXEC_NONE &&
		    report->controls[active_control].leave_sequence <=
		    boundary->enter_sequence)
			active_control = report->controls[active_control].parent;
		if (boundary->control != active_control)
			goto invalid_boundary;
		last_boundary_enter = boundary->enter_sequence;
	}
	for (index = 0; index < report->nodes_count; index++)
		if (report->nodes[index].parent != MANT_MANDOC_EXEC_NONE &&
		    margin_control_nodes[report->nodes[index].parent])
			margin_control_nodes[index] = 1;
	for (index = 0; index < report->geometries_count; index++) {
		geometry = &report->geometries[index];
		if (geometry->key != index ||
		    (geometry->node != MANT_MANDOC_EXEC_NONE &&
		    geometry->node >= report->nodes_count) ||
		    geometry->kind < MANT_MANDOC_GEOMETRY_ADVANCE ||
		    geometry->kind > MANT_MANDOC_GEOMETRY_FIELD ||
		    geometry->unit < MANT_MANDOC_UNIT_BASIC ||
		    geometry->unit > MANT_MANDOC_UNIT_DEVICE_LINE ||
		    geometry->origin_kind > MANT_MANDOC_GEOMETRY_ORIGIN_FRAGMENT ||
		    geometry->reserved != 0)
			goto invalid_geometry;
		switch (geometry->kind) {
		case MANT_MANDOC_GEOMETRY_ADVANCE:
			if (geometry->unit != MANT_MANDOC_UNIT_BASIC ||
			    geometry->requested < 0 || geometry->before < 0 ||
			    geometry->after < 0 || geometry->effective !=
			    geometry->after - geometry->before ||
			    (geometry->origin_kind ==
			    MANT_MANDOC_GEOMETRY_ORIGIN_NONE ?
			    geometry->origin_key != MANT_MANDOC_EXEC_NONE ||
			    geometry->related != MANT_MANDOC_EXEC_NONE :
			    geometry->origin_kind !=
			    MANT_MANDOC_GEOMETRY_ORIGIN_FLUSH ||
			    geometry->origin_key >= report->flushes_count ||
			    geometry->related != geometry->origin_key ||
			    geometry->node != report->flushes[
			    geometry->origin_key].node || geometry->sequence <=
			    report->flushes[geometry->origin_key].sequence ||
			    geometry->sequence >= report->flushes[
			    geometry->origin_key].outcome_sequence))
				goto invalid_advance_geometry;
			continue;
		invalid_advance_geometry:
			fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
			    "native execution advance geometry is inconsistent");
			goto fail;
		case MANT_MANDOC_GEOMETRY_ENDLINE:
			if (geometry->unit != MANT_MANDOC_UNIT_DEVICE_LINE ||
			    geometry->requested != 1 || geometry->effective != 1 ||
			    geometry->before < 0 || geometry->after < 0 ||
			    geometry->origin_kind !=
			    MANT_MANDOC_GEOMETRY_ORIGIN_BOUNDARY ||
			    geometry->origin_key >= report->boundaries_count ||
			    geometry->related != geometry->origin_key ||
			    report->boundaries[geometry->origin_key].request !=
			    MANT_MANDOC_BOUNDARY_DEVICE_ENDLINE ||
			    report->boundaries[geometry->origin_key].effect !=
			    MANT_MANDOC_BOUNDARY_ENDED_LINE ||
			    geometry->node != report->boundaries[
			    geometry->origin_key].node || geometry->before !=
			    report->boundaries[geometry->origin_key].line_before ||
			    geometry->after != report->boundaries[
			    geometry->origin_key].line_after || geometry->sequence <=
			    report->boundaries[geometry->origin_key].enter_sequence ||
			    geometry->sequence >= report->boundaries[
			    geometry->origin_key].leave_sequence)
				goto invalid_endline_geometry;
			continue;
		invalid_endline_geometry:
			fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
			    "native execution endline geometry is inconsistent");
			goto fail;
		case MANT_MANDOC_GEOMETRY_FIELD:
			if (geometry->unit != MANT_MANDOC_UNIT_BASIC ||
			    geometry->before < 0 || geometry->after < 0 ||
			    geometry->origin_kind !=
			    MANT_MANDOC_GEOMETRY_ORIGIN_FLUSH ||
			    geometry->origin_key >= report->flushes_count ||
			    geometry->related != geometry->origin_key ||
			    geometry->node != report->flushes[
			    geometry->origin_key].node || geometry->sequence <=
			    report->flushes[geometry->origin_key].sequence ||
			    geometry->sequence >= report->flushes[
			    geometry->origin_key].outcome_sequence)
				goto invalid_field_geometry;
			continue;
		invalid_field_geometry:
			fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
			    "native execution field geometry is inconsistent");
			goto fail;
		case MANT_MANDOC_GEOMETRY_GLYPH:
			break;
		default:
			goto invalid_geometry;
		}
		if (geometry->origin_kind != MANT_MANDOC_GEOMETRY_ORIGIN_ATOM ||
		    geometry->origin_key >= report->atoms_count ||
		    geometry->related >= report->fragments_count ||
		    covered_glyph_geometry[geometry->related])
			goto invalid_geometry;
		fragment = &report->fragments[geometry->related];
		reference = &report->fragment_atoms[fragment->atom_ref_start];
		if (fragment->start_bu < 0 || fragment->end_bu < 0 ||
		    reference->atom != geometry->origin_key ||
		    geometry->node != fragment->node ||
		    geometry->requested != fragment->end_bu - fragment->start_bu ||
		    geometry->effective != geometry->requested ||
		    geometry->before != fragment->start_bu ||
		    geometry->after != fragment->end_bu ||
		    geometry->sequence <= fragment->sequence ||
		    (fragment_flush_outcome[geometry->related] != 0 &&
		    geometry->sequence >=
		    fragment_flush_outcome[geometry->related])) {
invalid_geometry:
			fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
			    "native execution geometry is inconsistent");
			goto fail;
		}
		covered_glyph_geometry[geometry->related] = 1;
	}
	for (index = 0; index < report->fragments_count; index++)
		if (!covered_glyph_geometry[index])
			goto invalid_geometry;
	active_wrapper = MANT_MANDOC_EXEC_NONE;
	for (index = 0; index < report->wrappers_count; index++) {
		uint64_t *last_sibling_leave;

		wrapper = &report->wrappers[index];
		if (wrapper->key != index ||
		    (index != 0 && report->wrappers[index - 1].enter_sequence >=
		    wrapper->enter_sequence) ||
		    wrapper->kind < MANT_MANDOC_WRAPPER_NODE ||
		    wrapper->kind > MANT_MANDOC_WRAPPER_FONT ||
		    wrapper->affinity != 0 || wrapper->flags != 0 ||
		    wrapper->enter_atom > report->atoms_count ||
		    wrapper->leave_atom > report->atoms_count ||
		    wrapper->enter_atom > wrapper->leave_atom ||
		    wrapper->state_before > MANT_MANDOC_FONT_BOLD_UNDERLINE ||
		    wrapper->state_after > MANT_MANDOC_FONT_BOLD_UNDERLINE)
			goto invalid_wrapper;
		while (active_wrapper != MANT_MANDOC_EXEC_NONE &&
		    report->wrappers[active_wrapper].leave_sequence <=
		    wrapper->enter_sequence)
			active_wrapper = report->wrappers[active_wrapper].parent;
		if (wrapper->parent != active_wrapper)
			goto invalid_wrapper;
		if (wrapper->parent != MANT_MANDOC_EXEC_NONE) {
			if (wrapper->parent >= index)
				goto invalid_wrapper;
			if (report->wrappers[wrapper->parent].enter_sequence >=
			    wrapper->enter_sequence ||
			    report->wrappers[wrapper->parent].leave_sequence <=
			    wrapper->leave_sequence)
				goto invalid_wrapper;
			last_sibling_leave = &last_wrapper_child_leave[wrapper->parent];
		} else
			last_sibling_leave = &last_root_wrapper_leave;
		if (*last_sibling_leave != 0 &&
		    wrapper->enter_sequence <= *last_sibling_leave)
			goto invalid_wrapper;
		*last_sibling_leave = wrapper->leave_sequence;
		if (wrapper->kind == MANT_MANDOC_WRAPPER_NODE) {
			if (wrapper->node >= report->nodes_count ||
			    wrapper->target_start != MANT_MANDOC_EXEC_NONE ||
			    wrapper->target_length != 0 ||
			    wrapper->state_before != 0 || wrapper->state_after != 0 ||
			    wrapper->depth_before != 0 || wrapper->depth_after != 0 ||
			    wrapper->enter_sequence >= wrapper->leave_sequence)
				goto invalid_wrapper;
		} else if (wrapper->target_start != MANT_MANDOC_EXEC_NONE ||
		    wrapper->target_length != 0 ||
		    wrapper->enter_atom != wrapper->leave_atom ||
		    wrapper->enter_sequence != wrapper->leave_sequence ||
		    wrapper->depth_after > wrapper->depth_before +
		    (wrapper->depth_before != UINT32_MAX))
			goto invalid_wrapper;
		if (wrapper->kind == MANT_MANDOC_WRAPPER_NODE)
			active_wrapper = wrapper->key;
		continue;
invalid_wrapper:
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
		    "native execution wrapper relationship is inconsistent");
		goto fail;
	}

#define ADVANCE_NODE_WRAPPERS(sequence) do { \
	while (wrapper_cursor < report->wrappers_count && \
	    report->wrappers[wrapper_cursor].enter_sequence < (sequence)) { \
		wrapper = &report->wrappers[wrapper_cursor++]; \
		while (active_wrapper != MANT_MANDOC_EXEC_NONE && \
		    report->wrappers[active_wrapper].leave_sequence <= \
		    wrapper->enter_sequence) \
			active_wrapper = report->wrappers[active_wrapper].parent; \
		if (wrapper->kind == MANT_MANDOC_WRAPPER_NODE) \
			active_wrapper = wrapper->key; \
	} \
	while (active_wrapper != MANT_MANDOC_EXEC_NONE && \
	    report->wrappers[active_wrapper].leave_sequence <= (sequence)) \
		active_wrapper = report->wrappers[active_wrapper].parent; \
} while (0)
	wrapper_cursor = 0;
	active_wrapper = MANT_MANDOC_EXEC_NONE;
	for (index = 0; index < report->boundaries_count; index++) {
		boundary = &report->boundaries[index];
		ADVANCE_NODE_WRAPPERS(boundary->enter_sequence);
		if (boundary->wrapper != active_wrapper)
			goto invalid_boundary;
	}
	wrapper_cursor = 0;
	active_wrapper = MANT_MANDOC_EXEC_NONE;
	for (index = 0; index < report->controls_count; index++) {
		control = &report->controls[index];
		ADVANCE_NODE_WRAPPERS(control->enter_sequence);
		if (control->wrapper != active_wrapper)
			goto invalid_control;
	}
#undef ADVANCE_NODE_WRAPPERS
	for (index = 0; index < report->references_count; index++) {
		uint64_t *last_sibling_leave;

		semantic_reference = &report->references[index];
		if (semantic_reference->key != index ||
		    (index != 0 && report->references[index - 1].enter_sequence >=
		    semantic_reference->enter_sequence) ||
		    (semantic_reference->parent != MANT_MANDOC_EXEC_NONE &&
		    semantic_reference->parent >= index) ||
		    semantic_reference->owner_node >= report->nodes_count ||
		    semantic_reference->target_node >= report->nodes_count ||
		    semantic_reference->kind < MANT_MANDOC_REFERENCE_EXTERNAL_URI ||
		    semantic_reference->kind > MANT_MANDOC_REFERENCE_SECTION ||
		    semantic_reference->primary_start == MANT_MANDOC_EXEC_NONE ||
		    semantic_reference->primary_start > report->pool_length ||
		    semantic_reference->primary_length > report->pool_length -
		    semantic_reference->primary_start ||
		    (semantic_reference->secondary_start == MANT_MANDOC_EXEC_NONE ?
		    semantic_reference->secondary_length != 0 :
		    semantic_reference->secondary_start > report->pool_length ||
		    semantic_reference->secondary_length > report->pool_length -
		    semantic_reference->secondary_start) ||
		    semantic_reference->enter_atom >
		    semantic_reference->label_start_atom ||
		    semantic_reference->label_start_atom >
		    semantic_reference->leave_atom ||
		    semantic_reference->leave_atom > report->atoms_count ||
		    semantic_reference->affinity != MANT_MANDOC_AFFINITY_INLINE ||
		    semantic_reference->flags != 0 ||
		    semantic_reference->leave_sequence <=
		    semantic_reference->enter_sequence)
			goto invalid_reference;
		if (semantic_reference->kind != MANT_MANDOC_REFERENCE_MANUAL &&
		    semantic_reference->secondary_start != MANT_MANDOC_EXEC_NONE)
			goto invalid_reference;
		if ((semantic_reference->enter_atom != 0 &&
		    report->atoms[semantic_reference->enter_atom - 1].sequence >=
		    semantic_reference->enter_sequence) ||
		    (semantic_reference->enter_atom <
		    semantic_reference->leave_atom &&
		    report->atoms[semantic_reference->enter_atom].sequence <=
		    semantic_reference->enter_sequence) ||
		    (semantic_reference->enter_atom ==
		    semantic_reference->leave_atom &&
		    semantic_reference->enter_atom < report->atoms_count &&
		    report->atoms[semantic_reference->enter_atom].sequence <=
		    semantic_reference->leave_sequence) ||
		    (semantic_reference->enter_atom <
		    semantic_reference->leave_atom &&
		    report->atoms[semantic_reference->leave_atom - 1].sequence >=
		    semantic_reference->leave_sequence) ||
		    (semantic_reference->leave_atom < report->atoms_count &&
		    report->atoms[semantic_reference->leave_atom].sequence <=
		    semantic_reference->leave_sequence))
			goto invalid_reference;
		for (inner = semantic_reference->enter_atom;
		    inner < semantic_reference->label_start_atom; inner++)
			if (report->atoms[inner].role !=
			    MANT_MANDOC_ATOM_IMPLICIT_SPACE)
				goto invalid_reference;
		if (semantic_reference->label_start_atom <
		    semantic_reference->leave_atom &&
		    report->atoms[semantic_reference->label_start_atom].role ==
		    MANT_MANDOC_ATOM_IMPLICIT_SPACE)
			goto invalid_reference;
		if (semantic_reference->parent == MANT_MANDOC_EXEC_NONE)
			last_sibling_leave = &last_root_reference_leave;
		else {
			struct mant_mandoc_reference_record *parent;

			parent = &report->references[semantic_reference->parent];
			if (parent->enter_sequence >=
			    semantic_reference->enter_sequence ||
			    parent->leave_sequence <=
			    semantic_reference->leave_sequence ||
			    parent->enter_atom > semantic_reference->enter_atom ||
			    parent->leave_atom < semantic_reference->leave_atom)
				goto invalid_reference;
			last_sibling_leave = &last_reference_child_leave[
			    semantic_reference->parent];
		}
		if (*last_sibling_leave != 0 && semantic_reference->enter_sequence <=
		    *last_sibling_leave)
			goto invalid_reference;
		*last_sibling_leave = semantic_reference->leave_sequence;
		inner = semantic_reference->target_node;
		while (inner != semantic_reference->owner_node &&
		    report->nodes[inner].parent != MANT_MANDOC_EXEC_NONE)
			inner = report->nodes[inner].parent;
		if (inner != semantic_reference->owner_node)
			goto invalid_reference;
		for (inner = semantic_reference->enter_atom;
		    inner < semantic_reference->leave_atom; inner++)
			if (report->atoms[inner].sequence <=
			    semantic_reference->enter_sequence ||
			    report->atoms[inner].sequence >=
			    semantic_reference->leave_sequence)
				goto invalid_reference;
	}
	for (index = 0; index < report->anchors_count; index++) {
		anchor = &report->anchors[index];
		if (anchor->key != index || anchor->node >= report->nodes_count ||
		    anchor->target_start == MANT_MANDOC_EXEC_NONE ||
		    anchor->target_start > report->pool_length ||
		    anchor->target_length > report->pool_length -
		    anchor->target_start || anchor->device_line == 0 ||
		    anchor->atom_cursor > report->atoms_count ||
		    anchor->fragment_cursor > report->fragments_count ||
		    (anchor->atom_cursor != 0 &&
		    report->atoms[anchor->atom_cursor - 1].sequence >=
		    anchor->sequence) ||
		    (anchor->atom_cursor < report->atoms_count &&
		    report->atoms[anchor->atom_cursor].sequence <=
		    anchor->sequence) ||
		    (anchor->fragment_cursor != 0 &&
		    report->fragments[anchor->fragment_cursor - 1].sequence >=
		    anchor->sequence) ||
		    (anchor->fragment_cursor < report->fragments_count &&
		    report->fragments[anchor->fragment_cursor].sequence <=
		    anchor->sequence) ||
		    anchor->affinity != MANT_MANDOC_AFFINITY_BEFORE_OUTPUT ||
		    anchor->reserved != 0)
			goto invalid_anchor;
	}
	for (index = 0; index < report->diagnostics_count; index++) {
		diagnostic = &report->diagnostics[index];
		if ((diagnostic->node != MANT_MANDOC_EXEC_NONE &&
		    diagnostic->node >= report->nodes_count) ||
		    !valid_pool_range(report, diagnostic->message_start,
		    diagnostic->message_length, 0))
			goto invalid_diagnostic;
	}
	for (index = 0; index < report->fragments_count; index++) {
		fragment = &report->fragments[index];
		if (fragment->role != MANT_MANDOC_FRAGMENT_MARGIN_DECORATION)
			continue;
		for (inner = fragment->atom_ref_start;
		    inner < fragment->atom_ref_start + fragment->atom_ref_length;
		    inner++)
			margin_atoms[report->fragment_atoms[inner].atom] = 1;
	}
	/*
	 * Upstream roff_term_pre_mc() retains an authored margin character
	 * until a later term_newln().  That specific source edge can cross a
	 * node-wrapper subtree; all other formatter words and atoms cannot.
	 */
	for (index = 0; index < report->words_count; index++) {
		word = &report->words[index];
		if (word->wrapper == MANT_MANDOC_EXEC_NONE)
			continue;
		if (word->wrapper >= report->wrappers_count)
			goto invalid_word_wrapper;
		wrapper = &report->wrappers[word->wrapper];
		if (wrapper->kind != MANT_MANDOC_WRAPPER_NODE ||
		    word->enter_sequence <= wrapper->enter_sequence ||
		    word->leave_sequence >= wrapper->leave_sequence ||
		    word->enter_atom < wrapper->enter_atom ||
		    word->leave_atom > wrapper->leave_atom)
			goto invalid_word_wrapper;
		origin = word->node;
		while (origin != MANT_MANDOC_EXEC_NONE && origin != wrapper->node)
			origin = report->nodes[origin].parent;
		if (origin == wrapper->node)
			continue;
		if (word->node == MANT_MANDOC_EXEC_NONE ||
		    !margin_control_nodes[word->node] ||
		    word->enter_atom == word->leave_atom)
			goto invalid_word_wrapper;
		for (inner = word->enter_atom; inner < word->leave_atom; inner++)
			if (!margin_atoms[inner])
				goto invalid_word_wrapper;
	}
	for (index = 0; index < report->atoms_count; index++) {
		atom = &report->atoms[index];
		if (atom->wrapper == MANT_MANDOC_EXEC_NONE)
			continue;
		if (atom->wrapper >= report->wrappers_count)
			goto invalid_atom_wrapper;
		wrapper = &report->wrappers[atom->wrapper];
		if (wrapper->kind != MANT_MANDOC_WRAPPER_NODE ||
		    atom->sequence <= wrapper->enter_sequence ||
		    atom->sequence >= wrapper->leave_sequence ||
		    atom->key < wrapper->enter_atom ||
		    atom->key >= wrapper->leave_atom)
			goto invalid_atom_wrapper;
		origin = atom->node;
		while (origin != MANT_MANDOC_EXEC_NONE && origin != wrapper->node)
			origin = report->nodes[origin].parent;
		if (origin != wrapper->node &&
		    (atom->node == MANT_MANDOC_EXEC_NONE ||
		    !margin_control_nodes[atom->node] || !margin_atoms[index]))
			goto invalid_atom_wrapper;
	}
	for (index = 0; index < report->fragments_count; index++) {
		fragment = &report->fragments[index];
		if (fragment->wrapper == MANT_MANDOC_EXEC_NONE)
			continue;
		if (fragment->wrapper >= report->wrappers_count)
			goto invalid_fragment_wrapper;
		wrapper = &report->wrappers[fragment->wrapper];
		if (wrapper->kind != MANT_MANDOC_WRAPPER_NODE ||
		    fragment->sequence <= wrapper->enter_sequence ||
		    fragment->sequence >= wrapper->leave_sequence)
			goto invalid_fragment_wrapper;
	}
	free(generation_checkpoints);
	free(next_generation);
	free(last_capacity);
	free(last_close_reason);
	free(last_close_sequence);
	free(covered_refs);
	free(referenced_atoms);
	free(replaced_atoms);
	free(word_atoms);
	free(margin_atoms);
	free(margin_control_nodes);
	free(covered_fragments);
	free(covered_glyph_geometry);
	free(fragment_flush_outcome);
	free(last_reference_child_leave);
	free(last_wrapper_child_leave);
	free(last_control_child_leave);
	free(pending_flush);
	free(terminal_flush);
	free(direct_boundary_lines);
	free(subtree_boundary_lines);
	free(live_atoms);
	return 1;

invalid_flush_after_loop:
	fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
	    "native execution flush relationship is inconsistent");
	goto fail;

invalid_origin:
	fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
	    "native execution source or node origin is inconsistent");
	goto fail;
invalid_word:
	fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
	    "native execution formatter word relationship is inconsistent");
	goto fail;
invalid_boundary:
	fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
	    "native execution boundary relationship is inconsistent");
	goto fail;
invalid_control:
	fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
	    "native execution control relationship is inconsistent");
	goto fail;
invalid_word_wrapper:
	fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
	    "native execution word wrapper is inconsistent");
	goto fail;
invalid_atom_wrapper:
	fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
	    "native execution atom wrapper is inconsistent");
	goto fail;
invalid_fragment_wrapper:
	fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
	    "native execution fragment wrapper is inconsistent");
	goto fail;
invalid_reference:
	fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
	    "native execution semantic reference is inconsistent");
	goto fail;
invalid_anchor:
	fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
	    "native execution anchor is inconsistent");
	goto fail;
invalid_diagnostic:
	fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
	    "native execution diagnostic relationship is inconsistent");
	goto fail;
fail:
	free(generation_checkpoints);
	free(next_generation);
	free(last_capacity);
	free(last_close_reason);
	free(last_close_sequence);
	free(covered_refs);
	free(referenced_atoms);
	free(replaced_atoms);
	free(word_atoms);
	free(margin_atoms);
	free(margin_control_nodes);
	free(covered_fragments);
	free(covered_glyph_geometry);
	free(fragment_flush_outcome);
	free(last_reference_child_leave);
	free(last_wrapper_child_leave);
	free(last_control_child_leave);
	free(pending_flush);
	free(terminal_flush);
	free(direct_boundary_lines);
	free(subtree_boundary_lines);
	free(live_atoms);
	return 0;
}

static int
tree_supported(struct mant_mandoc_execution_report *report,
    const struct roff_node *node, uint64_t depth, uint64_t *count)
{
	for (; node != NULL; node = node->next) {
		if (!charge_work(report, 1))
			return 0;
		if (depth >= report->limits.max_depth) {
			fail_report(report, MANT_MANDOC_EXECUTION_BUDGET,
			    "native execution depth limit exceeded");
			return 0;
		}
		if (*count == UINT32_MAX) {
			fail_report(report, MANT_MANDOC_EXECUTION_BUDGET,
			    "native execution node identity limit exceeded");
			return 0;
		}
		if (node->type == ROFFT_EQN) {
			fail_report(report, MANT_MANDOC_EXECUTION_UNSUPPORTED,
			    "native execution does not yet support eqn");
			return 0;
		}
		(*count)++;
		if (!tree_supported(report, node->child, depth + 1, count))
			return 0;
	}
	return 1;
}

static int
collect_nodes(struct mant_mandoc_execution_report *report,
    const struct roff_node *node, uint32_t parent, uint64_t depth)
{
	struct mant_mandoc_node_record *record;
	uint32_t key, macro_start;
	const char *macro_name;

	if (node != NULL && depth >= report->limits.max_depth)
		return 0;
	for (; node != NULL; node = node->next) {
		if (!charge_work(report, 1) || !charge_record(report) ||
		    !reserve_nodes(report, report->nodes_count + 1))
			return 0;
		key = (uint32_t)report->nodes_count;
		report->node_index[key].node = node;
		report->node_index[key].key = key;
		if (node->string != NULL) {
			report->word_index[report->word_count].word = node->string;
			report->word_index[report->word_count].key = key;
			report->word_count++;
		}
		record = &report->nodes[report->nodes_count++];
		memset(record, 0, sizeof(*record));
		record->macro_start = MANT_MANDOC_EXEC_NONE;
		record->key = key;
		record->parent = parent;
		record->source = 0;
		if (node->line < 0 || node->pos < 0) {
			fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
			    "native execution node has a negative source coordinate");
			return 0;
		}
		record->line = (uint32_t)node->line;
		record->column = (uint32_t)node->pos + 1;
		record->kind = stable_node_kind(node->type);
		record->flags = stable_node_flags(node);
		if (record->kind == UINT32_MAX)
			return 0;
		if (node->tok != TOKEN_NONE) {
			macro_name = roff_name[node->tok];
			if (!append_pool(report, macro_name, strlen(macro_name),
			    &macro_start))
				return 0;
			record->macro_start = macro_start;
			record->macro_length = (uint32_t)strlen(macro_name);
		}
		if (!collect_nodes(report, node->child, key, depth + 1))
			return 0;
	}
	return 1;
}

static int
compare_node_index(const void *left, const void *right)
{
	const struct node_index *a = left;
	const struct node_index *b = right;
	uintptr_t ap = (uintptr_t)a->node;
	uintptr_t bp = (uintptr_t)b->node;

	return ap < bp ? -1 : ap > bp;
}

static int
compare_word_index(const void *left, const void *right)
{
	const struct word_index *a = left;
	const struct word_index *b = right;
	uintptr_t ap = (uintptr_t)a->word;
	uintptr_t bp = (uintptr_t)b->word;

	return ap < bp ? -1 : ap > bp;
}

static int
compare_live_atom_location(const void *left, const void *right)
{
	const struct live_atom_location *a = left;
	const struct live_atom_location *b = right;

	if (a->buffer_generation != b->buffer_generation)
		return a->buffer_generation < b->buffer_generation ? -1 : 1;
	if (a->slot != b->slot)
		return a->slot < b->slot ? -1 : 1;
	return a->atom < b->atom ? -1 : a->atom > b->atom;
}

static int
compare_generation_checkpoint(const void *left, const void *right)
{
	const struct generation_checkpoint *a = left;
	const struct generation_checkpoint *b = right;

	return a->sequence < b->sequence ? -1 : a->sequence > b->sequence;
}

static size_t
lower_bound_live_atom(const struct live_atom_location *locations, size_t count,
    uint32_t generation, uint32_t slot)
{
	size_t first, length, half, middle;

	first = 0;
	length = count;
	while (length != 0) {
		half = length / 2;
		middle = first + half;
		if (locations[middle].buffer_generation < generation ||
		    (locations[middle].buffer_generation == generation &&
		    locations[middle].slot < slot)) {
			first = middle + 1;
			length -= half + 1;
		} else
			length = half;
	}
	return first;
}

static uint32_t
lookup_node(const struct mant_mandoc_execution_report *report,
    const struct roff_node *node)
{
	struct node_index needle;
	struct node_index *found;

	if (node == NULL || report->node_index == NULL)
		return MANT_MANDOC_EXEC_NONE;
	needle.node = node;
	needle.key = 0;
	found = bsearch(&needle, report->node_index, report->node_count,
	    sizeof(*report->node_index), compare_node_index);
	return found == NULL ? MANT_MANDOC_EXEC_NONE : found->key;
}

static uint32_t
lookup_word_node(const struct mant_mandoc_execution_report *report,
    const char *word)
{
	struct word_index needle, *found;
	size_t index;

	if (word == NULL || report->word_index == NULL)
		return MANT_MANDOC_EXEC_NONE;
	needle.word = word;
	needle.key = 0;
	found = bsearch(&needle, report->word_index, report->word_count,
	    sizeof(*report->word_index), compare_word_index);
	if (found == NULL)
		return MANT_MANDOC_EXEC_NONE;
	index = (size_t)(found - report->word_index);
	if ((index > 0 && report->word_index[index - 1].word == word) ||
	    (index + 1 < report->word_count &&
	     report->word_index[index + 1].word == word))
		return MANT_MANDOC_EXEC_NONE;
	return found->key;
}

static uint32_t
stable_node_kind(enum roff_type kind)
{
	switch (kind) {
	case ROFFT_ROOT: return 0;
	case ROFFT_BLOCK: return 1;
	case ROFFT_HEAD: return 2;
	case ROFFT_BODY: return 3;
	case ROFFT_TAIL: return 4;
	case ROFFT_ELEM: return 5;
	case ROFFT_TEXT: return 6;
	case ROFFT_COMMENT: return 7;
	case ROFFT_TBL: return 8;
	case ROFFT_EQN: return 9;
	default: return UINT32_MAX;
	}
}

static uint32_t
stable_node_flags(const struct roff_node *node)
{
	uint32_t flags = 0;

	if (node->flags & NODE_NOSRC) flags |= 1U << 0;
	if (node->flags & NODE_EOS) flags |= 1U << 1;
	if (node->flags & NODE_NOPRT) flags |= 1U << 2;
	if (node->flags & NODE_NOFILL) flags |= 1U << 3;
	if (node->flags & NODE_ID) flags |= 1U << 4;
	if (node->flags & NODE_HREF) flags |= 1U << 5;
	if (node->flags & NODE_LINE) flags |= 1U << 6;
	if (node->flags & NODE_DELIMO) flags |= 1U << 7;
	if (node->flags & NODE_DELIMC) flags |= 1U << 8;
	if (node->flags & NODE_SYNPRETTY) flags |= 1U << 9;
	return flags;
}

static uint32_t
stable_term_flags(int flags)
{
	return (uint32_t)flags & ((1U << 23) - 1);
}

static uint32_t
stable_control_request(enum roff_tok tok)
{
	switch (tok) {
	case ROFF_br: return MANT_MANDOC_CONTROL_BREAK;
	case ROFF_ce: return MANT_MANDOC_CONTROL_CENTER;
	case ROFF_fi: return MANT_MANDOC_CONTROL_FILL;
	case ROFF_ft: return MANT_MANDOC_CONTROL_FONT;
	case ROFF_ll: return MANT_MANDOC_CONTROL_LINE_LENGTH;
	case ROFF_mc: return MANT_MANDOC_CONTROL_MARGIN_CHARACTER;
	case ROFF_nf: return MANT_MANDOC_CONTROL_NO_FILL;
	case ROFF_po: return MANT_MANDOC_CONTROL_PAGE_OFFSET;
	case ROFF_rj: return MANT_MANDOC_CONTROL_RIGHT_JUSTIFY;
	case ROFF_sp: return MANT_MANDOC_CONTROL_VERTICAL_SPACE;
	case ROFF_ta: return MANT_MANDOC_CONTROL_TAB_STOPS;
	case ROFF_ti: return MANT_MANDOC_CONTROL_TEMPORARY_INDENT;
	default: return MANT_MANDOC_EXEC_NONE;
	}
}

static const char *
stable_control_name(uint32_t request)
{
	switch (request) {
	case MANT_MANDOC_CONTROL_BREAK: return "br";
	case MANT_MANDOC_CONTROL_CENTER: return "ce";
	case MANT_MANDOC_CONTROL_FILL: return "fi";
	case MANT_MANDOC_CONTROL_FONT: return "ft";
	case MANT_MANDOC_CONTROL_LINE_LENGTH: return "ll";
	case MANT_MANDOC_CONTROL_MARGIN_CHARACTER: return "mc";
	case MANT_MANDOC_CONTROL_NO_FILL: return "nf";
	case MANT_MANDOC_CONTROL_PAGE_OFFSET: return "po";
	case MANT_MANDOC_CONTROL_RIGHT_JUSTIFY: return "rj";
	case MANT_MANDOC_CONTROL_VERTICAL_SPACE: return "sp";
	case MANT_MANDOC_CONTROL_TAB_STOPS: return "ta";
	case MANT_MANDOC_CONTROL_TEMPORARY_INDENT: return "ti";
	default: return NULL;
	}
}

static uint32_t
stable_font(int font)
{
	switch (font) {
	case TERMFONT_NONE: return MANT_MANDOC_FONT_ROMAN;
	case TERMFONT_BOLD: return MANT_MANDOC_FONT_BOLD;
	case TERMFONT_UNDER: return MANT_MANDOC_FONT_UNDERLINE;
	case TERMFONT_BI: return MANT_MANDOC_FONT_BOLD_UNDERLINE;
	default: return UINT32_MAX;
	}
}

static uint32_t
atom_kind(int scalar)
{
	switch (scalar) {
	case ' ': return MANT_MANDOC_ATOM_BREAKABLE_SPACE;
	case ASCII_NBRSP: return MANT_MANDOC_ATOM_NONBREAKABLE_SPACE;
	case ASCII_HYPH: return MANT_MANDOC_ATOM_BREAKABLE_HYPHEN;
	case ASCII_NBRZW: return MANT_MANDOC_ATOM_ZERO_WIDTH;
	case '\t': return MANT_MANDOC_ATOM_TAB;
	case ASCII_TABREF: return MANT_MANDOC_ATOM_TAB_REFERENCE;
	case '\b': return MANT_MANDOC_ATOM_BACKSPACE;
	case '\n': return MANT_MANDOC_ATOM_WORD_END_BREAK;
	case ASCII_BREAK: return MANT_MANDOC_ATOM_BREAK_POINT;
	default: return MANT_MANDOC_ATOM_GLYPH;
	}
}

static int
ensure_buffer(struct mant_mandoc_execution_report *report, size_t key,
    size_t slots)
{
	struct buffer_origin *buffers;
	struct buffer_origin *buffer;
	uint32_t *atoms;
	size_t old_count, capacity, index;

	if (key >= report->limits.max_buffer_cells ||
	    slots > report->limits.max_buffer_cells) {
		fail_report(report, MANT_MANDOC_EXECUTION_BUDGET,
		    "native execution buffer-cell limit exceeded");
		return 0;
	}
	if (key >= report->buffer_count) {
		old_count = report->buffer_count;
		if (key == SIZE_MAX || key + 1 > SIZE_MAX / sizeof(*buffers)) {
			fail_report(report, MANT_MANDOC_EXECUTION_ALLOCATION,
			    "native execution buffer registry overflow");
			return 0;
		}
		buffers = realloc(report->buffers, (key + 1) * sizeof(*buffers));
		if (buffers == NULL) {
			fail_report(report, MANT_MANDOC_EXECUTION_ALLOCATION,
			    "could not allocate native execution buffer registry");
			return 0;
		}
		report->buffers = buffers;
		memset(report->buffers + old_count, 0,
		    (key + 1 - old_count) * sizeof(*buffers));
		for (index = old_count; index <= key; index++)
			report->buffers[index].generation_record =
			    MANT_MANDOC_EXEC_NONE;
		report->buffer_count = key + 1;
	}
	buffer = &report->buffers[key];
	if (slots <= buffer->capacity)
		return note_buffer_extent(report, key, 0);
	capacity = slots;
	if (capacity < slots || capacity > SIZE_MAX / sizeof(*atoms)) {
		fail_report(report, MANT_MANDOC_EXECUTION_ALLOCATION,
		    "native execution buffer provenance overflow");
		return 0;
	}
	atoms = realloc(buffer->atoms, capacity * sizeof(*atoms));
	if (atoms == NULL) {
		fail_report(report, MANT_MANDOC_EXECUTION_ALLOCATION,
		    "could not allocate native execution buffer provenance");
		return 0;
	}
	for (index = buffer->capacity; index < capacity; index++)
		atoms[index] = MANT_MANDOC_EXEC_NONE;
	buffer->atoms = atoms;
	buffer->capacity = capacity;
	return note_buffer_extent(report, key, 0);
}

static int
note_buffer_extent(struct mant_mandoc_execution_report *report, size_t key,
    size_t extent)
{
	struct buffer_origin *buffer;
	struct mant_mandoc_buffer_generation_record *record;

	if (key >= report->buffer_count || extent > report->buffers[key].capacity ||
	    key > UINT32_MAX || extent > UINT32_MAX ||
	    report->buffers[key].capacity > UINT32_MAX) {
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
		    "native execution buffer extent is invalid");
		return 0;
	}
	buffer = &report->buffers[key];
	if (buffer->generation_record == MANT_MANDOC_EXEC_NONE) {
		if (!charge_record(report) || !reserve_buffer_generations(report,
		    report->buffer_generations_count + 1))
			return 0;
		buffer->generation_record =
		    (uint32_t)report->buffer_generations_count;
		record = &report->buffer_generations[
		    report->buffer_generations_count++];
		memset(record, 0, sizeof(*record));
		record->key = buffer->generation_record;
		record->buffer = (uint32_t)key;
		record->generation = buffer->generation;
		record->open_sequence = report->sequence++;
		record->close_sequence = UINT64_MAX;
	} else if (buffer->generation_record >=
	    report->buffer_generations_count) {
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
		    "native execution buffer generation key is invalid");
		return 0;
	}
	record = &report->buffer_generations[buffer->generation_record];
	record->capacity = (uint32_t)buffer->capacity;
	if (record->extent < extent)
		record->extent = (uint32_t)extent;
	return 1;
}

static int
close_buffer_generation(struct mant_mandoc_execution_report *report,
    size_t key, size_t extent, uint32_t reason)
{
	struct buffer_origin *buffer;
	struct mant_mandoc_buffer_generation_record *record;

	if (!note_buffer_extent(report, key, extent))
		return 0;
	buffer = &report->buffers[key];
	if (buffer->generation_record == MANT_MANDOC_EXEC_NONE ||
	    buffer->generation_record >= report->buffer_generations_count)
		return 0;
	record = &report->buffer_generations[buffer->generation_record];
	if (record->close_reason != 0 || record->close_sequence != UINT64_MAX ||
	    (reason != MANT_MANDOC_BUFFER_RESET &&
	    reason != MANT_MANDOC_BUFFER_REPORT_END)) {
		fail_report(report, MANT_MANDOC_EXECUTION_INTERNAL,
		    "native execution buffer generation closed twice");
		return 0;
	}
	record->close_reason = reason;
	record->close_sequence = report->sequence++;
	return 1;
}

static void
fail_report(struct mant_mandoc_execution_report *report, int status,
    const char *message)
{
	size_t length;

	if (report == NULL ||
	    report->status != MANT_MANDOC_EXECUTION_BUILDING)
		return;
	report->status = status;
	length = strlen(message) + 1;
	report->error = malloc(length);
	if (report->error != NULL)
		memcpy(report->error, message, length);
}

uint32_t
mant_mandoc_execution_validation_selftest(void)
{
	struct mant_mandoc_execution_report *report;
	struct mant_mandoc_source_record *source;
	struct mant_mandoc_node_record *node;
	struct mant_mandoc_atom_record *atom;
	struct mant_mandoc_boundary_record *boundary;
	struct mant_mandoc_geometry_record *geometry;
	struct mant_mandoc_wrapper_record *wrapper;
	struct mant_mandoc_execution_diagnostic_record *diagnostic;
	struct mant_mandoc_node_record *node_storage;
	struct roff_node native_node;
	struct tbl_span span;
	struct tbl_cell cell;
	struct tbl_dat data;
	struct termp term;
	struct termp_col columns[2];
	struct mant_mandoc_execution_limits limits;
	size_t node_capacity;
	int64_t converted;
	uint32_t failures = 0;

	memset(&limits, 0, sizeof(limits));
	limits.abi_version = MANT_MANDOC_EXECUTION_LIMITS_VERSION;
	limits.abi_size = sizeof(limits);
	limits.max_nodes = limits.max_depth = 16;
	limits.max_work = limits.max_records = 1024;
	limits.max_pool_bytes = 4096;
	limits.max_buffer_cells = 1024;
	limits.max_report_bytes = 1024 * 1024;
	report = mant_mandoc_execution_alloc("native-selftest.1", &limits,
	    NULL, NULL);
	if (report == NULL)
		return UINT32_MAX;
	if (!reserve_nodes(report, 1)) {
		failures |= 1U << 0;
		goto out;
	}
	report->node_count = report->nodes_count = 1;
	node = &report->nodes[0];
	memset(node, 0, sizeof(*node));
	node->parent = MANT_MANDOC_EXEC_NONE;
	node->macro_start = MANT_MANDOC_EXEC_NONE;
	if (!validate_sealed_report(report))
		failures |= 1U << 0;
	source = &report->sources[0];

	free(report->error);
	report->error = NULL;
	report->status = MANT_MANDOC_EXECUTION_BUILDING;
	node_storage = report->nodes;
	report->nodes = NULL;
	if (seal_report(report))
		failures |= 1U << 13;
	report->nodes = node_storage;

	free(report->error);
	report->error = NULL;
	report->status = MANT_MANDOC_EXECUTION_BUILDING;
	node_capacity = report->nodes_capacity;
	report->nodes_capacity = 0;
	if (validate_sealed_report(report))
		failures |= 1U << 11;
	report->nodes_capacity = node_capacity;

	free(report->error);
	report->error = NULL;
	report->status = MANT_MANDOC_EXECUTION_BUILDING;
	source->path_start = (uint32_t)report->pool_length + 1;
	if (validate_sealed_report(report))
		failures |= 1U << 1;
	source->path_start = 0;

	free(report->error);
	report->error = NULL;
	report->status = MANT_MANDOC_EXECUTION_BUILDING;
	node->kind = 10;
	if (validate_sealed_report(report))
		failures |= 1U << 2;
	node->kind = 0;

	free(report->error);
	report->error = NULL;
	report->status = MANT_MANDOC_EXECUTION_BUILDING;
	if (!reserve_atoms(report, 1)) {
		failures |= 1U << 3;
		goto out;
	}
	report->atoms_count = 1;
	atom = &report->atoms[0];
	memset(atom, 0, sizeof(*atom));
	atom->buffer = atom->generation = atom->buffer_generation =
	    atom->slot = MANT_MANDOC_EXEC_NONE;
	atom->kind = MANT_MANDOC_ATOM_GLYPH;
	atom->role = UINT32_MAX;
	atom->input_scalar = atom->display_scalar = 'x';
	atom->node = 0;
	atom->operand_start = MANT_MANDOC_EXEC_NONE;
	atom->wrapper = atom->replaced_by = MANT_MANDOC_EXEC_NONE;
	atom->disposition = MANT_MANDOC_ATOM_EMITTED;
	atom->sequence = 1;
	if (validate_sealed_report(report))
		failures |= 1U << 3;
	atom->role = MANT_MANDOC_ATOM_AUTHORED;
	atom->source = 1;
	free(report->error);
	report->error = NULL;
	report->status = MANT_MANDOC_EXECUTION_BUILDING;
	if (validate_sealed_report(report))
		failures |= 1U << 4;
	report->atoms_count = 0;

	free(report->error);
	report->error = NULL;
	report->status = MANT_MANDOC_EXECUTION_BUILDING;
	if (!reserve_boundaries(report, 1)) {
		failures |= 1U << 5;
		goto out;
	}
	report->boundaries_count = 1;
	boundary = &report->boundaries[0];
	memset(boundary, 0, sizeof(*boundary));
	boundary->node = boundary->parent = MANT_MANDOC_EXEC_NONE;
	boundary->request = UINT32_MAX;
	if (validate_sealed_report(report))
		failures |= 1U << 5;
	report->boundaries_count = 0;

	free(report->error);
	report->error = NULL;
	report->status = MANT_MANDOC_EXECUTION_BUILDING;
	if (!reserve_geometries(report, 1)) {
		failures |= 1U << 6;
		goto out;
	}
	report->geometries_count = 1;
	geometry = &report->geometries[0];
	memset(geometry, 0, sizeof(*geometry));
	geometry->node = geometry->related = geometry->origin_key =
	    MANT_MANDOC_EXEC_NONE;
	geometry->kind = UINT32_MAX;
	if (validate_sealed_report(report))
		failures |= 1U << 6;
	report->geometries_count = 0;

	free(report->error);
	report->error = NULL;
	report->status = MANT_MANDOC_EXECUTION_BUILDING;
	if (!reserve_diagnostics(report, 1)) {
		failures |= 1U << 7;
		goto out;
	}
	report->diagnostics_count = 1;
	diagnostic = &report->diagnostics[0];
	memset(diagnostic, 0, sizeof(*diagnostic));
	diagnostic->node = MANT_MANDOC_EXEC_NONE;
	diagnostic->message_start = (uint32_t)report->pool_length + 1;
	if (validate_sealed_report(report))
		failures |= 1U << 7;
	report->diagnostics_count = 0;

	free(report->error);
	report->error = NULL;
	report->status = MANT_MANDOC_EXECUTION_BUILDING;
	if (!reserve_wrappers(report, 1)) {
		failures |= 1U << 8;
		goto out;
	}
	report->wrappers_count = 1;
	wrapper = &report->wrappers[0];
	memset(wrapper, 0, sizeof(*wrapper));
	wrapper->parent = MANT_MANDOC_EXEC_NONE;
	wrapper->node = 0;
	wrapper->kind = UINT32_MAX;
	wrapper->target_start = MANT_MANDOC_EXEC_NONE;
	wrapper->enter_sequence = 1;
	wrapper->leave_sequence = 2;
	if (validate_sealed_report(report))
		failures |= 1U << 8;
	report->wrappers_count = 0;

	if (table_layout_kind(TBL_CELL_CENTRE) !=
	    MANT_MANDOC_EXEC_TABLE_LAYOUT_CENTER ||
	    table_layout_kind(TBL_CELL_DHORIZ) !=
	    MANT_MANDOC_EXEC_TABLE_LAYOUT_DOUBLE_RULE ||
	    table_layout_kind(TBL_CELL_MAX) != MANT_MANDOC_EXEC_NONE ||
	    table_data_kind(TBL_DATA_NONE) != MANT_MANDOC_EXEC_TABLE_DATA_NONE ||
	    table_data_kind(TBL_DATA_NDHORIZ) !=
	    MANT_MANDOC_EXEC_TABLE_DATA_ISOLATED_DOUBLE_RULE ||
	    table_data_kind((enum tbl_datt)-1) != MANT_MANDOC_EXEC_NONE)
		failures |= 1U << 9;

	free(report->error);
	report->error = NULL;
	report->status = MANT_MANDOC_EXECUTION_BUILDING;
	memset(&native_node, 0, sizeof(native_node));
	free(report->node_index);
	report->node_index = calloc(1, sizeof(*report->node_index));
	if (report->node_index == NULL) {
		failures |= 1U << 10;
		goto out;
	}
	report->node_index[0].node = &native_node;
	report->node_index[0].key = 0;
	memset(&span, 0, sizeof(span));
	memset(&cell, 0, sizeof(cell));
	memset(&data, 0, sizeof(data));
	memset(&term, 0, sizeof(term));
	memset(columns, 0, sizeof(columns));
	cell.pos = TBL_CELL_LEFT;
	data.layout = &cell;
	data.pos = TBL_DATA_DATA;
	term.tcols = columns;
	term.tcol = columns + 1;
	term.maxtcol = 2;
	report->current_table_row = 0;
	if (!execution_table_cell_begin(report, &term, &native_node, &span,
	    &cell, &data, 0, 0, 0, 0))
		failures |= 1U << 10;
	term.tcol = columns;
	if (execution_table_cell_end(report, &term, &native_node, &span,
	    &cell, &data, 0, 0, 0, 0))
		failures |= 1U << 10;
	term.tcol = columns + 1;
	if (execution_table_cell_end(report, &term, &native_node, &span,
	    &cell, &data, 1, 0, 0, 0) ||
	    !execution_table_cell_end(report, &term, &native_node, &span,
	    &cell, &data, 0, 0, 0, 0))
		failures |= 1U << 10;

	free(report->error);
	report->error = NULL;
	report->status = MANT_MANDOC_EXECUTION_BUILDING;
	if (SIZE_MAX > (size_t)INT64_MAX &&
	    size_to_report_i64(report, SIZE_MAX, &converted))
		failures |= 1U << 12;

out:
	mant_mandoc_execution_free(report);
	return failures;
}

static int
copy_records(const struct mant_mandoc_execution_report *report,
    const void *records, size_t record_count, size_t record_capacity,
    size_t record_size,
    size_t record_align, size_t start, void *destination,
    size_t destination_bytes, size_t element_size, size_t count,
    size_t *written)
{
	size_t bytes;

	if (written != NULL)
		*written = 0;
	if (report == NULL ||
	    report->status != MANT_MANDOC_EXECUTION_COMPLETE ||
	    record_count > record_capacity ||
	    (record_count != 0 && records == NULL) ||
	    written == NULL || element_size != record_size ||
	    start > record_count || count > record_count - start ||
	    count > SIZE_MAX / record_size)
		return 0;
	bytes = count * record_size;
	if (destination_bytes != bytes ||
	    (count != 0 && (destination == NULL ||
	    (uintptr_t)destination % record_align != 0)))
		return 0;
	if (count != 0)
		memcpy(destination, (const unsigned char *)records +
		    start * record_size, bytes);
	*written = count;
	return 1;
}

#define DEFINE_RECORD_API(name, plural, type, offsets) \
size_t \
mant_mandoc_execution_##name##_count( \
    const struct mant_mandoc_execution_report *report) \
{ \
	return report != NULL && \
	    report->status == MANT_MANDOC_EXECUTION_COMPLETE ? \
	    report->plural##_count : 0; \
} \
size_t mant_mandoc_execution_##name##_size(void) { return sizeof(struct type); } \
struct mant_##name##_alignment { char prefix; struct type value; }; \
size_t mant_mandoc_execution_##name##_align(void) \
{ return offsetof(struct mant_##name##_alignment, value); } \
uint32_t mant_mandoc_execution_##name##_field_count(void) \
{ return (uint32_t)(sizeof(offsets) / sizeof((offsets)[0])); } \
size_t mant_mandoc_execution_##name##_offset(uint32_t field) \
{ return field < sizeof(offsets) / sizeof((offsets)[0]) ? \
    (offsets)[field] : (size_t)-1; } \
int \
mant_mandoc_execution_copy_##plural( \
    const struct mant_mandoc_execution_report *report, size_t start, \
    void *destination, size_t destination_bytes, size_t element_size, \
    size_t count, size_t *written) \
{ \
	return copy_records(report, report == NULL ? NULL : report->plural, \
	    report == NULL ? 0 : report->plural##_count, \
	    report == NULL ? 0 : report->plural##_capacity, sizeof(struct type), \
	    offsetof(struct mant_##name##_alignment, value), start, destination, \
	    destination_bytes, element_size, count, written); \
}

#define OFF(type, field) offsetof(struct type, field)

static const size_t source_offsets[] = {
	OFF(mant_mandoc_source_record, key), OFF(mant_mandoc_source_record, parent),
	OFF(mant_mandoc_source_record, include_node), OFF(mant_mandoc_source_record, flags),
	OFF(mant_mandoc_source_record, path_start), OFF(mant_mandoc_source_record, path_length)
};
static const size_t node_offsets[] = {
	OFF(mant_mandoc_node_record, key), OFF(mant_mandoc_node_record, parent),
	OFF(mant_mandoc_node_record, source), OFF(mant_mandoc_node_record, line),
	OFF(mant_mandoc_node_record, column), OFF(mant_mandoc_node_record, kind),
	OFF(mant_mandoc_node_record, flags), OFF(mant_mandoc_node_record, macro_start),
	OFF(mant_mandoc_node_record, macro_length)
};
static const size_t buffer_generation_offsets[] = {
	OFF(mant_mandoc_buffer_generation_record, key),
	OFF(mant_mandoc_buffer_generation_record, buffer),
	OFF(mant_mandoc_buffer_generation_record, generation),
	OFF(mant_mandoc_buffer_generation_record, capacity),
	OFF(mant_mandoc_buffer_generation_record, extent),
	OFF(mant_mandoc_buffer_generation_record, close_reason),
	OFF(mant_mandoc_buffer_generation_record, reserved),
	OFF(mant_mandoc_buffer_generation_record, open_sequence),
	OFF(mant_mandoc_buffer_generation_record, close_sequence)
};
static const size_t word_offsets[] = {
	OFF(mant_mandoc_word_record, key),
	OFF(mant_mandoc_word_record, node),
	OFF(mant_mandoc_word_record, source),
	OFF(mant_mandoc_word_record, operand_start),
	OFF(mant_mandoc_word_record, operand_length),
	OFF(mant_mandoc_word_record, role),
	OFF(mant_mandoc_word_record, wrapper),
	OFF(mant_mandoc_word_record, enter_atom),
	OFF(mant_mandoc_word_record, leave_atom),
	OFF(mant_mandoc_word_record, reserved),
	OFF(mant_mandoc_word_record, enter_sequence),
	OFF(mant_mandoc_word_record, leave_sequence)
};
static const size_t atom_offsets[] = {
	OFF(mant_mandoc_atom_record, key), OFF(mant_mandoc_atom_record, buffer),
	OFF(mant_mandoc_atom_record, generation),
	OFF(mant_mandoc_atom_record, buffer_generation),
	OFF(mant_mandoc_atom_record, slot),
	OFF(mant_mandoc_atom_record, kind), OFF(mant_mandoc_atom_record, role),
	OFF(mant_mandoc_atom_record, input_scalar), OFF(mant_mandoc_atom_record, display_scalar),
	OFF(mant_mandoc_atom_record, width_bu), OFF(mant_mandoc_atom_record, node),
	OFF(mant_mandoc_atom_record, source), OFF(mant_mandoc_atom_record, operand_start),
	OFF(mant_mandoc_atom_record, operand_length), OFF(mant_mandoc_atom_record, font),
	OFF(mant_mandoc_atom_record, wrapper), OFF(mant_mandoc_atom_record, replaced_by),
	OFF(mant_mandoc_atom_record, disposition), OFF(mant_mandoc_atom_record, sequence)
};
static const size_t fragment_offsets[] = {
	OFF(mant_mandoc_fragment_record, key), OFF(mant_mandoc_fragment_record, node),
	OFF(mant_mandoc_fragment_record, buffer),
	OFF(mant_mandoc_fragment_record, generation),
	OFF(mant_mandoc_fragment_record, buffer_generation),
	OFF(mant_mandoc_fragment_record, atom_ref_start), OFF(mant_mandoc_fragment_record, atom_ref_length),
	OFF(mant_mandoc_fragment_record, device_line), OFF(mant_mandoc_fragment_record, role),
	OFF(mant_mandoc_fragment_record, wrapper), OFF(mant_mandoc_fragment_record, reserved),
	OFF(mant_mandoc_fragment_record, start_bu), OFF(mant_mandoc_fragment_record, end_bu),
	OFF(mant_mandoc_fragment_record, sequence)
};
static const size_t fragment_atom_offsets[] = {
	OFF(mant_mandoc_fragment_atom_record, fragment),
	OFF(mant_mandoc_fragment_atom_record, atom)
};
static const size_t flush_offsets[] = {
	OFF(mant_mandoc_flush_record, key),
	OFF(mant_mandoc_flush_record, node), OFF(mant_mandoc_flush_record, buffer),
	OFF(mant_mandoc_flush_record, generation),
	OFF(mant_mandoc_flush_record, buffer_generation),
	OFF(mant_mandoc_flush_record, scan_start),
	OFF(mant_mandoc_flush_record, scan_end), OFF(mant_mandoc_flush_record, accepted_start),
	OFF(mant_mandoc_flush_record, accepted_end), OFF(mant_mandoc_flush_record, consumed_start),
	OFF(mant_mandoc_flush_record, consumed_end),
	OFF(mant_mandoc_flush_record, tail_discarded_start),
	OFF(mant_mandoc_flush_record, tail_discarded_end),
	OFF(mant_mandoc_flush_record, remaining_start),
	OFF(mant_mandoc_flush_record, remaining_end), OFF(mant_mandoc_flush_record, fragment_start),
	OFF(mant_mandoc_flush_record, fragment_length), OFF(mant_mandoc_flush_record, flags_before),
	OFF(mant_mandoc_flush_record, flags_after), OFF(mant_mandoc_flush_record, boundary),
	OFF(mant_mandoc_flush_record, outcome), OFF(mant_mandoc_flush_record, leading_bu),
	OFF(mant_mandoc_flush_record, content_bu), OFF(mant_mandoc_flush_record, field_bu),
	OFF(mant_mandoc_flush_record, target_bu), OFF(mant_mandoc_flush_record, taboff_before),
	OFF(mant_mandoc_flush_record, taboff_after), OFF(mant_mandoc_flush_record, visual_before),
	OFF(mant_mandoc_flush_record, visual_after),
	OFF(mant_mandoc_flush_record, sequence),
	OFF(mant_mandoc_flush_record, outcome_sequence)
};
static const size_t boundary_offsets[] = {
	OFF(mant_mandoc_boundary_record, key), OFF(mant_mandoc_boundary_record, node),
	OFF(mant_mandoc_boundary_record, parent), OFF(mant_mandoc_boundary_record, request),
	OFF(mant_mandoc_boundary_record, effect), OFF(mant_mandoc_boundary_record, flags_before),
	OFF(mant_mandoc_boundary_record, flags_after), OFF(mant_mandoc_boundary_record, control),
	OFF(mant_mandoc_boundary_record, line_before), OFF(mant_mandoc_boundary_record, line_after),
	OFF(mant_mandoc_boundary_record, visual_before), OFF(mant_mandoc_boundary_record, visual_after),
	OFF(mant_mandoc_boundary_record, direct_device_lines),
	OFF(mant_mandoc_boundary_record, wrapper),
	OFF(mant_mandoc_boundary_record, enter_sequence),
	OFF(mant_mandoc_boundary_record, leave_sequence)
};
static const size_t control_offsets[] = {
	OFF(mant_mandoc_control_record, key),
	OFF(mant_mandoc_control_record, node),
	OFF(mant_mandoc_control_record, parent),
	OFF(mant_mandoc_control_record, wrapper),
	OFF(mant_mandoc_control_record, request),
	OFF(mant_mandoc_control_record, buffer),
	OFF(mant_mandoc_control_record, generation_before),
	OFF(mant_mandoc_control_record, generation_after),
	OFF(mant_mandoc_control_record, flags_before),
	OFF(mant_mandoc_control_record, flags_after),
	OFF(mant_mandoc_control_record, atom_start),
	OFF(mant_mandoc_control_record, atom_length),
	OFF(mant_mandoc_control_record, fragment_start),
	OFF(mant_mandoc_control_record, fragment_length),
	OFF(mant_mandoc_control_record, flush_start),
	OFF(mant_mandoc_control_record, flush_length),
	OFF(mant_mandoc_control_record, boundary_start),
	OFF(mant_mandoc_control_record, boundary_length),
	OFF(mant_mandoc_control_record, geometry_start),
	OFF(mant_mandoc_control_record, geometry_length),
	OFF(mant_mandoc_control_record, wrapper_start),
	OFF(mant_mandoc_control_record, wrapper_length),
	OFF(mant_mandoc_control_record, reserved),
	OFF(mant_mandoc_control_record, line_before),
	OFF(mant_mandoc_control_record, line_after),
	OFF(mant_mandoc_control_record, visual_before),
	OFF(mant_mandoc_control_record, visual_after),
	OFF(mant_mandoc_control_record, column_before),
	OFF(mant_mandoc_control_record, column_after),
	OFF(mant_mandoc_control_record, extent_before),
	OFF(mant_mandoc_control_record, extent_after),
	OFF(mant_mandoc_control_record, offset_before),
	OFF(mant_mandoc_control_record, offset_after),
	OFF(mant_mandoc_control_record, rmargin_before),
	OFF(mant_mandoc_control_record, rmargin_after),
	OFF(mant_mandoc_control_record, maxrmargin_before),
	OFF(mant_mandoc_control_record, maxrmargin_after),
	OFF(mant_mandoc_control_record, taboff_before),
	OFF(mant_mandoc_control_record, taboff_after),
	OFF(mant_mandoc_control_record, temporary_indent_before),
	OFF(mant_mandoc_control_record, temporary_indent_after),
	OFF(mant_mandoc_control_record, skip_vertical_before),
	OFF(mant_mandoc_control_record, skip_vertical_after),
	OFF(mant_mandoc_control_record, minimum_blank_before),
	OFF(mant_mandoc_control_record, minimum_blank_after),
	OFF(mant_mandoc_control_record, trailing_blank_before),
	OFF(mant_mandoc_control_record, trailing_blank_after),
	OFF(mant_mandoc_control_record, enter_sequence),
	OFF(mant_mandoc_control_record, leave_sequence)
};
static const size_t geometry_offsets[] = {
	OFF(mant_mandoc_geometry_record, key), OFF(mant_mandoc_geometry_record, node),
	OFF(mant_mandoc_geometry_record, related), OFF(mant_mandoc_geometry_record, kind),
	OFF(mant_mandoc_geometry_record, unit), OFF(mant_mandoc_geometry_record, origin_kind),
	OFF(mant_mandoc_geometry_record, origin_key), OFF(mant_mandoc_geometry_record, reserved),
	OFF(mant_mandoc_geometry_record, requested), OFF(mant_mandoc_geometry_record, effective),
	OFF(mant_mandoc_geometry_record, before), OFF(mant_mandoc_geometry_record, after),
	OFF(mant_mandoc_geometry_record, sequence)
};
static const size_t wrapper_offsets[] = {
	OFF(mant_mandoc_wrapper_record, key), OFF(mant_mandoc_wrapper_record, parent),
	OFF(mant_mandoc_wrapper_record, node), OFF(mant_mandoc_wrapper_record, kind),
	OFF(mant_mandoc_wrapper_record, target_start), OFF(mant_mandoc_wrapper_record, target_length),
	OFF(mant_mandoc_wrapper_record, enter_atom), OFF(mant_mandoc_wrapper_record, leave_atom),
	OFF(mant_mandoc_wrapper_record, affinity), OFF(mant_mandoc_wrapper_record, flags),
	OFF(mant_mandoc_wrapper_record, state_before), OFF(mant_mandoc_wrapper_record, state_after),
	OFF(mant_mandoc_wrapper_record, depth_before), OFF(mant_mandoc_wrapper_record, depth_after),
	OFF(mant_mandoc_wrapper_record, enter_sequence), OFF(mant_mandoc_wrapper_record, leave_sequence)
};
static const size_t reference_offsets[] = {
	OFF(mant_mandoc_reference_record, key),
	OFF(mant_mandoc_reference_record, parent),
	OFF(mant_mandoc_reference_record, owner_node),
	OFF(mant_mandoc_reference_record, target_node),
	OFF(mant_mandoc_reference_record, kind),
	OFF(mant_mandoc_reference_record, primary_start),
	OFF(mant_mandoc_reference_record, primary_length),
	OFF(mant_mandoc_reference_record, secondary_start),
	OFF(mant_mandoc_reference_record, secondary_length),
	OFF(mant_mandoc_reference_record, enter_atom),
	OFF(mant_mandoc_reference_record, label_start_atom),
	OFF(mant_mandoc_reference_record, leave_atom),
	OFF(mant_mandoc_reference_record, affinity),
	OFF(mant_mandoc_reference_record, flags),
	OFF(mant_mandoc_reference_record, enter_sequence),
	OFF(mant_mandoc_reference_record, leave_sequence)
};
static const size_t anchor_offsets[] = {
	OFF(mant_mandoc_anchor_record, key), OFF(mant_mandoc_anchor_record, node),
	OFF(mant_mandoc_anchor_record, target_start),
	OFF(mant_mandoc_anchor_record, target_length),
	OFF(mant_mandoc_anchor_record, device_line),
	OFF(mant_mandoc_anchor_record, atom_cursor),
	OFF(mant_mandoc_anchor_record, fragment_cursor),
	OFF(mant_mandoc_anchor_record, affinity),
	OFF(mant_mandoc_anchor_record, reserved),
	OFF(mant_mandoc_anchor_record, sequence)
};
static const size_t table_offsets[] = {
	OFF(mant_mandoc_table_record, key),
	OFF(mant_mandoc_table_record, first_row_node),
	OFF(mant_mandoc_table_record, row_start),
	OFF(mant_mandoc_table_record, row_length),
	OFF(mant_mandoc_table_record, cell_start),
	OFF(mant_mandoc_table_record, cell_length),
	OFF(mant_mandoc_table_record, logical_columns),
	OFF(mant_mandoc_table_record, flags),
	OFF(mant_mandoc_table_record, enter_atom),
	OFF(mant_mandoc_table_record, leave_atom),
	OFF(mant_mandoc_table_record, enter_fragment),
	OFF(mant_mandoc_table_record, leave_fragment),
	OFF(mant_mandoc_table_record, enter_flush),
	OFF(mant_mandoc_table_record, leave_flush),
	OFF(mant_mandoc_table_record, enter_sequence),
	OFF(mant_mandoc_table_record, leave_sequence)
};
static const size_t table_row_offsets[] = {
	OFF(mant_mandoc_table_row_record, key),
	OFF(mant_mandoc_table_row_record, table),
	OFF(mant_mandoc_table_row_record, node),
	OFF(mant_mandoc_table_row_record, ordinal),
	OFF(mant_mandoc_table_row_record, kind),
	OFF(mant_mandoc_table_row_record, logical_columns),
	OFF(mant_mandoc_table_row_record, cell_start),
	OFF(mant_mandoc_table_row_record, cell_length),
	OFF(mant_mandoc_table_row_record, enter_atom),
	OFF(mant_mandoc_table_row_record, leave_atom),
	OFF(mant_mandoc_table_row_record, enter_fragment),
	OFF(mant_mandoc_table_row_record, leave_fragment),
	OFF(mant_mandoc_table_row_record, enter_flush),
	OFF(mant_mandoc_table_row_record, leave_flush),
	OFF(mant_mandoc_table_row_record, enter_sequence),
	OFF(mant_mandoc_table_row_record, leave_sequence)
};
static const size_t table_cell_offsets[] = {
	OFF(mant_mandoc_table_cell_record, key),
	OFF(mant_mandoc_table_cell_record, row),
	OFF(mant_mandoc_table_cell_record, node),
	OFF(mant_mandoc_table_cell_record, ordinal),
	OFF(mant_mandoc_table_cell_record, data_ordinal),
	OFF(mant_mandoc_table_cell_record, logical_column),
	OFF(mant_mandoc_table_cell_record, column_span),
	OFF(mant_mandoc_table_cell_record, row_span),
	OFF(mant_mandoc_table_cell_record, layout_kind),
	OFF(mant_mandoc_table_cell_record, data_kind),
	OFF(mant_mandoc_table_cell_record, alignment),
	OFF(mant_mandoc_table_cell_record, font),
	OFF(mant_mandoc_table_cell_record, flags),
	OFF(mant_mandoc_table_cell_record, buffer),
	OFF(mant_mandoc_table_cell_record, buffer_generation),
	OFF(mant_mandoc_table_cell_record, reserved),
	OFF(mant_mandoc_table_cell_record, enter_atom),
	OFF(mant_mandoc_table_cell_record, leave_atom),
	OFF(mant_mandoc_table_cell_record, offset_bu),
	OFF(mant_mandoc_table_cell_record, rmargin_bu),
	OFF(mant_mandoc_table_cell_record, coloff_before_bu),
	OFF(mant_mandoc_table_cell_record, coloff_after_bu),
	OFF(mant_mandoc_table_cell_record, enter_sequence),
	OFF(mant_mandoc_table_cell_record, leave_sequence)
};
static const size_t diagnostic_offsets[] = {
	OFF(mant_mandoc_execution_diagnostic_record, code),
	OFF(mant_mandoc_execution_diagnostic_record, node),
	OFF(mant_mandoc_execution_diagnostic_record, message_start),
	OFF(mant_mandoc_execution_diagnostic_record, message_length),
	OFF(mant_mandoc_execution_diagnostic_record, sequence)
};

DEFINE_RECORD_API(source, sources, mant_mandoc_source_record, source_offsets)
DEFINE_RECORD_API(node, nodes, mant_mandoc_node_record, node_offsets)
DEFINE_RECORD_API(buffer_generation, buffer_generations,
    mant_mandoc_buffer_generation_record, buffer_generation_offsets)
DEFINE_RECORD_API(word, words, mant_mandoc_word_record, word_offsets)
DEFINE_RECORD_API(atom, atoms, mant_mandoc_atom_record, atom_offsets)
DEFINE_RECORD_API(fragment, fragments, mant_mandoc_fragment_record, fragment_offsets)
DEFINE_RECORD_API(fragment_atom, fragment_atoms, mant_mandoc_fragment_atom_record, fragment_atom_offsets)
DEFINE_RECORD_API(flush, flushes, mant_mandoc_flush_record, flush_offsets)
DEFINE_RECORD_API(boundary, boundaries, mant_mandoc_boundary_record, boundary_offsets)
DEFINE_RECORD_API(control, controls, mant_mandoc_control_record, control_offsets)
DEFINE_RECORD_API(geometry, geometries, mant_mandoc_geometry_record, geometry_offsets)
DEFINE_RECORD_API(wrapper, wrappers, mant_mandoc_wrapper_record, wrapper_offsets)
DEFINE_RECORD_API(reference, references, mant_mandoc_reference_record, reference_offsets)
DEFINE_RECORD_API(anchor, anchors, mant_mandoc_anchor_record, anchor_offsets)
DEFINE_RECORD_API(table, tables, mant_mandoc_table_record, table_offsets)
DEFINE_RECORD_API(table_row, table_rows, mant_mandoc_table_row_record, table_row_offsets)
DEFINE_RECORD_API(table_cell, table_cells, mant_mandoc_table_cell_record, table_cell_offsets)
DEFINE_RECORD_API(diagnostic, diagnostics, mant_mandoc_execution_diagnostic_record, diagnostic_offsets)

#undef OFF
#undef DEFINE_RECORD_API

int
mant_mandoc_execution_copy_pool(
    const struct mant_mandoc_execution_report *report, size_t start,
    void *destination, size_t length, size_t *written)
{
	if (written != NULL)
		*written = 0;
	if (report == NULL ||
	    report->status != MANT_MANDOC_EXECUTION_COMPLETE ||
	    report->pool_length > report->pool_capacity ||
	    (report->pool_length != 0 && report->pool == NULL) ||
	    written == NULL || start > report->pool_length ||
	    length > report->pool_length - start ||
	    (length != 0 && destination == NULL))
		return 0;
	if (length != 0)
		memcpy(destination, report->pool + start, length);
	*written = length;
	return 1;
}
