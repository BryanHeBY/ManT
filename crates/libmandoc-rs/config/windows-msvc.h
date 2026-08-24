/* Windows/MSVC feature configuration for the memory-only mandoc parser. */
#ifdef __cplusplus
#error "Do not use C++.  See the upstream INSTALL file."
#endif

#define _CRT_SECURE_NO_WARNINGS
#include <errno.h>
#include <stddef.h>
#include <stdarg.h>
#include <sys/types.h>
#include <time.h>
#include "mant_ascii_ctype.h"

#define MAN_CONF_FILE ""
#define MANPATH_BASE ""
#define MANPATH_DEFAULT ""
#define OSENUM MANDOC_OS_OTHER
#define OSNAME "Windows"
#define EFTYPE EINVAL
#define O_DIRECTORY 0
#define PATH_MAX 4096
#define __attribute__(x)

#define strcasecmp _stricmp
#define strncasecmp _strnicmp
#define strdup _strdup
#define strptime mant_strptime

#define HAVE_DIRENT_NAMLEN 0
#define HAVE_ENDIAN 0
#define HAVE_ERR 0
#define HAVE_FTS 0
#define HAVE_FTS_COMPARE_CONST 0
#define HAVE_GETLINE 0
#define HAVE_GETSUBOPT 0
#define HAVE_ISBLANK 1
#define HAVE_LESS_T 0
#define HAVE_MKDTEMP 0
#define HAVE_MKSTEMPS 0
#define HAVE_NTOHL 0
#define HAVE_PLEDGE 0
#define HAVE_PROGNAME 0
#define HAVE_REALLOCARRAY 0
#define HAVE_RECALLOCARRAY 0
#define HAVE_REWB_BSD 0
#define HAVE_REWB_SYSV 0
#define HAVE_SANDBOX_INIT 0
#define HAVE_STRCASESTR 0
#define HAVE_STRINGLIST 0
#define HAVE_STRLCAT 0
#define HAVE_STRLCPY 0
#define HAVE_STRNDUP 0
#define HAVE_STRPTIME 1
#define HAVE_STRSEP 0
#define HAVE_STRTONUM 0
#define HAVE_SYS_ENDIAN 0
#define HAVE_VASPRINTF 0
#define HAVE_WCHAR 0
#define HAVE_OHASH 0
#define NEED_XPG4_2 0

#define BINM_APROPOS "apropos"
#define BINM_CATMAN "catman"
#define BINM_MAKEWHATIS "makewhatis"
#define BINM_MAN "man"
#define BINM_SOELIM "soelim"
#define BINM_WHATIS "whatis"
#define BINM_PAGER "more"

extern void err(int, const char *, ...);
extern void errx(int, const char *, ...);
extern void warn(const char *, ...);
extern void warnx(const char *, ...);
extern const char *getprogname(void);
extern void setprogname(const char *);
extern void *reallocarray(void *, size_t, size_t);
extern void *recallocarray(void *, size_t, size_t, size_t);
extern size_t strlcat(char *, const char *, size_t);
extern size_t strlcpy(char *, const char *, size_t);
extern char *strndup(const char *, size_t);
extern long long strtonum(const char *, long long, long long, const char **);
extern int vasprintf(char **, const char *, va_list);
extern char *mant_strptime(const char *, const char *, struct tm *);
