//! Typed man(7) macros from the pinned token registry.

define_family! {
    /// All named man(7) macros in the pinned CVS registry.
    ///
    /// Variants are Rust identities, not upstream numeric token values.
    pub enum ManMacro {
        Th => "TH",
        Sh => "SH",
        Ss => "SS",
        Tp => "TP",
        Tq => "TQ",
        Lp => "LP",
        Pp => "PP",
        P => "P",
        Ip => "IP",
        Hp => "HP",
        Sm => "SM",
        Sb => "SB",
        Bi => "BI",
        Ib => "IB",
        Br => "BR",
        Rb => "RB",
        R => "R",
        B => "B",
        I => "I",
        Ir => "IR",
        Ri => "RI",
        Re => "RE",
        Rs => "RS",
        Dt => "DT",
        Uc => "UC",
        Pd => "PD",
        At => "AT",
        In => "in",
        Sy => "SY",
        Ys => "YS",
        Op => "OP",
        Ex => "EX",
        Ee => "EE",
        Ur => "UR",
        Ue => "UE",
        Mt => "MT",
        Me => "ME",
        Mr => "MR",
    }
}
