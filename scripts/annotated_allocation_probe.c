/* Linux/glibc-only A15 diagnostic; not linked into ManT or libmandoc.
 *
 * Count calls through the public allocation ABI of one preloaded process.
 * Requested bytes are successful request sizes (realloc counts its new size),
 * not live heap, actual allocator capacity, Rust-only work, or stage self-cost.
 * Internal libc calls, direct mmap, and allocators that bypass these symbols
 * are outside this counter. Normal library destruction writes one JSON line
 * to the dedicated MANT_A15_ALLOC_FD inherited from the runner; it does not
 * write to the measured program's stdout or stderr. free has no success or
 * requested-byte result, so its two corresponding counters remain zero.
 */
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <limits.h>
#include <pthread.h>
#include <stdatomic.h>
#include <stddef.h>
#include <stdint.h>
#include <unistd.h>

extern char **environ;

_Static_assert(sizeof(size_t) <= sizeof(uint64_t), "size_t exceeds counter width");

/* This tool deliberately targets glibc. These forwarding symbols avoid the
 * dlsym/malloc bootstrap recursion of a generic LD_PRELOAD allocator shim. */
extern void *__libc_malloc(size_t);
extern void *__libc_calloc(size_t, size_t);
extern void *__libc_realloc(void *, size_t);
extern void __libc_free(void *);
extern void *__libc_memalign(size_t, size_t);
extern void *__libc_valloc(size_t);
extern void *__libc_pvalloc(size_t);

enum operation {
    OP_MALLOC, OP_CALLOC, OP_REALLOC, OP_FREE, OP_ALIGNED_ALLOC,
    OP_POSIX_MEMALIGN, OP_MEMALIGN, OP_VALLOC, OP_PVALLOC, OP_REALLOCARRAY,
    OP_COUNT
};

struct counts {
    _Atomic uint64_t calls;
    _Atomic uint64_t successes;
    _Atomic uint64_t requested_bytes;
};

static struct counts counters[OP_COUNT];
static _Atomic uint64_t counter_overflow;
static pthread_once_t resolve_once = PTHREAD_ONCE_INIT;
static _Thread_local int resolving;
static void *(*real_aligned_alloc)(size_t, size_t);
static int (*real_posix_memalign)(void **, size_t, size_t);
static void *(*real_reallocarray)(void *, size_t, size_t);

static void
add(_Atomic uint64_t *counter, uint64_t amount)
{
    uint64_t old = atomic_load_explicit(counter, memory_order_relaxed);
    for (;;) {
        uint64_t next = UINT64_MAX - old < amount ? UINT64_MAX : old + amount;
        if (next == UINT64_MAX && amount > UINT64_MAX - old)
            atomic_store_explicit(&counter_overflow, 1, memory_order_relaxed);
        if (atomic_compare_exchange_weak_explicit(counter, &old, next,
                memory_order_relaxed, memory_order_relaxed))
            return;
    }
}

static void
attempt(enum operation operation)
{
    add(&counters[operation].calls, 1);
}

static void
success(enum operation operation, size_t size)
{
    add(&counters[operation].successes, 1);
    add(&counters[operation].requested_bytes, (uint64_t)size);
}

static void
resolve_public_allocators(void)
{
    /* malloc/calloc/realloc/free already forward directly to glibc. If the
     * loader allocates while resolving these other functions, it is safe. */
    resolving = 1;
    real_aligned_alloc = dlsym(RTLD_NEXT, "aligned_alloc");
    real_posix_memalign = dlsym(RTLD_NEXT, "posix_memalign");
    real_reallocarray = dlsym(RTLD_NEXT, "reallocarray");
    resolving = 0;
}

void *
malloc(size_t size)
{
    attempt(OP_MALLOC);
    void *result = __libc_malloc(size);
    if (result != NULL)
        success(OP_MALLOC, size);
    return result;
}

void *
calloc(size_t count, size_t size)
{
    attempt(OP_CALLOC);
    void *result = __libc_calloc(count, size);
    if (result != NULL)
        success(OP_CALLOC, count * size);
    return result;
}

void *
realloc(void *pointer, size_t size)
{
    attempt(OP_REALLOC);
    void *result = __libc_realloc(pointer, size);
    /* realloc(ptr, 0) may free ptr and return NULL; this counter records
     * requested bytes, not successful deallocations. */
    if (result != NULL)
        success(OP_REALLOC, size);
    return result;
}

void
free(void *pointer)
{
    attempt(OP_FREE); /* includes free(NULL), because it is a public call */
    __libc_free(pointer);
}

void *
aligned_alloc(size_t alignment, size_t size)
{
    attempt(OP_ALIGNED_ALLOC);
    void *result;
    if (resolving) {
        /* Only possible during dlsym bootstrap; no public allocator can be
         * resolved recursively through pthread_once on this same thread. */
        result = __libc_memalign(alignment, size);
    } else {
        int saved_errno = errno;
        pthread_once(&resolve_once, resolve_public_allocators);
        errno = saved_errno;
        result = real_aligned_alloc != NULL
            ? real_aligned_alloc(alignment, size)
            : __libc_memalign(alignment, size);
    }
    if (result != NULL)
        success(OP_ALIGNED_ALLOC, size);
    return result;
}

