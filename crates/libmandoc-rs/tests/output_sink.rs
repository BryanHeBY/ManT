#![cfg(feature = "render")]
#![allow(unsafe_code)]

//! The raw and borrowed-sink modes share one bounded, per-call output slot.

use std::ffi::c_void;

use libmandoc_rs::{RenderFormat, Renderer};

unsafe extern "C" {
    fn mant_mandoc_output_alloc(limit: usize) -> *mut c_void;
    fn mant_mandoc_output_alloc_sink(
        limit: usize,
        sink: extern "C" fn(*mut c_void, *const c_void, usize) -> i32,
        arg: *mut c_void,
    ) -> *mut c_void;
    fn mant_mandoc_output_begin(output: *mut c_void) -> i32;
    fn mant_mandoc_output_write(data: *const c_void, length: usize);
    fn mant_mandoc_output_end();
    fn mant_mandoc_output_data(output: *const c_void) -> *const u8;
    fn mant_mandoc_output_length(output: *const c_void) -> usize;
    fn mant_mandoc_output_status(output: *const c_void) -> i32;
    fn mant_mandoc_output_free(output: *mut c_void);
}

extern "C" fn collect(arg: *mut c_void, data: *const c_void, length: usize) -> i32 {
    let collected = unsafe { &mut *(arg.cast::<Vec<u8>>()) };
    if collected.try_reserve(length).is_err() {
        return 0;
    }
    collected.extend_from_slice(unsafe { std::slice::from_raw_parts(data.cast::<u8>(), length) });
    1
}

extern "C" fn reject(_: *mut c_void, _: *const c_void, _: usize) -> i32 {
    0
}

extern "C" fn recursively_write(_: *mut c_void, _: *const c_void, _: usize) -> i32 {
    unsafe { mant_mandoc_output_write(b"xx".as_ptr().cast(), 2) };
    1
}

extern "C" fn prematurely_end(_: *mut c_void, _: *const c_void, _: usize) -> i32 {
    unsafe { mant_mandoc_output_end() };
    1
}

extern "C" fn prematurely_free(arg: *mut c_void, _: *const c_void, _: usize) -> i32 {
    let output = unsafe { *(arg.cast::<*mut c_void>()) };
    unsafe { mant_mandoc_output_free(output) };
    1
}

#[test]
fn borrowed_sink_is_bounded_and_owns_no_duplicate_raw_buffer() {
    // CVS term.c::term_field -> term_ascii.c::utf8_letter emits this body.
    // The exact input was checked with the fixed reference before assertion.
    let rendered = Renderer::new(RenderFormat::Utf8)
        .render_bytes("sink.1", b".TH SINK 1\n.SH BODY\nokay\n")
        .expect("raw renderer still reaches the native sink");
    assert!(rendered.output.contains("     okay"));

    let mut received: Vec<u8> = Vec::new();
    let output =
        unsafe { mant_mandoc_output_alloc_sink(3, collect, (&raw mut received).cast::<c_void>()) };
    assert!(!output.is_null());
    assert_eq!(unsafe { mant_mandoc_output_begin(output) }, 1);
    assert_eq!(unsafe { mant_mandoc_output_begin(output) }, 0);
    unsafe { mant_mandoc_output_write(b"ab".as_ptr().cast(), 2) };
    assert_eq!(received, b"ab");
    assert_eq!(unsafe { mant_mandoc_output_length(output) }, 2);
    assert!(unsafe { mant_mandoc_output_data(output) }.is_null());
    unsafe { mant_mandoc_output_write(b"cd".as_ptr().cast(), 2) };
    assert_eq!(unsafe { mant_mandoc_output_status(output) }, 1);
    assert_eq!(received, b"ab");
    unsafe { mant_mandoc_output_end() };
    unsafe { mant_mandoc_output_free(output) };

    let raw = unsafe { mant_mandoc_output_alloc(3) };
    assert_eq!(unsafe { mant_mandoc_output_begin(raw) }, 1);
    unsafe { mant_mandoc_output_write(b"ab".as_ptr().cast(), 2) };
    assert_eq!(unsafe { mant_mandoc_output_length(raw) }, 2);
    assert!(!unsafe { mant_mandoc_output_data(raw) }.is_null());
    unsafe { mant_mandoc_output_end() };
    unsafe { mant_mandoc_output_free(raw) };
}

