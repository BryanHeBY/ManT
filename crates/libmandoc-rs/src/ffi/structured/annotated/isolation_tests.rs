//! An invalid semantic view must not erase a previously checked device body.

use super::super::STATUS_BUDGET;
use super::{
    FailureView, Handle, InputStorage, MarkView, ResultView, STATUS_OK, STATUS_RELATION,
    mant_annotated_render, mant_annotated_result_check, mant_annotated_result_view, raw_limits,
    transfer,
};
use crate::{InputFormat, SourceBundle, structured::StructuredLimits};
use std::ptr::NonNull;

fn with_native_result(test: impl FnOnce(Handle, ResultView, super::super::raw::Limits)) {
    // This exact input ran on the pinned CVS reference first. man_term.c's
    // TP/B handlers emit the visible name and body; the following mutation
    // tests only the adapter boundary, not an alternative roff interpretation.
    with_native_result_for(
        b".TH T 1\n.SH D\n.TP\n.B --foo\nDescription.\n",
        InputFormat::Man,
        test,
    );
}

fn with_native_result_for(
    source: &[u8],
    format: InputFormat,
    test: impl FnOnce(Handle, ResultView, super::super::raw::Limits),
) {
    let mut bundle = SourceBundle::new();
    bundle.insert("t.1", source.to_vec()).unwrap();
    let limits = raw_limits(&StructuredLimits::default());
    let storage = InputStorage::new_annotated("t.1", &bundle, format, &limits).unwrap();
    let input = storage.view(78, super::super::PROFILE_UTF8);
    let mut pointer = std::ptr::null_mut();
    let mut failure = FailureView::default();
    assert_eq!(
        unsafe {
            mant_annotated_render(
                &raw const input,
                &raw const limits,
                &raw mut pointer,
                &raw mut failure,
            )
        },
        STATUS_OK
    );
    let handle = Handle(NonNull::new(pointer).unwrap());
    assert_eq!(
        unsafe { mant_annotated_result_check(handle.0.as_ptr(), &raw mut failure) },
        STATUS_OK
    );
    let mut view = ResultView::default();
    assert_eq!(
        unsafe { mant_annotated_result_view(handle.0.as_ptr(), &raw mut view) },
        STATUS_OK
    );
    test(handle, view, limits);
}

#[test]
fn ffi_semantic_rejection_keeps_checked_body_but_not_actionable_labels() {
    with_native_result(|handle, view, limits| {
        let baseline = transfer(&handle, &view, &limits, false).unwrap();
        assert!(!baseline.annotation_degraded);
        assert!(baseline.text.contains("Description."));

        // The C-owned handle is unchanged. Only this borrowed ABI descriptor
        // points to a test-owned, same-layout mark copy with an invalid kind.
        // The copy remains alive throughout both checked transfers.
        assert!(view.marks.count > 0);
        let mut bad_marks = unsafe {
            std::slice::from_raw_parts(view.marks.ptr.cast::<MarkView>(), view.marks.count as usize)
        }
        .to_vec();
        let original_marks = bad_marks.clone();
        bad_marks[0].kind = 99;
        let mut bad_marks_view = view;
        bad_marks_view.marks.ptr = bad_marks.as_ptr().cast();
        assert_eq!(
            transfer(&handle, &bad_marks_view, &limits, false)
                .unwrap_err()
                .status,
            STATUS_RELATION
        );
        let body_only = transfer(&handle, &bad_marks_view, &limits, true).unwrap();
        assert!(body_only.annotation_degraded);
        assert_eq!(body_only.text, baseline.text);
        assert_eq!(body_only.rows, baseline.rows);
        assert!(body_only.marks.is_empty());
        assert!(body_only.selection_parts.is_empty());
        assert!(body_only.runs.iter().all(|run| {
            run.label.owner == 0 && run.label.link == 0 && run.label.head_component == 0
        }));
        assert!(
            body_only
                .runs
                .iter()
                .zip(&baseline.runs)
                .all(|(body, prior)| body.label.source == prior.label.source
                    && body.label.style == prior.label.style
                    && body.column == prior.column
                    && body.width == prior.width)
        );

        // Even the annotation slice descriptor may be unusable. The body-only
        // branch never borrows it, so no borrowed mark can outlive the handle.
        let mut bad_descriptor = view;
        bad_descriptor.marks.ptr = std::ptr::null();
        assert_eq!(
            transfer(&handle, &bad_descriptor, &limits, false)
                .unwrap_err()
                .status,
            STATUS_RELATION
        );
        assert_eq!(
            transfer(&handle, &bad_descriptor, &limits, true)
                .unwrap()
                .text,
            baseline.text
        );

        rejects_optional_budget_overruns(&handle, &view, &limits, &original_marks);

        // A corrupt text arena is a hard error even on the body-only path.
        let invalid_utf8 = [0xff];
        let mut bad_body_view = view;
        bad_body_view.display.bytes = invalid_utf8.as_ptr();
        bad_body_view.display.byte_count = 1;
        assert_eq!(
            transfer(&handle, &bad_body_view, &limits, true)
                .unwrap_err()
                .status,
            STATUS_RELATION
        );

        // An unknown final style bit would be silently lost by typed projection.
        // It is a display-integrity error, not an annotation relation to discard.
        assert!(view.display.run_count > 0);
        let mut bad_runs = unsafe {
            std::slice::from_raw_parts(view.display.runs, view.display.run_count as usize)
        }
        .to_vec();
        bad_runs[0].label.style |= 2;
        let mut bad_style_view = view;
        bad_style_view.display.runs = bad_runs.as_ptr();
        assert_eq!(
            transfer(&handle, &bad_style_view, &limits, true)
                .unwrap_err()
                .status,
            STATUS_RELATION
        );
    });
}