int
posix_memalign(void **pointer, size_t alignment, size_t size)
{
    attempt(OP_POSIX_MEMALIGN);
    int saved_errno = errno;
    int result;
    if (resolving) {
        if (alignment < sizeof(void *) || (alignment & (alignment - 1)) != 0)
            result = EINVAL;
        else {
            void *allocated = __libc_memalign(alignment, size);
            result = allocated == NULL ? ENOMEM : 0;
            if (result == 0)
                *pointer = allocated;
        }
    } else {
        pthread_once(&resolve_once, resolve_public_allocators);
        errno = saved_errno;
        if (real_posix_memalign == NULL)
            result = ENOSYS;
        else
            result = real_posix_memalign(pointer, alignment, size);
    }
    if (result == 0)
        success(OP_POSIX_MEMALIGN, size);
    errno = saved_errno; /* POSIX returns its error number, not via errno. */
    return result;
}

void *
memalign(size_t alignment, size_t size)
{
    attempt(OP_MEMALIGN);
    void *result = __libc_memalign(alignment, size);
    if (result != NULL)
        success(OP_MEMALIGN, size);
    return result;
}

void *
valloc(size_t size)
{
    attempt(OP_VALLOC);
    void *result = __libc_valloc(size);
    if (result != NULL)
        success(OP_VALLOC, size);
    return result;
}

void *
pvalloc(size_t size)
{
    attempt(OP_PVALLOC);
    void *result = __libc_pvalloc(size);
    if (result != NULL)
        success(OP_PVALLOC, size);
    return result;
}

void *
reallocarray(void *pointer, size_t count, size_t size)
{
    attempt(OP_REALLOCARRAY);
    void *result;
    if (resolving) {
        if (count != 0 && size > SIZE_MAX / count) {
            errno = ENOMEM;
            result = NULL;
        } else
            result = __libc_realloc(pointer, count * size);
    } else {
        int saved_errno = errno;
        pthread_once(&resolve_once, resolve_public_allocators);
        errno = saved_errno;
        if (real_reallocarray == NULL) {
            errno = ENOSYS;
            result = NULL;
        } else
            result = real_reallocarray(pointer, count, size);
    }
    if (result != NULL)
        success(OP_REALLOCARRAY, count * size);
    return result;
}

/* Formatting uses only stack storage, integer arithmetic and write(2), so
 * the report cannot allocate after taking its counter snapshot. */
static char *
append_text(char *cursor, const char *text)
{
    while (*text != '\0')
        *cursor++ = *text++;
    return cursor;
}

static char *
append_number(char *cursor, uint64_t value)
{
    char digits[20];
    size_t count = 0;
    do {
        digits[count++] = (char)('0' + value % 10);
        value /= 10;
    } while (value != 0);
    while (count != 0)
        *cursor++ = digits[--count];
    return cursor;
}

static int
report_fd(void)
{
    static const char prefix[] = "MANT_A15_ALLOC_FD=";
    for (char **entry = environ; entry != NULL && *entry != NULL; entry++) {
        const char *value = *entry;
        size_t index = 0;
        while (prefix[index] != '\0' && value[index] == prefix[index])
            index++;
        if (prefix[index] != '\0')
            continue;
        value += index;
        if (*value < '0' || *value > '9')
            return -1;
        unsigned int fd = 0;
        while (*value >= '0' && *value <= '9') {
            unsigned int digit = (unsigned int)(*value++ - '0');
            if (fd > ((unsigned int)INT_MAX - digit) / 10)
                return -1;
            fd = fd * 10 + digit;
        }
        /* A manual invocation must not point the report at program streams. */
        return *value == '\0' && fd >= 3 ? (int)fd : -1;
    }
    return -1;
}

__attribute__((destructor)) static void
report(void)
{
    static const char *const names[OP_COUNT] = {
        "malloc", "calloc", "realloc", "free", "aligned_alloc",
        "posix_memalign", "memalign", "valloc", "pvalloc", "reallocarray"
    };
    /* Ten fixed names, three at-most-20-digit counters each, and syntax
     * require fewer than 2,048 bytes even when every counter saturates. */
    int fd = report_fd();
    if (fd < 0)
        return;
    char output[2048];
    char *cursor = output;
    cursor = append_text(cursor, "{\"schemaVersion\":1,\"operations\":{");
    for (size_t index = 0; index < OP_COUNT; index++) {
        if (index != 0)
            *cursor++ = ',';
        *cursor++ = '"';
        cursor = append_text(cursor, names[index]);
        cursor = append_text(cursor, "\":{\"calls\":");
        cursor = append_number(cursor, atomic_load_explicit(
            &counters[index].calls, memory_order_relaxed));
        cursor = append_text(cursor, ",\"successes\":");
        cursor = append_number(cursor, atomic_load_explicit(
            &counters[index].successes, memory_order_relaxed));
        cursor = append_text(cursor, ",\"requestedBytes\":");
        cursor = append_number(cursor, atomic_load_explicit(
            &counters[index].requested_bytes, memory_order_relaxed));
        *cursor++ = '}';
    }
    cursor = append_text(cursor, "},\"counterOverflow\":");
    cursor = append_text(cursor, atomic_load_explicit(&counter_overflow,
        memory_order_relaxed) != 0 ? "true" : "false");
    cursor = append_text(cursor, "}\n");
    size_t remaining = (size_t)(cursor - output);
    char *next = output;
    while (remaining != 0) {
        ssize_t written = write(fd, next, remaining);
        if (written > 0) {
            next += written;
            remaining -= (size_t)written;
        } else if (written < 0 && errno == EINTR) {
            continue;
        } else {
            /* The runner rejects a missing or truncated report. Never
             * modify the measured program's normal exit status here. */
            break;
        }
    }
}
