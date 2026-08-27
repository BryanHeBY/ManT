use super::super::{
    Diagnostic, DiagnosticCode, DocumentBuilder, Environment, EnvironmentRequest, Limits, Severity,
    apply_environment_request, apply_string_request, diagnostic, emit_escaped_request_name,
    environment_error_diagnostic, lex_arguments, normalize_roff_name_prefix, push_diagnostic,
    register_division_by_zero,
};

pub(in crate::parser) enum RequestHandling {
    Handled,
    Unhandled,
}

pub(in crate::parser) struct EnvironmentRequestContext<'a> {
    pub(in crate::parser) request: EnvironmentRequest,
    pub(in crate::parser) arguments: &'a [u8],
    pub(in crate::parser) escape: u8,
    pub(in crate::parser) source_id: crate::SourceId,
    pub(in crate::parser) start: u32,
    pub(in crate::parser) end: u32,
    pub(in crate::parser) control_start: u32,
    pub(in crate::parser) argument_start: u32,
    pub(in crate::parser) environment: &'a mut Environment,
    pub(in crate::parser) builder: &'a mut DocumentBuilder,
    pub(in crate::parser) limits: &'a Limits,
    pub(in crate::parser) expansion_steps: &'a mut usize,
    pub(in crate::parser) diagnostics: &'a mut Vec<Diagnostic>,
    pub(in crate::parser) truncated: &'a mut bool,
}

pub(in crate::parser) fn execute_environment_request(
    context: EnvironmentRequestContext<'_>,
) -> RequestHandling {
    if matches!(
        context.request,
        EnvironmentRequest::DefineString | EnvironmentRequest::AppendString
    ) {
        execute_string_request(context);
        RequestHandling::Handled
    } else {
        execute_generic_request(context)
    }
}

fn execute_string_request(context: EnvironmentRequestContext<'_>) {
    let EnvironmentRequestContext {
        request,
        arguments,
        escape,
        source_id,
        start,
        end,
        control_start: _,
        argument_start,
        environment,
        builder: _,
        limits,
        expansion_steps,
        diagnostics,
        truncated,
    } = context;
    if let Ok(arguments) = lex_arguments(arguments, escape, limits) {
        emit_escaped_request_name(
            &arguments,
            escape,
            argument_start,
            source_id,
            limits,
            diagnostics,
            truncated,
        );
    }
    if let Err(error) = apply_string_request(
        environment,
        arguments,
        escape,
        request.appends_string(),
        limits,
        source_id,
        start,
        end,
        expansion_steps,
        diagnostics,
        truncated,
    ) {
        *truncated = true;
        push_diagnostic(
            diagnostics,
            limits,
            environment_error_diagnostic(error, source_id, start, end),
            truncated,
        );
    }
}

fn execute_generic_request(context: EnvironmentRequestContext<'_>) -> RequestHandling {
    let EnvironmentRequestContext {
        request,
        arguments,
        escape,
        source_id,
        start,
        end,
        control_start,
        argument_start,
        environment,
        builder,
        limits,
        expansion_steps: _,
        diagnostics,
        truncated,
    } = context;
    let Ok(arguments) = lex_arguments(arguments, escape, limits) else {
        // Preserve the existing dispatcher recovery: malformed generic
        // arguments continue to package/user-macro classification.
        return RequestHandling::Unhandled;
    };

    emit_request_name_diagnostics(
        request,
        &arguments,
        escape,
        argument_start,
        source_id,
        limits,
        diagnostics,
        truncated,
    );

    let division_by_zero = (request == EnvironmentRequest::DefineRegister)
        .then(|| register_division_by_zero(&arguments))
        .flatten();
    match apply_environment_request(
        environment,
        builder,
        request.name(),
        escape,
        &arguments,
        limits,
    ) {
        Ok(()) => {
            if let Some(expression) = division_by_zero {
                push_diagnostic(
                    diagnostics,
                    limits,
                    diagnostic(
                        DiagnosticCode::ROFF_DIVISION_BY_ZERO,
                        Severity::Error,
                        source_id,
                        control_start.saturating_add(2),
                        control_start.saturating_add(3),
                        format!(
                            "divide by zero: {}",
                            super::super::visible_bytes(&expression.bytes)
                        ),
                    ),
                    truncated,
                );
            }
        }
        Err(error) => {
            *truncated = true;
            push_diagnostic(
                diagnostics,
                limits,
                environment_error_diagnostic(error, source_id, start, end),
                truncated,
            );
        }
    }
    RequestHandling::Handled
}

#[allow(clippy::too_many_arguments)]
fn emit_request_name_diagnostics(
    request: EnvironmentRequest,
    arguments: &[super::super::Argument],
    escape: u8,
    argument_start: u32,
    source_id: crate::SourceId,
    limits: &Limits,
    diagnostics: &mut Vec<Diagnostic>,
    truncated: &mut bool,
) {
    if matches!(
        request,
        EnvironmentRequest::DefineRegister | EnvironmentRequest::RemoveRegister
    ) {
        emit_escaped_request_name(
            arguments,
            escape,
            argument_start,
            source_id,
            limits,
            diagnostics,
            truncated,
        );
    }
    if request != EnvironmentRequest::Remove {
        return;
    }
    let Some(argument) = arguments.iter().find(|argument| {
        normalize_roff_name_prefix(&argument.bytes, escape)
            .invalid_escape_preview
            .is_some()
    }) else {
        return;
    };
    emit_escaped_request_name(
        std::slice::from_ref(argument),
        escape,
        argument_start,
        source_id,
        limits,
        diagnostics,
        truncated,
    );
}