#[test]
fn sink_failure_stops_later_writes_and_releases_active_slot() {
    let output = unsafe { mant_mandoc_output_alloc_sink(8, reject, std::ptr::null_mut()) };
    assert!(!output.is_null());
    assert_eq!(unsafe { mant_mandoc_output_begin(output) }, 1);
    unsafe { mant_mandoc_output_write(b"no".as_ptr().cast(), 2) };
    assert_eq!(unsafe { mant_mandoc_output_status(output) }, 3);
    assert_eq!(unsafe { mant_mandoc_output_length(output) }, 0);
    unsafe { mant_mandoc_output_write(b"more".as_ptr().cast(), 4) };
    assert_eq!(unsafe { mant_mandoc_output_length(output) }, 0);
    unsafe { mant_mandoc_output_free(output) };

    let next = unsafe { mant_mandoc_output_alloc(8) };
    assert_eq!(unsafe { mant_mandoc_output_begin(next) }, 1);
    unsafe { mant_mandoc_output_end() };
    unsafe { mant_mandoc_output_free(next) };
}

#[test]
fn callback_reentry_and_premature_end_fail_closed() {
    for callback in [
        recursively_write as extern "C" fn(*mut c_void, *const c_void, usize) -> i32,
        prematurely_end,
    ] {
        let output = unsafe { mant_mandoc_output_alloc_sink(3, callback, std::ptr::null_mut()) };
        assert_eq!(unsafe { mant_mandoc_output_begin(output) }, 1);
        unsafe { mant_mandoc_output_write(b"ab".as_ptr().cast(), 2) };
        assert_eq!(unsafe { mant_mandoc_output_status(output) }, 3);
        assert_eq!(unsafe { mant_mandoc_output_length(output) }, 0);
        unsafe { mant_mandoc_output_write(b"cd".as_ptr().cast(), 2) };
        assert_eq!(unsafe { mant_mandoc_output_length(output) }, 0);
        unsafe { mant_mandoc_output_free(output) };
    }

    let mut output: *mut c_void = std::ptr::null_mut();
    let pending = unsafe {
        mant_mandoc_output_alloc_sink(3, prematurely_free, (&raw mut output).cast::<c_void>())
    };
    output = pending;
    assert_eq!(unsafe { mant_mandoc_output_begin(output) }, 1);
    unsafe { mant_mandoc_output_write(b"ab".as_ptr().cast(), 2) };
    assert_eq!(unsafe { mant_mandoc_output_status(output) }, 3);
    assert_eq!(unsafe { mant_mandoc_output_length(output) }, 0);
    unsafe { mant_mandoc_output_free(output) };
}

#[test]
fn independent_threads_have_independent_active_sinks() {
    let results = [b"alpha".as_slice(), b"bravo".as_slice()]
        .into_iter()
        .map(|input| {
            std::thread::spawn(move || {
                let mut received: Vec<u8> = Vec::new();
                let output = unsafe {
                    mant_mandoc_output_alloc_sink(
                        input.len(),
                        collect,
                        (&raw mut received).cast::<c_void>(),
                    )
                };
                assert_eq!(unsafe { mant_mandoc_output_begin(output) }, 1);
                unsafe { mant_mandoc_output_write(input.as_ptr().cast(), input.len()) };
                assert_eq!(unsafe { mant_mandoc_output_status(output) }, 0);
                unsafe { mant_mandoc_output_free(output) };
                received
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        results
            .into_iter()
            .map(|join| join.join().unwrap())
            .collect::<Vec<_>>(),
        [b"alpha", b"bravo"]
    );
}
