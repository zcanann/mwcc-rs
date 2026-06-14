//! AST -> PPC codegen for the v0 subset.
//!
//! This is where the byte-matching battle is fought. v0 reproduces mwcc 1.3.2's
//! output for leaf and single-binop returns under the PPC EABI:
//!   - integer args  r3, r4, r5, ...   integer return  r3
//!   - float   args  f1, f2, f3, ...   float   return  f1
//!   - leaf functions emit no stack frame; redundant moves are elided.
//! Deeper expression trees are best-effort for now and will be tuned against the
//! oracle (the real mwcceppc) via the A/B harness.

use crate::parser::{Expr, Func, Ty};
use crate::ppc;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq)]
enum Class {
    Gpr,
    Fpr,
}

struct Loc {
    class: Class,
    reg: u32,
}

pub struct Gen {
    words: Vec<u32>,
    locs: HashMap<String, Loc>,
}

impl Gen {
    pub fn new() -> Self {
        Gen { words: Vec::new(), locs: HashMap::new() }
    }

    /// Generate a function body, returning the encoded big-endian text bytes.
    pub fn gen(mut self, f: &Func) -> Result<Vec<u8>, String> {
        // Assign parameter registers per EABI.
        let mut gpr = 3u32;
        let mut fpr = 1u32;
        for p in &f.params {
            match p.ty {
                Ty::Int => {
                    self.locs.insert(p.name.clone(), Loc { class: Class::Gpr, reg: gpr });
                    gpr += 1;
                }
                Ty::Float => {
                    self.locs.insert(p.name.clone(), Loc { class: Class::Fpr, reg: fpr });
                    fpr += 1;
                }
                Ty::Void => return Err("void parameter".into()),
            }
        }

        match f.ret {
            Ty::Int => self.eval_gpr(&f.ret_expr, 3)?,
            Ty::Float => self.eval_fpr(&f.ret_expr, 1)?,
            Ty::Void => {}
        }
        self.words.push(ppc::blr());

        let mut bytes = Vec::with_capacity(self.words.len() * 4);
        for w in &self.words {
            bytes.extend_from_slice(&w.to_be_bytes());
        }
        Ok(bytes)
    }

    /// Evaluate an integer expression into GPR `target`.
    fn eval_gpr(&mut self, e: &Expr, target: u32) -> Result<(), String> {
        match e {
            Expr::IntLit(n) => {
                self.load_imm(target, *n);
                Ok(())
            }
            Expr::Var(name) => {
                let loc = self.locs.get(name).ok_or(format!("unknown var {name}"))?;
                if loc.class != Class::Gpr {
                    return Err(format!("{name} is not an int"));
                }
                if loc.reg != target {
                    let r = loc.reg;
                    self.words.push(ppc::mr(target, r));
                }
                Ok(())
            }
            Expr::Bin(op, a, b) => {
                // Compute lhs into target, then combine with rhs.
                self.eval_gpr(a, target)?;
                let rb = self.leaf_gpr(b)?;
                self.words.push(match op {
                    '+' => ppc::add(target, target, rb),
                    // `sub rD,rA,rB` = rA-rB = subf rD,rB,rA
                    '-' => ppc::subf(target, rb, target),
                    '*' => ppc::mullw(target, target, rb),
                    _ => return Err(format!("int op {op} not yet supported")),
                });
                Ok(())
            }
            Expr::FloatLit(_) => Err("float literal in int context".into()),
        }
    }

    /// A leaf integer operand that already lives in a register (a param).
    fn leaf_gpr(&self, e: &Expr) -> Result<u32, String> {
        match e {
            Expr::Var(name) => {
                let loc = self.locs.get(name).ok_or(format!("unknown var {name}"))?;
                if loc.class != Class::Gpr {
                    return Err(format!("{name} is not an int"));
                }
                Ok(loc.reg)
            }
            _ => Err("v0: rhs of binop must be a parameter".into()),
        }
    }

    /// Evaluate a float expression into FPR `target`.
    fn eval_fpr(&mut self, e: &Expr, target: u32) -> Result<(), String> {
        match e {
            Expr::Var(name) => {
                let loc = self.locs.get(name).ok_or(format!("unknown var {name}"))?;
                if loc.class != Class::Fpr {
                    return Err(format!("{name} is not a float"));
                }
                if loc.reg != target {
                    let r = loc.reg;
                    self.words.push(ppc::fmr(target, r));
                }
                Ok(())
            }
            Expr::Bin(op, a, b) => {
                self.eval_fpr(a, target)?;
                let fb = self.leaf_fpr(b)?;
                self.words.push(match op {
                    '+' => ppc::fadds(target, target, fb),
                    '-' => ppc::fsubs(target, target, fb),
                    '*' => ppc::fmuls(target, target, fb),
                    _ => return Err(format!("float op {op} not yet supported")),
                });
                Ok(())
            }
            Expr::FloatLit(_) => Err("v0: float literals not yet supported (need constant pool)".into()),
            Expr::IntLit(_) => Err("int literal in float context".into()),
        }
    }

    fn leaf_fpr(&self, e: &Expr) -> Result<u32, String> {
        match e {
            Expr::Var(name) => {
                let loc = self.locs.get(name).ok_or(format!("unknown var {name}"))?;
                if loc.class != Class::Fpr {
                    return Err(format!("{name} is not a float"));
                }
                Ok(loc.reg)
            }
            _ => Err("v0: rhs of float binop must be a parameter".into()),
        }
    }

    /// Load a 32-bit integer constant, matching mwcc's `li` / `lis`+`ori`.
    fn load_imm(&mut self, target: u32, n: i64) {
        let n = n as i32;
        if (-0x8000..=0x7fff).contains(&n) {
            self.words.push(ppc::li(target, n as i16));
        } else {
            // mwcc pattern: lis rD, ha16 ; addi rD, rD, lo16
            // ha16 compensates for addi's sign-extension of lo16.
            let v = n as u32;
            let lo = (v & 0xffff) as i16;
            let ha = (((v as i32) - (lo as i32)) >> 16) as i16;
            self.words.push(ppc::addis(target, 0, ha)); // lis
            self.words.push(ppc::addi(target, target, lo));
        }
    }
}
