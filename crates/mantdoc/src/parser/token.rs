use crate::MacroSet;

macro_rules! define_tokens {
    ($name:ident { $($variant:ident => $spelling:literal),+ $(,)? }) => {
        #[derive(Clone, Copy, Debug, Eq, PartialEq)]
        pub(super) enum $name {
            $($variant),+
        }

        impl $name {
            #[cfg(test)]
            const ALL: &'static [Self] = &[$(Self::$variant),+];

            #[cfg(test)]
            pub(super) const fn name(self) -> &'static [u8] {
                match self {
                    $(Self::$variant => $spelling),+
                }
            }

            fn classify(name: &[u8]) -> Option<Self> {
                match name {
                    $($spelling => Some(Self::$variant),)+
                    _ => None,
                }
            }
        }
    };
}

define_tokens!(RoffToken {
    Ad => b"ad",
    Br => b"br",
    Ce => b"ce",
    Fi => b"fi",
    Ft => b"ft",
    Hy => b"hy",
    Na => b"na",
    Nf => b"nf",
    Nh => b"nh",
    Rj => b"rj",
    Sp => b"sp",
});

define_tokens!(ManToken {
    Th => b"TH",
    Sh => b"SH",
    Ss => b"SS",
    Tp => b"TP",
    Tq => b"TQ",
    Lp => b"LP",
    Pp => b"PP",
    P => b"P",
    Ip => b"IP",
    Hp => b"HP",
    Sm => b"SM",
    Sb => b"SB",
    Bi => b"BI",
    Ib => b"IB",
    Br => b"BR",
    Rb => b"RB",
    R => b"R",
    B => b"B",
    I => b"I",
    Ir => b"IR",
    Ri => b"RI",
    Re => b"RE",
    Rs => b"RS",
    Dt => b"DT",
    Uc => b"UC",
    Pd => b"PD",
    At => b"AT",
    In => b"in",
    Sy => b"SY",
    Ys => b"YS",
    Op => b"OP",
    Ex => b"EX",
    Ee => b"EE",
    Ur => b"UR",
    Ue => b"UE",
    Mt => b"MT",
    Me => b"ME",
    Mr => b"MR",
});

define_tokens!(MdocToken {
    Dd => b"Dd", Dt => b"Dt", Os => b"Os", Sh => b"Sh", Ss => b"Ss",
    Pp => b"Pp", D1 => b"D1", Dl => b"Dl", Bd => b"Bd", Ed => b"Ed",
    Bl => b"Bl", El => b"El", It => b"It", Ad => b"Ad", An => b"An",
    Ap => b"Ap", Ar => b"Ar", Cd => b"Cd", Cm => b"Cm", Dv => b"Dv",
    Er => b"Er", Ev => b"Ev", Ex => b"Ex", Fa => b"Fa", Fd => b"Fd",
    Fl => b"Fl", Fn => b"Fn", Ft => b"Ft", Ic => b"Ic", In => b"In",
    Li => b"Li", Nd => b"Nd", Nm => b"Nm", Op => b"Op", Ot => b"Ot",
    Pa => b"Pa", Rv => b"Rv", St => b"St", Va => b"Va", Vt => b"Vt",
    Xr => b"Xr", PercentA => b"%A", PercentB => b"%B", PercentD => b"%D",
    PercentI => b"%I", PercentJ => b"%J", PercentN => b"%N",
    PercentO => b"%O", PercentP => b"%P", PercentR => b"%R",
    PercentT => b"%T", PercentV => b"%V", Ac => b"Ac", Ao => b"Ao",
    Aq => b"Aq", At => b"At", Bc => b"Bc", Bf => b"Bf", Bo => b"Bo",
    Bq => b"Bq", Bsx => b"Bsx", Bx => b"Bx", Db => b"Db", Dc => b"Dc",
    Do => b"Do", Dq => b"Dq", Ec => b"Ec", Ef => b"Ef", Em => b"Em",
    Eo => b"Eo", Fx => b"Fx", Ms => b"Ms", No => b"No", Ns => b"Ns",
    Nx => b"Nx", Ox => b"Ox", Pc => b"Pc", Pf => b"Pf", Po => b"Po",
    Pq => b"Pq", Qc => b"Qc", Ql => b"Ql", Qo => b"Qo", Qq => b"Qq",
    Re => b"Re", Rs => b"Rs", Sc => b"Sc", So => b"So", Sq => b"Sq",
    Sm => b"Sm", Sx => b"Sx", Sy => b"Sy", Tn => b"Tn", Ux => b"Ux",
    Xc => b"Xc", Xo => b"Xo", Fo => b"Fo", Fc => b"Fc", Oo => b"Oo",
    Oc => b"Oc", Bk => b"Bk", Ek => b"Ek", Bt => b"Bt", Hf => b"Hf",
    Fr => b"Fr", Ud => b"Ud", Lb => b"Lb", Lp => b"Lp", Lk => b"Lk",
    Mt => b"Mt", Brq => b"Brq", Bro => b"Bro", Brc => b"Brc",
    PercentC => b"%C", Es => b"Es", En => b"En", Dx => b"Dx",
    PercentQ => b"%Q", PercentU => b"%U", Ta => b"Ta", Tg => b"Tg",
});

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum PackageToken {
    Roff(RoffToken),
    Man(ManToken),
    Mdoc(MdocToken),
    Unknown,
}