#[test]
fn ffi_va_dv_role_corruption_rejects_marks_without_erasing_native_body() {
    // The exact input ran pinned CVS -Tutf8 first. mdoc_macro.c::in_line()
    // retains distinct Va/Dv instances, while mdoc_term.c renders their
    // visible operands. Only copied ABI descriptors are corrupted below;
    // the C-owned result remains sealed and cannot be mutated by this test.
    with_native_result_for(
        b".Dd September 26, 2026\n.Dt T 1\n.Os\n.Sh DESCRIPTION\n.Bl -tag -width Ds\n.It Va counter\nVariable.\n.It Dv MODE_FAST\nConstant.\n.El\n",
        InputFormat::Mdoc,
        |handle, view, limits| {
            let baseline = transfer(&handle, &view, &limits, false).unwrap();
            assert!(!baseline.annotation_degraded);
            assert!(baseline.text.contains("Variable."));
            assert!(baseline.text.contains("Constant."));
            let marks = unsafe {
                std::slice::from_raw_parts(view.marks.ptr.cast::<MarkView>(), view.marks.count as usize)
            }
            .to_vec();
            let variable = marks
                .iter()
                .position(|mark| mark.kind == 6 && mark.flags & (1 << 11) != 0)
                .expect("authored Va component");
            let defined = marks
                .iter()
                .position(|mark| mark.kind == 6 && mark.flags & (1 << 12) != 0)
                .expect("authored Dv component");
            for (index, corruption) in [
                (variable, 0_u8),
                (defined, 1),
                (variable, 2),
                (variable, 3),
            ] {
                let mut bad_marks = marks.clone();
                match corruption {
                    0 => bad_marks[index].token = marks[defined].token,
                    1 => bad_marks[index].token = marks[variable].token,
                    2 => bad_marks[index].flags |= 1 << 12,
                    _ => bad_marks[index].source = 0,
                }
                let mut bad_view = view;
                bad_view.marks.ptr = bad_marks.as_ptr().cast();
                assert_eq!(
                    transfer(&handle, &bad_view, &limits, false).unwrap_err().status,
                    STATUS_RELATION,
                    "corruption {corruption}",
                );
                let body_only = transfer(&handle, &bad_view, &limits, true).unwrap();
                assert!(body_only.annotation_degraded);
                assert_eq!(body_only.text, baseline.text);
                assert!(body_only.marks.is_empty());
                assert!(body_only.runs.iter().all(|run| run.label.head_component == 0));
            }
        },
    );
}

fn rejects_optional_budget_overruns(
    handle: &Handle,
    view: &ResultView,
    limits: &super::super::raw::Limits,
    original_marks: &[MarkView],
) {
    // Optional arenas and strings exceeding their budgets must not be
    // converted into a successful body-only semantic downgrade.
    for (field, maximum, kind) in [
        (0, limits.max_transfer_objects, 32),
        (1, limits.max_transfer_objects, 32),
        (2, limits.max_transfer_edges, 33),
        (3, limits.max_content_bytes, 10),
        (4, 24, 32),
    ] {
        let mut oversized = *view;
        let count = u32::try_from(maximum + 1).unwrap();
        match field {
            0 => oversized.marks.count = count,
            1 => oversized.coverage_issues.count = count,
            2 => oversized.selection_parts.count = count,
            3 => oversized.join_text.count = count,
            _ => oversized.coverage_checks.count = count,
        }
        let error = transfer(handle, &oversized, limits, false).unwrap_err();
        assert_eq!((error.status, error.limit_kind), (STATUS_BUDGET, kind));
    }
    for oversized_field in 0..3 {
        let mut marks = original_marks.to_vec();
        let length = limits.max_content_bytes + 1;
        match oversized_field {
            0 => marks[0].name_length = length,
            1 => marks[0].target_a.len = length,
            _ => marks[0].target_b.len = length,
        }
        let mut oversized = *view;
        oversized.marks.ptr = marks.as_ptr().cast();
        let error = transfer(handle, &oversized, limits, false).unwrap_err();
        assert_eq!((error.status, error.limit_kind), (STATUS_BUDGET, 10));
    }
}
