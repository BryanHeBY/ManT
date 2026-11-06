/*
 * Reuse pinned out.c::tblcalc on borrowed owned-AST facts. No parser or
 * terminal is retained or executed; the temporary data chain has exactly
 * the original traversal order. tblcalc uses row identity only for the
 * shared options, so one DATA span can borrow the flattened data chain.
 */
static size_t
mant_table_en(size_t count, void *context)
{
	(void)context;
	return count * 24;
}

static double
mant_table_hspan(const struct roffsu *units)
{
	/* term_ascii.c::ascii_hspan(), before its bounded integer conversion. */
	switch (units->unit) {
	case SCALE_BU: return units->scale;
	case SCALE_CM: return units->scale * 240 / 2.54;
	case SCALE_FS: return units->scale * 65536;
	case SCALE_IN: return units->scale * 240;
	case SCALE_MM: return units->scale * 0.24;
	case SCALE_VS:
	case SCALE_PC: return units->scale * 40;
	case SCALE_PT: return units->scale * 10 / 3;
	case SCALE_EN:
	case SCALE_EM: return units->scale * 24;
	default: return 0;
	}
}

int
mant_mandoc_table_hline_input(const char *argument)
{
	struct roffsu units;
	if (argument == NULL || a2roffsu(argument, &units, SCALE_EM) == NULL)
		return 0;
	/* term_word ESCAPE_HLINE: nonpositive/truncated-zero lengths need
	 * rmargin > offset. WMAX can make that execution-column width zero. */
	return mant_table_hspan(&units) + 0.01 >= 1 ? 2 : 1;
}

int
mant_mandoc_table_hmotion_input(const char *argument, int backbefore)
{
	struct roffsu units;
	if (argument == NULL)
		return 0;
	if (*argument == '|')
		argument++;
	if (a2roffsu(argument, &units, SCALE_EM) == NULL)
		return 0;
	/* Before any write pcol is zero, so an absolute advance is measured
	 * from zero as well. Existing writes already prove occupancy. */
	return mant_table_hspan(&units) + 0.01 >= (backbefore ? 25 : 1);
}

size_t
mant_mandoc_table_line_length(size_t current, size_t previous, const char *argument)
{
	struct roffsu units;
	double basic;
	size_t width;
	int operation = 0;
	/* term_setwidth() and ascii_setwidth(): unlike ordinary motions, an
	 * omitted unit is invalid (SCALE_MAX), and restores the previous width. */
	if (argument == NULL)
		return previous;
	if (*argument == '+' || *argument == '-') {
		for (operation = 1; *argument == '+' || *argument == '-'; argument++)
			if (*argument == '-')
				operation = -operation;
	}
	if (a2roffsu(argument, &units, SCALE_MAX) == NULL)
		return previous;
	basic = mant_table_hspan(&units);
	if (!(basic >= 0))
		return previous;
	width = basic >= 24000 ? 24000 : (size_t)(basic + 0.01);
	if (operation > 0)
		return current + width > 24000 ? 24000 : current + width;
	if (operation < 0)
		return current > width ? current - width : 0;
	return width == 0 ? previous : width;
}

