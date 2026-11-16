/*
 * Keep both allocation and native source-byte inspection bounded. Source
 * labels have their own per-record allowance so renaming a source cannot
 * consume the cumulative diagnostic message budget.
 */
#define MANT_DIAGNOSTIC_MAX_COUNT 4096
#define MANT_DIAGNOSTIC_MAX_FIELD_BYTES 4096
#define MANT_DIAGNOSTIC_MAX_MESSAGE_BYTES (4 * 1024 * 1024)

enum mant_diagnostic_kind {
	MANT_DIAGNOSTIC_ORDINARY = 0,
	MANT_DIAGNOSTIC_INPUT_PROCESSING_LIMIT = 1
};

enum mant_diagnostic_level {
	MANT_DIAGNOSTIC_STYLE = 1,
	MANT_DIAGNOSTIC_WARNING = 2,
	MANT_DIAGNOSTIC_ERROR = 3,
	MANT_DIAGNOSTIC_UNSUPPORTED = 4
};

static unsigned int
diagnostic_level(enum mandoclevel level)
{
	switch (level) {
	case MANDOCLEVEL_STYLE:
		return MANT_DIAGNOSTIC_STYLE;
	case MANDOCLEVEL_WARNING:
		return MANT_DIAGNOSTIC_WARNING;
	case MANDOCLEVEL_UNSUPP:
		return MANT_DIAGNOSTIC_UNSUPPORTED;
	default:
		return MANT_DIAGNOSTIC_ERROR;
	}
}

static size_t
diagnostic_field_length(const char *text)
{
	size_t size = 0;

	if (text != NULL)
		while (size < MANT_DIAGNOSTIC_MAX_FIELD_BYTES && text[size] != '\0')
			size++;
	return size;
}

static size_t
diagnostic_message(char *buffer, const char *base, const char *format,
    va_list *args)
{
	static const char omitted[] = " [truncated]";
	size_t used, remaining, separator;
	int written;

	used = diagnostic_field_length(base);
	if (used != 0)
		memcpy(buffer, base, used);
	if (format == NULL)
		return used;
	remaining = MANT_DIAGNOSTIC_MAX_FIELD_BYTES - used;
	separator = used == 0 ? 0 : 2;
	if (remaining < separator)
		goto truncated;
	if (separator != 0)
		memcpy(buffer + used, ": ", separator);
	remaining -= separator;
	written = vsnprintf(buffer + used + separator, remaining + 1, format, *args);
	if (written < 0)
		return used;
	if ((size_t)written > remaining)
		goto truncated;
	return used + separator + (size_t)written;

truncated:
	memcpy(buffer + MANT_DIAGNOSTIC_MAX_FIELD_BYTES - (sizeof(omitted) - 1),
	    omitted, sizeof(omitted) - 1);
	return MANT_DIAGNOSTIC_MAX_FIELD_BYTES;
}

static int
reserve_diagnostic(struct mant_mandoc_document *document)
{
	struct mant_mandoc_diagnostic *records;
	size_t capacity;

	if (document->diagnostic_count < document->diagnostic_capacity)
		return 1;
	capacity = document->diagnostic_capacity == 0 ? 16 :
	    document->diagnostic_capacity * 2;
	records = realloc(document->diagnostics, capacity * sizeof(*records));
	if (records == NULL) {
		document->diagnostic_error =
		    "could not allocate native diagnostic records";
		return 0;
	}
	document->diagnostics = records;
	document->diagnostic_capacity = capacity;
	return 1;
}

static struct mant_mandoc_diagnostic_view
diagnostic_receipt(enum mandocerr kind, enum mandoclevel level,
    int line, int column)
{
	struct mant_mandoc_diagnostic_view view = {0};

	view.kind = kind == MANDOCERR_ROFFLOOP ?
	    MANT_DIAGNOSTIC_INPUT_PROCESSING_LIMIT : MANT_DIAGNOSTIC_ORDINARY;
	view.level = diagnostic_level(level);
	view.line = line > 0 ? (unsigned int)line : 0;
	view.column = line > 0 ? (column < 0 ? 1 :
	    (unsigned int)column + 1) : 0;
	return view;
}

