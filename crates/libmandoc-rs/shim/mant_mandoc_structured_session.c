/* Parse/render orchestration, TLS ownership, and unified cleanup. */
#include "config.h"
#include "mant_thread_local.h"

#include <stdlib.h>
#include <stdio.h>
#include <string.h>

#include "mandoc.h"
#include "roff.h"
#include "mdoc.h"
#include "tbl.h"
#include "out.h"
#include "mandoc_parse.h"
#include "main.h"
#include "manconf.h"
#include "term.h"

#include "mant_mandoc_structured_source.h"
#include "mant_mandoc_structured_address.h"
#include "mant_mandoc_structured_buffer.h"
#include "mant_mandoc_structured_builder.h"
#include "mant_mandoc_structured_link.h"
#include "mant_mandoc_output.h"

MANT_THREAD_LOCAL struct structured_session *active_session;
MANT_THREAD_LOCAL int structured_active;
MANT_THREAD_LOCAL uint64_t structured_fail_after = UINT64_MAX;
MANT_THREAD_LOCAL uint64_t structured_allocation_count;
MANT_THREAD_LOCAL struct mant_structured_probe_metrics *structured_probe;

void
mant_structured_test_fail_after(uint64_t successful_allocations)
{
	structured_fail_after = successful_allocations;
}

int
mant_structured_injected_allocation_failure(void)
{
	return structured_allocation_count++ >= structured_fail_after;
}

struct structured_session *
mant_structured_active_session(void)
{
	return active_session;
}
void
mant_structured_clear_failure(struct mant_structured_failure_view *failure)
{
	if (failure != NULL)
		memset(failure, 0, sizeof(*failure));
}
static int
check_nesting_depth(struct structured_session *session,
    const struct roff_node *node)
{
	uint64_t depth;

	if (node == NULL)
		return 1;
	depth = 1;
	for (;;) {
		if (depth > session->limits->max_nesting_depth) {
			mant_structured_set_failure(session, MANT_STRUCTURED_BUDGET,
			    MANT_STRUCTURED_STAGE_PARSE, 35, depth,
			    session->limits->max_nesting_depth);
			return 0;
		}
		if (node->child != NULL) {
			node = node->child;
			depth++;
			continue;
		}
		while (node->next == NULL) {
			node = node->parent;
			if (node == NULL)
				return 1;
			depth--;
		}
		node = node->next;
	}
}

static int
validate_limits(const struct mant_structured_limits *limits)
{
	const uint64_t *value;
	size_t count, i;

	if (limits == NULL || limits->reserved != 0)
		return 0;
	value = &limits->max_input_sources;
	count = (offsetof(struct mant_structured_limits, reserved) -
	    offsetof(struct mant_structured_limits, max_input_sources)) /
	    sizeof(uint64_t);
	for (i = 0; i < count; i++)
		if (value[i] == 0)
			return 0;
	return limits->max_input_sources <= UINT32_MAX &&
	    limits->max_sources <= UINT32_MAX &&
	    limits->max_diagnostics <= UINT32_MAX;
}


static int
supported_token(enum roff_tok tok)
{
	switch (tok) {
	case TOKEN_NONE:
	case ROFF_br:
	case ROFF_nf:
	case ROFF_fi:
	case ROFF_ll:
	case MDOC_Dd:
	case MDOC_Dt:
	case MDOC_Os:
	case MDOC_Sh:
	case MDOC_Pp:
	case MDOC_Bl:
	case MDOC_Bd:
	case MDOC_Ed:
	case MDOC_El:
	case MDOC_It:
	case MDOC_Tg:
	case MDOC_Xo:
	case MDOC_Xc:
	case MDOC_Ar:
	case MDOC_Cm:
	case MDOC_Ev:
	case MDOC_Fl:
	case MDOC_Ic:
	case MDOC_Li:
	case MDOC_Nd:
	case MDOC_Nm:
	case MDOC_Pa:
	case MDOC_Xr:
	case MDOC_Em:
	case MDOC_No:
	case MDOC_Ns:
	case MDOC_Pf:
	case MDOC_Sy:
	case MDOC_Sm:
	case MDOC_Sx:
	case MDOC_Lk:
	case MDOC_Mt:
	case MAN_TH:
	case MAN_SH:
	case MAN_LP:
	case MAN_PP:
	case MAN_P:
	case MAN_IP:
	case MAN_TP:
	case MAN_TQ:
	case MAN_RS:
	case MAN_RE:
	case MAN_SM:
	case MAN_SB:
	case MAN_BI:
	case MAN_IB:
	case MAN_BR:
	case MAN_RB:
	case MAN_R:
	case MAN_B:
	case MAN_I:
	case MAN_IR:
	case MAN_RI:
	case MAN_UR:
	case MAN_UE:
	case MAN_MT:
	case MAN_ME:
	case MAN_MR:
		return 1;
	default:
		return 0;
	}
}

