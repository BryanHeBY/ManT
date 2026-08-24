/* Small MSVC compatibility surface required by the selected parser sources. */
#include "config.h"

#include <ctype.h>
#include <errno.h>
#include <stdint.h>
#include <stddef.h>
#include <string.h>
#include <time.h>

static int parse_digits(const char **, int, int, int *);
static int ascii_prefix_equal(const char *, const char *, size_t);
static int parse_month(const char **, int *);
static void skip_space(const char **);
static int64_t days_from_civil(int, unsigned, unsigned);
static void civil_from_days(int64_t, int *, unsigned *, unsigned *);

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

static void
skip_space(const char **cursor)
{
	while (isspace((unsigned char)**cursor))
		(*cursor)++;
}

/* Howard Hinnant's proleptic Gregorian calendar conversion, adjusted to the
 * Unix epoch.  These helpers deliberately accept a day beyond the end of a
 * month because POSIX timegm(3) normalizes the struct tm populated by
 * strptime(3). */
static int64_t
days_from_civil(int year, unsigned month, unsigned day)
{
	int era;
	unsigned year_of_era, day_of_year, day_of_era, month_prime;

	year -= month <= 2;
	era = (year >= 0 ? year : year - 399) / 400;
	year_of_era = (unsigned)(year - era * 400);
	month_prime = month > 2 ? month - 3 : month + 9;
	day_of_year = (153 * month_prime + 2) / 5 + day - 1;
	day_of_era = year_of_era * 365 + year_of_era / 4 -
	    year_of_era / 100 + day_of_year;
	return (int64_t)era * 146097 + day_of_era - 719468;
}

static void
civil_from_days(int64_t days, int *year, unsigned *month, unsigned *day)
{
	int era, adjusted_year;
	unsigned day_of_era, year_of_era, day_of_year, month_prime;

	days += 719468;
	era = (int)(days >= 0 ? days : days - 146096) / 146097;
	day_of_era = (unsigned)(days - (int64_t)era * 146097);
	year_of_era = (day_of_era - day_of_era / 1460 + day_of_era / 36524 -
	    day_of_era / 146096) / 365;
	adjusted_year = (int)year_of_era + era * 400;
	day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 -
	    year_of_era / 100);
	month_prime = (5 * day_of_year + 2) / 153;
	*day = day_of_year - (153 * month_prime + 2) / 5 + 1;
	*month = month_prime < 10 ? month_prime + 3 : month_prime - 9;
	*year = adjusted_year + (*month <= 2);
}

char *
mant_strptime(const char *input, const char *format, struct tm *result)
{
	const char	*cursor;
	int		 day, month, year;

	cursor = input;
	if (strcmp(format, "%Y-%m-%d") == 0) {
		if (!parse_digits(&cursor, 1, 4, &year) || *cursor++ != '-' ||
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

		if (!parse_month(&cursor, &month))
			return NULL;
		skip_space(&cursor);
		if (!parse_digits(&cursor, 1, 2, &day))
			return NULL;
		if (*format == '$') {
			skip_space(&cursor);
		} else {
			if (*cursor++ != ',')
				return NULL;
			skip_space(&cursor);
		}
		if (!parse_digits(&cursor, 1, 4, &year))
			return NULL;
		if (*format == '$') {
			skip_space(&cursor);
			if (*cursor++ != '$')
				return NULL;
		}
	}

	if (month < 1 || month > 12 || day < 1 || day > 31)
		return NULL;
	result->tm_year = year - 1900;
	result->tm_mon = month - 1;
	result->tm_mday = day;
	return (char *)cursor;
}

time_t
mant_timegm(struct tm *value)
{
	int year;
	int64_t days, seconds;

	year = value->tm_year + 1900;
	days = days_from_civil(year, (unsigned)value->tm_mon + 1, 1) +
	    value->tm_mday - 1;
	seconds = days * 86400 + value->tm_hour * 3600 +
	    value->tm_min * 60 + value->tm_sec;
	return (time_t)seconds;
}

int
mant_gmtime_s(struct tm *result, const time_t *value)
{
	int64_t seconds, days, remainder;
	int year;
	unsigned month, day;

	if (result == NULL || value == NULL)
		return EINVAL;
	seconds = (int64_t)*value;
	days = seconds / 86400;
	remainder = seconds % 86400;
	if (remainder < 0) {
		remainder += 86400;
		days--;
	}
	civil_from_days(days, &year, &month, &day);
	memset(result, 0, sizeof(*result));
	result->tm_year = year - 1900;
	result->tm_mon = (int)month - 1;
	result->tm_mday = (int)day;
	result->tm_hour = (int)(remainder / 3600);
	result->tm_min = (int)(remainder % 3600 / 60);
	result->tm_sec = (int)(remainder % 60);
	result->tm_wday = (int)((days + 4) % 7);
	if (result->tm_wday < 0)
		result->tm_wday += 7;
	result->tm_yday = (int)(days - days_from_civil(year, 1, 1));
	return 0;
}