/*
 * Called from mandoc_msg() after filtering and severity selection. Detail
 * saturation leaves a warning and the first omitted input-limit receipt,
 * independently of the ordinary message budget. The original writer is
 * available outside a session.
 */
int
mant_mandoc_capture_diagnostic(enum mandocerr kind, enum mandoclevel level,
    const char *source, int line, int column, const char *base,
    const char *format, va_list *args)
{
	struct mant_mandoc_document *document = active_document;
	struct mant_mandoc_diagnostic *record;
	unsigned char *storage;
	char message[MANT_DIAGNOSTIC_MAX_FIELD_BYTES + 1];
	size_t source_length, message_length;

	if (document == NULL)
		return 0;
	if (document->diagnostic_error != NULL)
		return 1;
	if (document->diagnostic_omission != NULL)
		goto omitted;
	if (document->diagnostic_count == MANT_DIAGNOSTIC_MAX_COUNT) {
		document->diagnostic_omission =
		    "native diagnostic record count budget reached. Additional details omitted";
		goto omitted;
	}
	message_length = diagnostic_message(message, base, format, args);
	if (message_length > MANT_DIAGNOSTIC_MAX_MESSAGE_BYTES -
	    document->diagnostic_message_bytes) {
		document->diagnostic_omission =
		    "native diagnostic message byte budget reached. Additional details omitted";
		goto omitted;
	}
	if (!reserve_diagnostic(document))
		return 1;
	source_length = diagnostic_field_length(source);
	storage = NULL;
	if (message_length + source_length != 0) {
		storage = malloc(message_length + source_length);
		if (storage == NULL) {
			document->diagnostic_error =
			    "could not allocate native diagnostic text";
			return 1;
		}
	}
	record = &document->diagnostics[document->diagnostic_count++];
	record->storage = storage;
	record->view = diagnostic_receipt(kind, level, line, column);
	record->view.message = message_length == 0 ? NULL : storage;
	record->view.message_length = message_length;
	record->view.source = source_length == 0 ? NULL : storage + message_length;
	record->view.source_length = source_length;
	if (message_length != 0)
		memcpy(storage, message, message_length);
	if (source_length != 0)
		memcpy(storage + message_length, source, source_length);
	document->diagnostic_message_bytes += message_length;
	return 1;

omitted:
	if (kind == MANDOCERR_ROFFLOOP && document->omitted_input_limit.kind ==
	    MANT_DIAGNOSTIC_ORDINARY) {
		document->omitted_input_limit =
		    diagnostic_receipt(kind, level, line, column);
		/* base comes from mandoc_msg.c's static type_message table. */
		document->omitted_input_limit.message =
		    (const unsigned char *)base;
		document->omitted_input_limit.message_length =
		    diagnostic_field_length(base);
	}
	return 1;
}

size_t
mant_mandoc_document_diagnostic_count(const struct mant_mandoc_document *document)
{
	return document == NULL ? 0 : document->diagnostic_count +
	    (document->omitted_input_limit.kind != MANT_DIAGNOSTIC_ORDINARY) +
	    (document->diagnostic_omission != NULL);
}

int
mant_mandoc_document_diagnostic(const struct mant_mandoc_document *document,
    size_t index, struct mant_mandoc_diagnostic_view *view)
{
	size_t retained;

	if (document == NULL || view == NULL)
		return 0;
	retained = document->diagnostic_count;
	if (index < retained) {
		*view = document->diagnostics[index].view;
		return 1;
	}
	if (document->omitted_input_limit.kind != MANT_DIAGNOSTIC_ORDINARY) {
		if (index == retained) {
			*view = document->omitted_input_limit;
			return 1;
		}
		retained++;
	}
	if (document->diagnostic_omission == NULL || index != retained)
		return 0;
	*view = diagnostic_receipt(MANDOCERR_WARNING, MANDOCLEVEL_WARNING, 0, 0);
	view->message = (const unsigned char *)document->diagnostic_omission;
	view->message_length = diagnostic_field_length(document->diagnostic_omission);
	return 1;
}
