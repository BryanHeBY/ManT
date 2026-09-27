/*
 * ManT local patch: make libmandoc's parser-global mutable state private to
 * the calling OS thread.  The selected API is synchronous and never hands C
 * state to another thread, so static TLS gives each concurrent Rust parse an
 * independent parser session without a process-wide lock.
 *
 * The Rust crate supports Linux/glibc, macOS, and Windows/MSVC.  C11 TLS is
 * available on the Unix targets; MSVC's static TLS spelling is equivalent for
 * the constant-initialized state used by this parser subset.
 */
#ifndef MANT_THREAD_LOCAL_H
#define MANT_THREAD_LOCAL_H

#if defined(_MSC_VER)
#define MANT_THREAD_LOCAL static __declspec(thread)
#else
#define MANT_THREAD_LOCAL static _Thread_local
#endif

#endif