static int
supported_tree(const struct roff_node *node)
{
	for (; node != NULL; node = node->next) {
		if ((node->tok == MDOC_Bl && node->norm != NULL &&
		    node->norm->Bl.type == LIST_column) ||
		    !supported_token(node->tok) || !supported_tree(node->child))
			return 0;
	}
	return 1;
}

uint32_t
mant_structured_render(const struct mant_structured_input_view *input,
    const struct mant_structured_limits *limits,
    struct mant_structured_result **out_result,
    struct mant_structured_failure_view *failure)
{
	struct structured_session session;
	struct mant_structured_result *result;
	struct mparse *parser;
	struct roff_meta *meta;
	struct mandoc_msg_state message_state;
	struct manoutput output_options;
	struct mant_mandoc_output *output;
	struct termp *renderer;
	uint64_t source_map_bytes;
	uint32_t status;
	int options, message_state_saved, mchars_ready, output_active;

	if (out_result == NULL || failure == NULL)
		return MANT_STRUCTURED_INVALID_INPUT;
	*out_result = NULL;
	mant_structured_clear_failure(failure);
	if (structured_active) {
		failure->status = MANT_STRUCTURED_REENTRANT;
		failure->stage = MANT_STRUCTURED_STAGE_MARSHAL;
		return failure->status;
	}
	structured_active = 1;
	structured_allocation_count = 0;
	memset(&session, 0, sizeof(session));
	session.input = input;
	session.limits = limits;
	session.probe = structured_probe;
	session.status = MANT_STRUCTURED_OK;
	result = NULL;
	parser = NULL;
	output = NULL;
	renderer = NULL;
	message_state_saved = 0;
	mchars_ready = 0;
	output_active = 0;
	if (!validate_limits(limits)) {
		mant_structured_set_failure(&session, MANT_STRUCTURED_INVALID_INPUT,
		    MANT_STRUCTURED_STAGE_MARSHAL, 0, 0, 0);
		goto cleanup;
	}
	if (!mant_structured_validate_input(&session)) {
		if (session.status == MANT_STRUCTURED_OK)
			mant_structured_set_failure(&session, MANT_STRUCTURED_INVALID_INPUT,
			    MANT_STRUCTURED_STAGE_MARSHAL, 0, 0, 0);
		goto cleanup;
	}
	result = mant_structured_allocate(&session, sizeof(*result), 1,
	    MANT_STRUCTURED_STAGE_MARSHAL);
	if (result == NULL)
		goto cleanup;
	session.result = result;
	source_map_bytes = (uint64_t)input->sources.count *
	    (sizeof(*session.source_keys) + sizeof(*session.source_maps));
	if (!mant_structured_charge(&session, &session.source_map_entries,
	    input->sources.count, limits->max_source_map_entries, 6,
	    MANT_STRUCTURED_STAGE_MARSHAL) ||
	    !mant_structured_charge(&session, &session.source_map_bytes, source_map_bytes,
	    limits->max_source_map_bytes, 7, MANT_STRUCTURED_STAGE_MARSHAL))
		goto cleanup;
	session.source_keys = mant_structured_allocate(&session,
	    (uint64_t)input->sources.count * sizeof(*session.source_keys), 1,
	    MANT_STRUCTURED_STAGE_MARSHAL);
	if (session.source_keys == NULL)
		goto cleanup;
	session.source_maps = mant_structured_allocate(&session,
	    (uint64_t)input->sources.count * sizeof(*session.source_maps), 1,
	    MANT_STRUCTURED_STAGE_MARSHAL);
	if (session.source_maps == NULL)
		goto cleanup;
	options = MPARSE_UTF8 | MPARSE_LATIN1 | MPARSE_VALIDATE |
	    MPARSE_COMMENT | MPARSE_SO;
	if (session.inputs[input->root_input - 1].format == MANT_FORMAT_MAN)
		options |= MPARSE_MAN;
	else
		options |= MPARSE_MDOC;
	/* Structured diagnostics are captured by the bounded observer.  Leaving
	 * the legacy FILE sink disabled prevents an unbounded duplicate stream. */
	mandoc_msg_getstate(&message_state);
	message_state_saved = 1;
	mandoc_msg_setoutfile(NULL);
	mandoc_msg_setmin(MANDOCERR_BASE);
	mandoc_msg_setobserver(mant_structured_observe_diagnostic, &session);
	mandoc_msg_setlineobserver(mant_structured_observe_source_line, &session);
	active_session = &session;
	mchars_alloc();
	mchars_ready = 1;
	parser = mparse_alloc(options, MANDOC_OS_OTHER, NULL);
	if (!mant_structured_read_input(&session, parser, input->root_input))
		goto native_cleanup;
	mandoc_msg_setsourcekey(session.source_keys[input->root_input - 1]);
	meta = mparse_result(parser);
	if (meta == NULL) {
		mant_structured_set_failure(&session, MANT_STRUCTURED_NATIVE,
		    MANT_STRUCTURED_STAGE_PARSE, 0, 0, 0);
		goto native_cleanup;
	}
	if (!check_nesting_depth(&session, meta->first))
		goto native_cleanup;
	if (session.probe == NULL && !supported_tree(meta->first)) {
		mant_structured_set_failure(&session, MANT_STRUCTURED_UNSUPPORTED,
		    MANT_STRUCTURED_STAGE_RENDER, 0, 1, 0);
		goto native_cleanup;
	}
	result->root_source = session.source_keys[input->root_input - 1];
	result->profile = input->profile;
	result->width = input->width;
	if (!mant_structured_copy_metadata(&session, meta))
		goto native_cleanup;
	if (result->metadata.has_body) {
		output = mant_mandoc_output_alloc(
		    limits->max_content_bytes > SIZE_MAX ? SIZE_MAX :
		    (size_t)limits->max_content_bytes);
		if (output == NULL || !mant_mandoc_output_begin(output)) {
			mant_structured_set_failure(&session, MANT_STRUCTURED_BUILDER_ALLOC,
			    MANT_STRUCTURED_STAGE_RENDER, 0, 0,
			    limits->max_builder_allocated_bytes);
			goto native_cleanup;
		}
		output_active = 1;
		memset(&output_options, 0, sizeof(output_options));
		output_options.width = input->width;
		renderer = input->profile == MANT_PROFILE_ASCII ?
		    ascii_alloc(&output_options) : utf8_alloc(&output_options);
		if (renderer == NULL) {
			mant_structured_set_failure(&session, MANT_STRUCTURED_NATIVE,
			    MANT_STRUCTURED_STAGE_RENDER, 0, 0, 0);
			goto native_cleanup;
		}
		term_setcollector(renderer, mant_structured_observe_terminal, &session);
		if (meta->macroset == MACROSET_MDOC)
			terminal_mdoc(renderer, meta);
		else
			terminal_man(renderer, meta);
		term_setcollector(renderer, NULL, NULL);
		ascii_free(renderer);
		renderer = NULL;
		mant_mandoc_output_end();
		output_active = 0;
		if (mant_mandoc_output_status(output) != 0 &&
		    session.status == MANT_STRUCTURED_OK)
			mant_structured_set_failure(&session, MANT_STRUCTURED_BUDGET,
			    MANT_STRUCTURED_STAGE_RENDER, 10,
			    mant_mandoc_output_length(output),
			    limits->max_content_bytes);
		if (session.probe != NULL)
			session.probe->rendered_bytes =
			    mant_mandoc_output_length(output);
		mant_mandoc_output_free(output);
		output = NULL;
		if (session.status == MANT_STRUCTURED_OK) {
			if (!mant_structured_buffer_is_settled(&session))
				mant_structured_set_failure(&session,
				    MANT_STRUCTURED_RELATION,
				    MANT_STRUCTURED_STAGE_RENDER, 0, 0, 0);
			else
				mant_structured_finish_term_roots(&session);
		}
		if (session.status != MANT_STRUCTURED_OK)
			goto native_cleanup;
		mant_structured_address_finish(&session);
		if (session.status != MANT_STRUCTURED_OK)
			goto native_cleanup;
		if (!mant_structured_finalize_links(&session))
			goto native_cleanup;
	}
	result->magic = MANT_STRUCTURED_MAGIC;

native_cleanup:
	if (renderer != NULL) {
		term_setcollector(renderer, NULL, NULL);
		ascii_free(renderer);
	}
	if (output_active)
		mant_mandoc_output_end();
	mant_mandoc_output_free(output);
	if (parser != NULL)
		mparse_free(parser);
	parser = NULL;
	if (mchars_ready)
		mchars_free();
	active_session = NULL;
	if (message_state_saved)
		mandoc_msg_setstate(&message_state);
	if (session.status == MANT_STRUCTURED_OK) {
		if (!mant_structured_check_source_positions(&session) &&
		    session.status == MANT_STRUCTURED_OK)
			mant_structured_set_failure(&session, MANT_STRUCTURED_RELATION,
			    MANT_STRUCTURED_STAGE_CHECK, 0, 0, 0);
		else if (session.status == MANT_STRUCTURED_OK) {
			result->source_maps = session.source_maps;
			result->source_map_count = input->sources.count;
			session.source_maps = NULL;
			if (!mant_structured_result_is_valid(result, &session))
				mant_structured_set_failure(&session, MANT_STRUCTURED_RELATION,
				    MANT_STRUCTURED_STAGE_CHECK, 0, 0, 0);
			else
				result->checked = 1;
		}
	}
	if (session.probe != NULL && session.status == MANT_STRUCTURED_OK)
		mant_structured_set_failure(&session, MANT_STRUCTURED_UNSUPPORTED,
		    MANT_STRUCTURED_STAGE_RENDER, 0, 0, 0);

cleanup:
	mant_structured_buffer_release(&session, result);
	status = session.status;
	if (status == MANT_STRUCTURED_OK) {
		*out_result = result;
		result = NULL;
	} else {
		failure->status = status;
		failure->stage = session.stage;
		failure->limit_kind = session.limit_kind;
		failure->observed = session.observed;
		failure->allowed = session.allowed;
	}
	free(session.source_keys);
	if (session.source_maps != NULL)
		for (uint32_t source = 0; source < input->sources.count; source++)
			free(session.source_maps[source].lines);
	free(session.source_maps);
	free(session.node_stack);
	free(session.node_contexts);
	free(session.owner_root_counts);
	free(session.root_atoms);
	free(session.block_child_counts);
	free(session.list_states);
	free(session.link_identities);
	mant_structured_address_release(&session);
	mant_structured_result_free(result);
	structured_fail_after = UINT64_MAX;
	structured_active = 0;
	return status;
}

uint32_t
mant_structured_probe(const struct mant_structured_input_view *input,
    const struct mant_structured_limits *limits,
    struct mant_structured_probe_metrics *metrics,
    struct mant_structured_failure_view *failure)
{
	struct mant_structured_result *result = NULL;
	uint32_t status;

	if (metrics == NULL || failure == NULL)
		return MANT_STRUCTURED_INVALID_INPUT;
	memset(metrics, 0, sizeof(*metrics));
	mant_structured_clear_failure(failure);
	if (structured_active || structured_probe != NULL) {
		failure->status = MANT_STRUCTURED_REENTRANT;
		failure->stage = MANT_STRUCTURED_STAGE_MARSHAL;
		return failure->status;
	}
	structured_probe = metrics;
	status = mant_structured_render(input, limits, &result, failure);
	structured_probe = NULL;
	mant_structured_result_free(result);
	if (status == MANT_STRUCTURED_OK) {
		failure->status = MANT_STRUCTURED_RELATION;
		failure->stage = MANT_STRUCTURED_STAGE_CHECK;
		return failure->status;
	}
	return status;
}