int
mant_mandoc_table_measure(const struct mant_mandoc_table_measure_cell *input,
    size_t count, unsigned int columns, unsigned char decimal,
    unsigned int edges, size_t offset, size_t margin,
    mant_mandoc_measure_text measure, void *context,
    size_t *widths, size_t *spacings, size_t *decimals)
{
	struct tbl_opts opts;
	struct tbl_span span;
	struct rofftbl table;
	struct tbl_dat *data;
	struct tbl_cell *layouts;
	size_t i;

	if (columns == 0 || columns > 1000000 || count > 1000000 ||
	    measure == NULL || widths == NULL || spacings == NULL ||
	    decimals == NULL || (count != 0 && input == NULL))
		return 0;
	for (i = 0; i < count; i++)
		if (input[i].column >= columns || input[i].following_spans >=
		    columns - input[i].column || input[i].kind < TBL_CELL_CENTRE ||
		    input[i].kind >= TBL_CELL_MAX || input[i].kind == TBL_CELL_SPAN)
			return 0;
	data = calloc(count == 0 ? 1 : count, sizeof(*data));
	layouts = calloc(count == 0 ? 1 : count, sizeof(*layouts));
	if (data == NULL || layouts == NULL) {
		free(data);
		free(layouts);
		return 0;
	}
	for (i = 0; i < count; i++) {
		layouts[i].col = (int)input[i].column;
		layouts[i].pos = (enum tbl_cellt)input[i].kind;
		layouts[i].width = input[i].minimum_width;
		layouts[i].spacing = input[i].spacing;
		layouts[i].flags =
		    ((input[i].width_flags & 1U) ? TBL_CELL_WIGN : 0) |
		    ((input[i].width_flags & 2U) ? TBL_CELL_EQUAL : 0) |
		    ((input[i].width_flags & 4U) ? TBL_CELL_WMAX : 0);
		data[i].layout = &layouts[i];
		data[i].string = (char *)input[i].text;
		data[i].hspans = (int)input[i].following_spans;
		data[i].block = input[i].text_block;
		data[i].pos = TBL_DATA_DATA;
		data[i].next = i + 1 == count ? NULL : &data[i + 1];
	}
	memset(&opts, 0, sizeof(opts));
	opts.cols = (int)columns;
	opts.decimal = (char)decimal;
	/* Only their sum participates in WMAX's available-width arithmetic. */
	opts.lvert = edges != 0;
	opts.rvert = edges > 1;
	memset(&span, 0, sizeof(span));
	span.opts = &opts;
	span.pos = TBL_SPAN_DATA;
	span.first = count == 0 ? NULL : data;
	memset(&table, 0, sizeof(table));
	table.slen = measure;
	table.len = mant_table_en;
	table.arg = context;
	tblcalc(&table, &span, offset, margin);
	for (i = 0; i < columns; i++) {
		widths[i] = table.cols[i].width;
		spacings[i] = table.cols[i].spacing;
		decimals[i] = table.cols[i].decimal;
	}
	free(table.cols);
	free(layouts);
	free(data);
	return 1;
}

/* tbl_term.c::tbl_number(): measure the ORIGINAL byte spelling, including
 * digits in escape arguments. Padding is later subject to tbl_fill_string's
 * coloff/offset guard; a positive requested pad alone proves no write. */
size_t
mant_mandoc_table_number_padding(const char *text, unsigned char decimal,
    size_t width, size_t position, mant_mandoc_measure_text measure, void *context)
{
	const char *cp, *lastdigit = NULL, *lastpoint = NULL;
	size_t total, integer = 0, pad = 0;
	char byte[2];
	for (cp = text; *cp != '\0'; cp++) {
		if (cp[0] == '\\' && cp[1] == '&') {
			lastdigit = lastpoint = cp;
			break;
		} else if ((unsigned char)*cp == decimal &&
		    (isdigit((unsigned char)cp[1]) ||
		     (cp > text && isdigit((unsigned char)cp[-1]))))
			lastpoint = cp;
		else if (isdigit((unsigned char)*cp))
			lastdigit = cp;
	}
	total = measure(text, context);
	if (lastdigit != NULL) {
		if (lastpoint == NULL)
			lastpoint = lastdigit + 1;
		byte[1] = '\0';
		for (cp = text; cp < lastpoint; cp++) {
			byte[0] = *cp;
			integer += measure(byte, context);
		}
		if (position > integer && width > total) {
			pad = position - integer;
			if (pad + total > width)
				pad = width - total;
		}
	} else if (width > total)
		pad = (width - total) / 2;
	return pad;
}

size_t mant_mandoc_table_measure_cell_size(void)
{ return sizeof(struct mant_mandoc_table_measure_cell); }
size_t mant_mandoc_table_measure_cell_align(void)
{ return MANT_VIEW_ALIGNOF(struct mant_mandoc_table_measure_cell); }
const size_t *mant_mandoc_table_measure_cell_offsets(size_t *count)
{
	static const size_t offsets[] = {
		offsetof(struct mant_mandoc_table_measure_cell, column),
		offsetof(struct mant_mandoc_table_measure_cell, kind),
		offsetof(struct mant_mandoc_table_measure_cell, minimum_width),
		offsetof(struct mant_mandoc_table_measure_cell, spacing),
		offsetof(struct mant_mandoc_table_measure_cell, width_flags),
		offsetof(struct mant_mandoc_table_measure_cell, text),
		offsetof(struct mant_mandoc_table_measure_cell, following_spans),
		offsetof(struct mant_mandoc_table_measure_cell, text_block),
	};
	*count = sizeof(offsets) / sizeof(offsets[0]);
	return offsets;
}
