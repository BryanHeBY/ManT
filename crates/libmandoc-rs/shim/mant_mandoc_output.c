/* Bounded per-call output capture for the optional upstream renderers. */
#include "config.h"
#include "mant_thread_local.h"

#include <stdint.h>
#include <locale.h>
#include <stdlib.h>
#include <string.h>

#include "mant_mandoc_output.h"

struct mant_mandoc_output {
	unsigned char	*data;
	size_t		 length;
	size_t		 capacity;
	size_t		 limit;
	size_t		 attempted_length;
	mant_mandoc_output_sink sink;
	void		*sink_arg;
	enum mant_mandoc_output_operation operation;
	int		 status;
	int		 in_callback;
};

MANT_THREAD_LOCAL struct mant_mandoc_output *active_output;

struct mant_mandoc_output *
mant_mandoc_output_alloc(size_t limit)
{
	struct mant_mandoc_output *output;

	if (limit == 0)
		return NULL;
	output = calloc(1, sizeof(*output));
	if (output != NULL)
		output->limit = limit;
	return output;
}

struct mant_mandoc_output *
mant_mandoc_output_alloc_sink(size_t limit, mant_mandoc_output_sink sink,
    void *sink_arg)
{
	struct mant_mandoc_output *output;

	if (sink == NULL)
		return NULL;
	output = mant_mandoc_output_alloc(limit);
	if (output != NULL) {
		output->sink = sink;
		output->sink_arg = sink_arg;
	}
	return output;
}

int
mant_mandoc_output_begin(struct mant_mandoc_output *output)
{
	if (output == NULL || active_output != NULL)
		return 0;
	active_output = output;
	return 1;
}

void
mant_mandoc_output_write(const void *data, size_t length)
{
	struct mant_mandoc_output *output;
	unsigned char		*resized;
	size_t			 capacity;

	output = active_output;
	if (output == NULL || output->status != 0 || length == 0)
		return;
	if (output->in_callback) {
		output->status = 3;
		return;
	}
	if (data == NULL) {
		output->status = 3;
		return;
	}
	if (length > output->limit - output->length) {
		output->attempted_length = length > SIZE_MAX - output->length ?
		    SIZE_MAX : output->length + length;
		output->status = 1;
		return;
	}
	if (output->sink != NULL) {
		int accepted;

		output->in_callback = 1;
		accepted = output->sink(output->sink_arg, data, length);
		output->in_callback = 0;
		if (!accepted || output->status != 0) {
			output->status = 3;
			return;
		}
		output->length += length;
		return;
	}
	if (length <= output->capacity - output->length) {
		memcpy(output->data + output->length, data, length);
		output->length += length;
		return;
	}
	capacity = output->capacity == 0 ? 4096 : output->capacity;
	while (capacity - output->length < length) {
		if (capacity >= output->limit / 2) {
			capacity = output->limit;
			break;
		}
		capacity *= 2;
	}
	resized = realloc(output->data, capacity);
	if (resized == NULL) {
		output->status = 2;
		return;
	}
	output->data = resized;
	output->capacity = capacity;
	memcpy(output->data + output->length, data, length);
	output->length += length;
}

void
mant_mandoc_output_write_op(const void *data, size_t length,
    enum mant_mandoc_output_operation operation)
{
	struct mant_mandoc_output *output = active_output;
	enum mant_mandoc_output_operation saved;

	if (output == NULL)
		return;
	if (operation < MANT_OUTPUT_GENERIC || operation > MANT_OUTPUT_ENDLINE) {
		output->status = 3;
		return;
	}
	saved = output->operation;
	output->operation = operation;
	mant_mandoc_output_write(data, length);
	output->operation = saved;
}

enum mant_mandoc_output_operation
mant_mandoc_output_current_operation(void)
{
	return active_output == NULL ? MANT_OUTPUT_GENERIC :
	    active_output->operation;
}

void
mant_mandoc_output_utf8(int codepoint)
{
	mant_mandoc_output_utf8_op(codepoint, MANT_OUTPUT_GENERIC);
}

void
mant_mandoc_output_utf8_op(int codepoint,
    enum mant_mandoc_output_operation operation)
{
	unsigned char bytes[4];
	size_t length;

	if (codepoint < 0 || codepoint > 0x10ffff ||
	    (codepoint >= 0xd800 && codepoint <= 0xdfff))
		codepoint = 0xfffd;
	if (codepoint <= 0x7f) {
		bytes[0] = (unsigned char)codepoint;
		length = 1;
	} else if (codepoint <= 0x7ff) {
		bytes[0] = 0xc0 | (unsigned char)(codepoint >> 6);
		bytes[1] = 0x80 | (unsigned char)(codepoint & 0x3f);
		length = 2;
	} else if (codepoint <= 0xffff) {
		bytes[0] = 0xe0 | (unsigned char)(codepoint >> 12);
		bytes[1] = 0x80 | (unsigned char)((codepoint >> 6) & 0x3f);
		bytes[2] = 0x80 | (unsigned char)(codepoint & 0x3f);
		length = 3;
	} else {
		bytes[0] = 0xf0 | (unsigned char)(codepoint >> 18);
		bytes[1] = 0x80 | (unsigned char)((codepoint >> 12) & 0x3f);
		bytes[2] = 0x80 | (unsigned char)((codepoint >> 6) & 0x3f);
		bytes[3] = 0x80 | (unsigned char)(codepoint & 0x3f);
		length = 4;
	}
	mant_mandoc_output_write_op(bytes, length, operation);
}

const char *
mant_mandoc_ctype_locale(void)
{
	return setlocale(LC_CTYPE, NULL);
}

void
mant_mandoc_output_end(void)
{
	if (active_output != NULL && active_output->in_callback) {
		active_output->status = 3;
		return;
	}
	active_output = NULL;
}

const unsigned char *
mant_mandoc_output_data(const struct mant_mandoc_output *output)
{
	return output == NULL || output->sink != NULL ? NULL : output->data;
}

size_t
mant_mandoc_output_length(const struct mant_mandoc_output *output)
{
	return output == NULL ? 0 : output->length;
}

size_t
mant_mandoc_output_attempted_length(const struct mant_mandoc_output *output)
{
	return output == NULL ? 0 : output->attempted_length;
}

int
mant_mandoc_output_status(const struct mant_mandoc_output *output)
{
	return output == NULL ? 2 : output->status;
}

void
mant_mandoc_output_free(struct mant_mandoc_output *output)
{
	if (output != NULL && output->in_callback) {
		output->status = 3;
		return;
	}
	if (active_output == output)
		active_output = NULL;
	if (output != NULL) {
		free(output->data);
		free(output);
	}
}
