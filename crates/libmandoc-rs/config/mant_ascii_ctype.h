/* Locale-independent ASCII character classes for roff syntax. */
#ifndef MANT_ASCII_CTYPE_H
#define MANT_ASCII_CTYPE_H

#include <ctype.h>

static inline int
mant_ascii_isalpha(int c)
{
	return (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z');
}

static inline int
mant_ascii_isdigit(int c)
{
	return c >= '0' && c <= '9';
}

static inline int
mant_ascii_isalnum(int c)
{
	return mant_ascii_isalpha(c) || mant_ascii_isdigit(c);
}

static inline int
mant_ascii_isgraph(int c)
{
	return c >= 0x21 && c <= 0x7e;
}

static inline int
mant_ascii_islower(int c)
{
	return c >= 'a' && c <= 'z';
}

static inline int
mant_ascii_isspace(int c)
{
	return c == ' ' || (c >= '\t' && c <= '\r');
}

static inline int
mant_ascii_isupper(int c)
{
	return c >= 'A' && c <= 'Z';
}

static inline int
mant_ascii_tolower(int c)
{
	return mant_ascii_isupper(c) ? c - 'A' + 'a' : c;
}

static inline int
mant_ascii_toupper(int c)
{
	return mant_ascii_islower(c) ? c - 'a' + 'A' : c;
}

#undef isalnum
#undef isalpha
#undef isdigit
#undef isgraph
#undef islower
#undef isspace
#undef isupper
#undef tolower
#undef toupper

#define isalnum(c) mant_ascii_isalnum(c)
#define isalpha(c) mant_ascii_isalpha(c)
#define isdigit(c) mant_ascii_isdigit(c)
#define isgraph(c) mant_ascii_isgraph(c)
#define islower(c) mant_ascii_islower(c)
#define isspace(c) mant_ascii_isspace(c)
#define isupper(c) mant_ascii_isupper(c)
#define tolower(c) mant_ascii_tolower(c)
#define toupper(c) mant_ascii_toupper(c)

#endif
