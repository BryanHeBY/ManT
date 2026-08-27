use crate::parser::token::{MdocToken, PackageToken};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum MacroForm {
    Inline,
    ImplicitPartial,
    ExplicitPartial,
    Other,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NestedDisposition {
    Callable,
    NonCallable,
    Literal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct MacroSpec {
    token: MdocToken,
    form: MacroForm,
    nested: NestedDisposition,
    argument_limit: Option<usize>,
    close: Option<MdocToken>,
}

impl MacroSpec {
    pub(super) fn classify(name: &str) -> Option<Self> {
        let token = MdocToken::classify(name.as_bytes())?;
        Some(Self {
            token,
            form: macro_form(token),
            nested: nested_disposition(token),
            argument_limit: argument_limit(token),
            close: explicit_close(token),
        })
    }

    pub(super) const fn form(self) -> MacroForm {
        self.form
    }

    pub(super) const fn is_callable(self) -> bool {
        matches!(self.nested, NestedDisposition::Callable)
    }

    pub(super) const fn is_noncallable(self) -> bool {
        matches!(self.nested, NestedDisposition::NonCallable)
    }

    pub(super) const fn argument_limit(self) -> Option<usize> {
        self.argument_limit
    }

    pub(super) const fn close(self) -> Option<MdocToken> {
        self.close
    }
}

const fn macro_form(token: MdocToken) -> MacroForm {
    match token {
        MdocToken::Ad
        | MdocToken::An
        | MdocToken::Ap
        | MdocToken::Ar
        | MdocToken::Bsx
        | MdocToken::Bx
        | MdocToken::Cd
        | MdocToken::Cm
        | MdocToken::Dx
        | MdocToken::Dv
        | MdocToken::Em
        | MdocToken::Er
        | MdocToken::Ev
        | MdocToken::Fa
        | MdocToken::Fl
        | MdocToken::Fn
        | MdocToken::Fx
        | MdocToken::Ft
        | MdocToken::Ic
        | MdocToken::In
        | MdocToken::Lk
        | MdocToken::Li
        | MdocToken::Ms
        | MdocToken::Mt
        | MdocToken::Nm
        | MdocToken::No
        | MdocToken::Ns
        | MdocToken::Ot
        | MdocToken::Nx
        | MdocToken::Ox
        | MdocToken::Pa
        | MdocToken::Pf
        | MdocToken::St
        | MdocToken::Sx
        | MdocToken::Sy
        | MdocToken::Tn
        | MdocToken::Ux
        | MdocToken::Va
        | MdocToken::Vt
        | MdocToken::Xr => MacroForm::Inline,
        MdocToken::Aq
        | MdocToken::Bq
        | MdocToken::Brq
        | MdocToken::Dq
        | MdocToken::Op
        | MdocToken::Pq
        | MdocToken::Ql
        | MdocToken::Qq
        | MdocToken::Sq => MacroForm::ImplicitPartial,
        MdocToken::Ao
        | MdocToken::Bo
        | MdocToken::Bro
        | MdocToken::Do
        | MdocToken::Eo
        | MdocToken::Oo
        | MdocToken::Po
        | MdocToken::Qo
        | MdocToken::So
        | MdocToken::Xo => MacroForm::ExplicitPartial,
        _ => MacroForm::Other,
    }
}

const fn nested_disposition(token: MdocToken) -> NestedDisposition {
    if PackageToken::Mdoc(token).is_mdoc_callable() {
        NestedDisposition::Callable
    } else if matches!(
        token,
        MdocToken::Dd
            | MdocToken::Dt
            | MdocToken::Os
            | MdocToken::Sh
            | MdocToken::Ss
            | MdocToken::Pp
            | MdocToken::D1
            | MdocToken::Dl
            | MdocToken::Bd
            | MdocToken::Ed
            | MdocToken::Bl
            | MdocToken::El
            | MdocToken::It
            | MdocToken::Ex
            | MdocToken::Fd
            | MdocToken::Nd
            | MdocToken::Rv
            | MdocToken::PercentA
            | MdocToken::PercentB
            | MdocToken::PercentD
            | MdocToken::PercentI
            | MdocToken::PercentJ
            | MdocToken::PercentN
            | MdocToken::PercentO
            | MdocToken::PercentP
            | MdocToken::PercentR
            | MdocToken::PercentT
            | MdocToken::PercentV
            | MdocToken::Bf
            | MdocToken::Db
            | MdocToken::Ef
            | MdocToken::Re
            | MdocToken::Rs
            | MdocToken::Sm
            | MdocToken::Bk
            | MdocToken::Ek
            | MdocToken::Bt
            | MdocToken::Hf
            | MdocToken::Ud
            | MdocToken::Lb
            | MdocToken::Lp
            | MdocToken::PercentC
            | MdocToken::PercentQ
            | MdocToken::PercentU
            | MdocToken::Tg
    ) {
        NestedDisposition::NonCallable
    } else {
        NestedDisposition::Literal
    }
}

const fn argument_limit(token: MdocToken) -> Option<usize> {
    match token {
        MdocToken::Ap | MdocToken::Ns | MdocToken::Ux => Some(0),
        MdocToken::Bsx
        | MdocToken::Dx
        | MdocToken::Fx
        | MdocToken::In
        | MdocToken::Nx
        | MdocToken::Ox
        | MdocToken::Pf
        | MdocToken::St => Some(1),
        MdocToken::Bx | MdocToken::Xr => Some(2),
        _ => None,
    }
}

const fn explicit_close(token: MdocToken) -> Option<MdocToken> {
    match token {
        MdocToken::Ao => Some(MdocToken::Ac),
        MdocToken::Bo => Some(MdocToken::Bc),
        MdocToken::Bro => Some(MdocToken::Brc),
        MdocToken::Do => Some(MdocToken::Dc),
        MdocToken::Eo => Some(MdocToken::Ec),
        MdocToken::Oo => Some(MdocToken::Oc),
        MdocToken::Po => Some(MdocToken::Pc),
        MdocToken::Qo => Some(MdocToken::Qc),
        MdocToken::So => Some(MdocToken::Sc),
        MdocToken::Xo => Some(MdocToken::Xc),
        _ => None,
    }
}

pub(super) fn is_inline_mdoc_macro(name: &str) -> bool {
    MacroSpec::classify(name).is_some_and(|spec| spec.form() == MacroForm::Inline)
}

pub(super) fn is_implicit_partial_block_macro(name: &str) -> bool {
    MacroSpec::classify(name).is_some_and(|spec| spec.form() == MacroForm::ImplicitPartial)
}

pub(super) fn implicit_partial_block_name(name: &str) -> &'static str {
    let spec = MacroSpec::classify(name)
        .filter(|spec| spec.form() == MacroForm::ImplicitPartial)
        .expect("caller checked the implicit partial block grammar");
    std::str::from_utf8(spec.token.name()).expect("mdoc macro names are ASCII")
}

pub(super) fn is_mdoc_callable_macro(name: &str) -> bool {
    MacroSpec::classify(name).is_some_and(MacroSpec::is_callable)
}

pub(super) fn is_mdoc_noncallable_macro(name: &str) -> bool {
    MacroSpec::classify(name).is_some_and(MacroSpec::is_noncallable)
}

pub(super) fn mdoc_inline_argument_limit(name: &str) -> Option<usize> {
    MacroSpec::classify(name).and_then(MacroSpec::argument_limit)
}

pub(super) fn explicit_partial_block_close(name: &str) -> Option<&'static str> {
    let close = MacroSpec::classify(name)?.close()?;
    Some(std::str::from_utf8(close.name()).expect("mdoc macro names are ASCII"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macro_specs_keep_forms_arity_and_close_pairs_together() {
        let xr = MacroSpec::classify("Xr").unwrap();
        assert_eq!(xr.form(), MacroForm::Inline);
        assert_eq!(xr.argument_limit(), Some(2));
        assert!(xr.is_callable());

        let bo = MacroSpec::classify("Bo").unwrap();
        assert_eq!(bo.form(), MacroForm::ExplicitPartial);
        assert_eq!(bo.close(), Some(MdocToken::Bc));
        assert_eq!(explicit_partial_block_close("Bo"), Some("Bc"));

        assert!(is_mdoc_noncallable_macro("Sh"));
        assert!(!is_mdoc_callable_macro("Sh"));
        assert!(MacroSpec::classify("local-macro").is_none());
    }
}
