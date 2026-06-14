//! PowerPC (Gekko) instruction encoders. Big-endian 32-bit words.
//!
//! Only the encoders v0 needs. Each returns the 32-bit instruction word; the
//! ELF writer serialises big-endian. Encodings verified against real
//! `mwcceppc 1.3.2` output (see oracle_probe/).

/// D-form: opcode, rD/rS, rA, 16-bit signed/unsigned immediate.
fn d_form(opcode: u32, d: u32, a: u32, imm: u16) -> u32 {
    (opcode << 26) | (d << 21) | (a << 16) | (imm as u32)
}

/// XO-form (integer): opcode 31, rD, rA, rB, OE=0, extended opcode, Rc=0.
fn xo_form(d: u32, a: u32, b: u32, xo: u32) -> u32 {
    (31 << 26) | (d << 21) | (a << 16) | (b << 11) | (xo << 1)
}

/// A-form (float): opcode, frD, frA, frB, frC, extended opcode, Rc=0.
fn a_form(opcode: u32, d: u32, a: u32, b: u32, c: u32, xo: u32) -> u32 {
    (opcode << 26) | (d << 21) | (a << 16) | (b << 11) | (c << 6) | (xo << 1)
}

// --- integer ---

/// `addi rD, rA, SIMM`. `li rD, SIMM` is `addi rD, r0, SIMM`.
pub fn addi(d: u32, a: u32, simm: i16) -> u32 {
    d_form(14, d, a, simm as u16)
}
/// `li rD, SIMM`
pub fn li(d: u32, simm: i16) -> u32 {
    addi(d, 0, simm)
}
/// `addis rD, rA, SIMM`. `lis rD, SIMM` is `addis rD, r0, SIMM`.
pub fn addis(d: u32, a: u32, simm: i16) -> u32 {
    d_form(15, d, a, simm as u16)
}
/// `ori rA, rS, UIMM`
pub fn ori(a: u32, s: u32, uimm: u16) -> u32 {
    d_form(24, s, a, uimm)
}
/// `add rD, rA, rB`
pub fn add(d: u32, a: u32, b: u32) -> u32 {
    xo_form(d, a, b, 266)
}
/// `subf rD, rA, rB`  => rD = rB - rA.  (`sub rD,rA,rB` is `subf rD,rB,rA`)
pub fn subf(d: u32, a: u32, b: u32) -> u32 {
    xo_form(d, a, b, 40)
}
/// `mullw rD, rA, rB`
pub fn mullw(d: u32, a: u32, b: u32) -> u32 {
    xo_form(d, a, b, 235)
}
/// `or rA, rS, rB`. `mr rA, rS` is `or rA, rS, rS`.
pub fn or(a: u32, s: u32, b: u32) -> u32 {
    (31 << 26) | (s << 21) | (a << 16) | (b << 11) | (444 << 1)
}
pub fn mr(a: u32, s: u32) -> u32 {
    or(a, s, s)
}

// --- float (single) ---

/// `fadds frD, frA, frB`
pub fn fadds(d: u32, a: u32, b: u32) -> u32 {
    a_form(59, d, a, b, 0, 21)
}
/// `fsubs frD, frA, frB`
pub fn fsubs(d: u32, a: u32, b: u32) -> u32 {
    a_form(59, d, a, b, 0, 20)
}
/// `fmuls frD, frA, frC`  (note: frC field, frB=0)
pub fn fmuls(d: u32, a: u32, c: u32) -> u32 {
    a_form(59, d, a, 0, c, 25)
}
/// `fmr frD, frB`
pub fn fmr(d: u32, b: u32) -> u32 {
    (63 << 26) | (d << 21) | (b << 11) | (72 << 1)
}

// --- branch ---

/// `blr` — branch to link register (function return).
pub fn blr() -> u32 {
    0x4E80_0020
}
