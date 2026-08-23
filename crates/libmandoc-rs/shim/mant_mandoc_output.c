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
	int		 status;
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
	if (data == NULL || length > output->limit - output->length) {
		output->status = 1;
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
mant_mandoc_output_utf8(int codepoint)
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
	mant_mandoc_output_write(bytes, length);
}

const char *
mant_mandoc_ctype_locale(void)
{
	return setlocale(LC_CTYPE, NULL);
}

void
mant_mandoc_output_end(void)
{
	active_output = NULL;
}

const unsigned char *
mant_mandoc_output_data(const struct mant_mandoc_output *output)
{
	return output == NULL ? NULL : output->data;
}

size_t
mant_mandoc_output_length(const struct mant_mandoc_output *output)
{
	return output == NULL ? 0 : output->length;
}

int
mant_mandoc_output_status(const struct mant_mandoc_output *output)
{
	return output == NULL ? 2 : output->status;
}

void
mant_mandoc_output_free(struct mant_mandoc_output *output)
{
	if (active_output == output)
		active_output = NULL;
	if (output != NULL) {
		free(output->data);
		free(output);
	}
}
