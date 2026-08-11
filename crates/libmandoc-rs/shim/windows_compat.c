/* Small MSVC compatibility surface required by the selected parser sources. */
#include "config.h"

#include <ctype.h>
#include <stddef.h>
#include <string.h>
#include <time.h>

static int parse_digits(const char **, int, int, int *);
static int ascii_prefix_equal(const char *, const char *, size_t);
static int parse_month(const char **, int *);
static int valid_date(int, int, int);

static int
parse_digits(const char **cursor, int minimum, int maximum, int *value)
{
	const char	*input;
	int		 count, parsed;

	input = *cursor;
	count = 0;
	parsed = 0;
	while (count < maximum && isdigit((unsigned char)input[count])) {
		parsed = parsed * 10 + input[count] - '0';
		count++;
	}
	if (count < minimum)
		return 0;
	*cursor = input + count;
	*value = parsed;
	return 1;
}

static int
ascii_prefix_equal(const char *left, const char *right, size_t length)
{
	size_t index;

	for (index = 0; index < length; index++) {
		if (left[index] == '\0' ||
		    tolower((unsigned char)left[index]) !=
		    tolower((unsigned char)right[index]))
			return 0;
	}
	return 1;
}

static int
parse_month(const char **cursor, int *month)
{
	static const char *const names[] = {
		"January", "February", "March", "April", "May", "June",
		"July", "August", "September", "October", "November", "December"
	};
	size_t length;
	int    index;

	for (index = 0; index < 12; index++) {
		length = strlen(names[index]);
		if (ascii_prefix_equal(*cursor, names[index], length)) {
			*cursor += length;
			*month = index + 1;
			return 1;
		}
		if (ascii_prefix_equal(*cursor, names[index], 3)) {
			*cursor += 3;
			*month = index + 1;
			return 1;
		}
	}
	return 0;
}

static int
valid_date(int year, int month, int day)
{
	static const int month_days[] = {
		31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31
	};
	int limit;

	if (year < 1 || month < 1 || month > 12 || day < 1)
		return 0;
	limit = month_days[month - 1];
	if (month == 2 &&
	    (year % 400 == 0 || (year % 4 == 0 && year % 100 != 0)))
		limit++;
	return day <= limit;
}

char *
mant_strptime(const char *input, const char *format, struct tm *result)
{
	const char	*cursor;
	int		 day, month, year;

	cursor = input;
	if (strcmp(format, "%Y-%m-%d") == 0) {
		if (!parse_digits(&cursor, 4, 4, &year) || *cursor++ != '-' ||
		    !parse_digits(&cursor, 1, 2, &month) || *cursor++ != '-' ||
		    !parse_digits(&cursor, 1, 2, &day))
			return NULL;
	} else {
		if (strcmp(format, "$Mdocdate: %b %d %Y $") == 0) {
			if (strncmp(cursor, "$Mdocdate: ", 11) != 0)
				return NULL;
			cursor += 11;
		} else if (strcmp(format, "%b %d, %Y") != 0)
			return NULL;

		if (!parse_month(&cursor, &month) || *cursor++ != ' ' ||
		    !parse_digits(&cursor, 1, 2, &day))
			return NULL;
		if (*format == '$') {
			if (*cursor++ != ' ')
				return NULL;
		} else if (*cursor++ != ',' || *cursor++ != ' ')
			return NULL;
		if (!parse_digits(&cursor, 4, 4, &year))
			return NULL;
		if (*format == '$' &&
		    (*cursor++ != ' ' || *cursor++ != '$'))
			return NULL;
	}

	if (!valid_date(year, month, day))
		return NULL;
	result->tm_year = year - 1900;
	result->tm_mon = month - 1;
	result->tm_mday = day;
	return (char *)cursor;
}