impl PackageToken {
    pub(super) fn classify(macro_set: MacroSet, name: &[u8]) -> Self {
        match macro_set {
            MacroSet::Man => ManToken::classify(name)
                .map(Self::Man)
                .or_else(|| RoffToken::classify(name).map(Self::Roff))
                .unwrap_or(Self::Unknown),
            MacroSet::Mdoc => MdocToken::classify(name)
                .map(Self::Mdoc)
                .or_else(|| RoffToken::classify(name).map(Self::Roff))
                .unwrap_or(Self::Unknown),
            MacroSet::None => RoffToken::classify(name).map_or(Self::Unknown, Self::Roff),
        }
    }

    #[cfg(test)]
    pub(super) const fn name(self) -> Option<&'static [u8]> {
        match self {
            Self::Roff(token) => Some(token.name()),
            Self::Man(token) => Some(token.name()),
            Self::Mdoc(token) => Some(token.name()),
            Self::Unknown => None,
        }
    }

    pub(super) const fn is_builtin(self, macro_set: MacroSet) -> bool {
        match self {
            Self::Roff(token) => {
                matches!(macro_set, MacroSet::Man) && !matches!(token, RoffToken::Ft)
            }
            Self::Man(token) => !matches!(
                token,
                ManToken::Dt | ManToken::Uc | ManToken::At | ManToken::Op | ManToken::Mr
            ),
            Self::Mdoc(token) => matches!(token, MdocToken::At | MdocToken::Bc),
            Self::Unknown => false,
        }
    }

    pub(super) const fn is_man_visible_argument(self) -> bool {
        matches!(
            self,
            Self::Man(
                ManToken::B
                    | ManToken::I
                    | ManToken::R
                    | ManToken::Sm
                    | ManToken::Sb
                    | ManToken::Br
                    | ManToken::Bi
                    | ManToken::Ib
                    | ManToken::Ir
                    | ManToken::Rb
                    | ManToken::Ri
                    | ManToken::Ip
                    | ManToken::Hp
                    | ManToken::Tp
                    | ManToken::Tq
            )
        )
    }

    pub(super) const fn is_mdoc_callable(self) -> bool {
        matches!(
            self,
            Self::Mdoc(
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
                    | MdocToken::Xr
                    | MdocToken::Aq
                    | MdocToken::Bq
                    | MdocToken::Brq
                    | MdocToken::Dq
                    | MdocToken::Op
                    | MdocToken::Pq
                    | MdocToken::Ql
                    | MdocToken::Qq
                    | MdocToken::Sq
                    | MdocToken::Ao
                    | MdocToken::Bo
                    | MdocToken::Bro
                    | MdocToken::Do
                    | MdocToken::Eo
                    | MdocToken::Oo
                    | MdocToken::Po
                    | MdocToken::Qo
                    | MdocToken::So
                    | MdocToken::Xo
                    | MdocToken::Ec
                    | MdocToken::Fc
            )
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_catalogues_round_trip_without_duplicate_spellings() {
        for (index, token) in RoffToken::ALL.iter().enumerate() {
            assert!(
                !RoffToken::ALL[..index]
                    .iter()
                    .any(|prior| prior.name() == token.name())
            );
        }
        for (index, token) in ManToken::ALL.iter().enumerate() {
            assert!(
                !ManToken::ALL[..index]
                    .iter()
                    .any(|prior| prior.name() == token.name())
            );
        }
        for (index, token) in MdocToken::ALL.iter().enumerate() {
            assert!(
                !MdocToken::ALL[..index]
                    .iter()
                    .any(|prior| prior.name() == token.name())
            );
            assert_eq!(
                PackageToken::classify(MacroSet::Mdoc, token.name()).name(),
                Some(token.name())
            );
        }
    }

    #[test]
    fn compatibility_builtin_and_callable_sets_remain_distinct() {
        assert!(PackageToken::classify(MacroSet::Man, b"TH").is_builtin(MacroSet::Man));
        assert!(!PackageToken::classify(MacroSet::Man, b"MR").is_builtin(MacroSet::Man));
        assert!(PackageToken::classify(MacroSet::Mdoc, b"At").is_builtin(MacroSet::Mdoc));
        assert!(!PackageToken::classify(MacroSet::Mdoc, b"Sh").is_builtin(MacroSet::Mdoc));
        assert!(PackageToken::classify(MacroSet::Mdoc, b"Xr").is_mdoc_callable());
        assert!(!PackageToken::classify(MacroSet::Mdoc, b"Sh").is_mdoc_callable());
    }
}
