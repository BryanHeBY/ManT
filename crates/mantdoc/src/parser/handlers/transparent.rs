use super::super::{
    Diagnostic, DiagnosticCode, EmitContext, Environment, InputTrap, Limits, Severity,
    TransparentRequest, arm_input_trap, diagnostic, emit_translation_request_diagnostics,
    push_diagnostic, trim_horizontal_space, validate_character_request, visible_bytes,
};
use super::RequestTransition;

pub(in crate::parser) struct TransparentRequestContext<'a> {
    pub(in crate::parser) request: TransparentRequest,
    pub(in crate::parser) arguments: &'a [u8],
    pub(in crate::parser) escape: u8,
    pub(in crate::parser) source_id: crate::SourceId,
    pub(in crate::parser) end: u32,
    pub(in crate::parser) control_start: u32,
    pub(in crate::parser) argument_start: u32,
    pub(in crate::parser) environment: &'a mut Environment,
    pub(in crate::parser) input_trap: &'a mut InputTrap,
    pub(in crate::parser) text_bytes: &'a mut usize,
    pub(in crate::parser) limits: &'a Limits,
    pub(in crate::parser) diagnostics: &'a mut Vec<Diagnostic>,
    pub(in crate::parser) truncated: &'a mut bool,
}

pub(in crate::parser) fn execute_transparent_request(
    mut context: TransparentRequestContext<'_>,
) -> RequestTransition {
    match context.request {
        TransparentRequest::Translation => execute_translation(&mut context),
        TransparentRequest::Character => execute_character(&mut context),
        TransparentRequest::InputTrap => execute_input_trap(&mut context),
    }
    RequestTransition::Consumed
}

fn execute_translation(context: &mut TransparentRequestContext<'_>) {
    emit_translation_request_diagnostics(
        context.arguments,
        context.escape,
        context.control_start,
        context.argument_start,
        &mut EmitContext::new(
            context.source_id,
            context.limits,
            context.text_bytes,
            context.diagnostics,
            context.truncated,
        ),
    );
    context
        .environment
        .define_translation(context.arguments, context.escape);
}

fn execute_character(context: &mut TransparentRequestContext<'_>) {
    validate_character_request(
        context.arguments,
        context.escape,
        context.environment,
        context.source_id,
        context.argument_start,
        context.end,
        context.limits,
        context.diagnostics,
        context.truncated,
    );
}

fn execute_input_trap(context: &mut TransparentRequestContext<'_>) {
    if arm_input_trap(context.input_trap, context.arguments) {
        return;
    }
    let display = visible_bytes(trim_horizontal_space(context.arguments));
    let display = (!display.is_empty()).then(|| format!(" {display}"));
    push_diagnostic(
        context.diagnostics,
        context.limits,
        diagnostic(
            DiagnosticCode::ROFF_NON_NUMERIC_ARGUMENT,
            Severity::Error,
            context.source_id,
            context.control_start,
            context.control_start.saturating_add(2),
            format!(
                "skipping request without numeric argument: it{}",
                display.unwrap_or_default()
            ),
        ),
        context.truncated,
    );
}
