#![cfg(all(feature = "render", unix))]
#![allow(unsafe_code)]

use std::{
    ffi::{CStr, CString, c_char, c_int},
    process::{self, Command},
};

use libmandoc_rs::{RenderFormat, Renderer};

#[cfg(target_os = "linux")]
const LC_ALL: c_int = 6;
#[cfg(target_os = "macos")]
const LC_ALL: c_int = 0;

unsafe extern "C" {
    fn setlocale(category: c_int, locale: *const c_char) -> *mut c_char;
}

const SOURCE: &[u8] = b".Dd September 1, 2020\n\
.Dt PROBE 1 I386\n\
.Os ManT\n\
.Sh NAME\n\
.Nm probe\n\
.Nd well-known read-only thing\n";
const LOCALE_UNAVAILABLE_EXIT_CODE: i32 = 77;

#[test]
fn locale_child_render() {
    let Some(locale) = std::env::var_os("LIBMANDOC_RS_LOCALE_CHILD") else {
        return;
    };
    let locale = CString::new(locale.as_encoded_bytes()).expect("locale without NUL");
    let selected = unsafe { setlocale(LC_ALL, locale.as_ptr()) };
    if selected.is_null() {
        process::exit(LOCALE_UNAVAILABLE_EXIT_CODE);
    }

    let report = Renderer::new(RenderFormat::Utf8)
        .render_bytes("locale.1", SOURCE)
        .expect("render under the caller-selected process locale");
    let selected = unsafe { CStr::from_ptr(selected) }.to_string_lossy();
    println!(
        "LIBMANDOC_RS_LOCALE_RESULT={selected}\n{}LIBMANDOC_RS_LOCALE_END",
        report.output
    );
}

#[test]
fn parsing_and_rendering_ignore_the_callers_process_locale() {
    let baseline = run_child("C");
    let alternate = [
        "tr_TR.iso88599",
        "tr_TR.utf8",
        "tr_TR.UTF-8",
        "zh_CN.utf8",
        "zh_CN.UTF-8",
        "en_US.utf8",
        "en_US.UTF-8",
        "UTF-8",
    ]
    .into_iter()
    .find_map(run_child_if_available);

    let Some(alternate) = alternate else {
        eprintln!(
            "no installed non-C locale is available; locale regression coverage is limited to the isolated C baseline"
        );
        return;
    };

    assert_eq!(normalize_selected_locale(&alternate), baseline);
}

fn run_child(locale: &str) -> String {
    run_child_if_available(locale).unwrap_or_else(|| panic!("test locale is unavailable: {locale}"))
}

fn run_child_if_available(locale: &str) -> Option<String> {
    let output = Command::new(std::env::current_exe().expect("current test executable"))
        .args(["--exact", "locale_child_render", "--nocapture"])
        .env("LIBMANDOC_RS_LOCALE_CHILD", locale)
        .output()
        .expect("run isolated locale child");
    if output.status.code() == Some(LOCALE_UNAVAILABLE_EXIT_CODE) {
        return None;
    }
    assert!(
        output.status.success(),
        "locale child for {locale:?} failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).expect("UTF-8 test harness output");
    stdout
        .split_once("LIBMANDOC_RS_LOCALE_RESULT=")
        .and_then(|(_, result)| result.split_once("LIBMANDOC_RS_LOCALE_END"))
        .map(|(result, _)| result.to_owned())
}

fn normalize_selected_locale(output: &str) -> String {
    let (_, rendered) = output
        .split_once('\n')
        .expect("locale child reports the selected locale before its rendering");
    format!("C\n{rendered}")
}
