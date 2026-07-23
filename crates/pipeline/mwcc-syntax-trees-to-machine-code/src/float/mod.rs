//! The FLOAT DAG return arm: a `double` function returning a pure
//! multiply-add tree of double parameters and pooled double constants runs
//! through the FROZEN float models (fires 331-336): the linearizer's float
//! contract (HAZARD_FPU single pipe, the load port, the blocked-load stall
//! and empty-cycle lift) orders the body, and the hybrid float register
//! machine (reverse death-order allocation with boundary shares) assigns the
//! FPRs. Captured vocabulary ONLY: fmadd (contracted under fp_contract) and
//! fmul — a tree with an unfused add, subtract, divide, negate, memory load,
//! duplicate literal, or constant-folded pair DEFERS.

use mwcc_machine_code::Instruction;
use mwcc_syntax_trees::{BinaryOperator, Expression};

/// A value in the lowered tree: a parameter's DAG value id or a node index.
#[derive(Clone, Copy)]
enum Operand {
    Param(u32),
    Node(usize),
}

/// The recursive tree after fmadd contraction, before ordering.
enum Tree {
    Param(u32),
    /// A named double local's shared node (by locals-list index).
    LocalRef(usize),
    Const(u64),
    /// A constant-index read of a static const double TABLE: one shared
    /// lis/addi ADDR16 base, reads as lfd's at fixed offsets.
    TableConst(i16),
    /// factor_left * factor_right + addend (fp_contract).
    Madd {
        factor_left: Box<Tree>,
        factor_right: Box<Tree>,
        addend: Box<Tree>,
    },
    /// base - factor_left * factor_right (fp_contract: fnmsub d,a,c,b = b - a*c).
    Fnmsub {
        factor_left: Box<Tree>,
        factor_right: Box<Tree>,
        base: Box<Tree>,
    },
    /// factor_left * factor_right - subtrahend (fp_contract: fmsub d,a,c,b = a*c - b).
    Fmsub {
        factor_left: Box<Tree>,
        factor_right: Box<Tree>,
        subtrahend: Box<Tree>,
    },
    Mul {
        left: Box<Tree>,
        right: Box<Tree>,
    },
    /// A plain unfused add (measured: a pooled constant + a non-mul value,
    /// the constant in the A slot — fadd f1,f0,f1).
    Fadd {
        left: Box<Tree>,
        right: Box<Tree>,
    },
    /// A plain unfused subtract (neither side a product): source slots
    /// (fsub f1,f1,f0 — the k_sin else-tail's outer x-minus).
    Fsub {
        left: Box<Tree>,
        right: Box<Tree>,
    },
}

fn tree_contains_contracted_operation(tree: &Tree) -> bool {
    match tree {
        Tree::Madd { .. } | Tree::Fnmsub { .. } | Tree::Fmsub { .. } => true,
        Tree::Mul { left, right }
        | Tree::Fadd { left, right }
        | Tree::Fsub { left, right } => {
            tree_contains_contracted_operation(left) || tree_contains_contracted_operation(right)
        }
        Tree::Param(_)
        | Tree::LocalRef(_)
        | Tree::Const(_)
        | Tree::TableConst(_) => false,
    }
}

/// One emitted node, operands in the final instruction slots (the measured
/// convention: a CONSTANT factor takes the A slot, otherwise source order).
enum FloatOp {
    Const(u64),
    /// x reloaded from its frame slot (no relocation).
    FrameLoad(i16),
    /// A coefficient-table read: lfd at the fixed offset off the table base.
    TableLoad(i16),
    /// A conditionally-defined local (the diamond's qx): allocates like a
    /// window-top tier local but emits NOTHING — the diamond arms already
    /// loaded it.
    Phantom,
    Madd {
        a: Operand,
        c: Operand,
        b: Operand,
    },
    Fnmsub {
        a: Operand,
        c: Operand,
        b: Operand,
    },
    Fmsub {
        a: Operand,
        c: Operand,
        b: Operand,
    },
    Mul {
        a: Operand,
        c: Operand,
    },
    Add {
        a: Operand,
        b: Operand,
    },
    Sub {
        a: Operand,
        b: Operand,
    },
    /// The dual arm's liveness sink (emits nothing).
    Sink,
}

const LOAD_LATENCY: u32 = 2;
const FLOAT_ARITH_LATENCY: u32 = 3;
/// Double fmul GATES its consumers at 4 cycles while weighing 3 for
/// priority (measured: the z=x*x chains; see the linearize fixtures).
const FLOAT_MUL_GATE: u32 = 4;

fn float_def(instruction: &Instruction) -> Option<u8> {
    match instruction {
        Instruction::LoadFloatDouble { d, .. }
        | Instruction::FloatMultiplyDouble { d, .. }
        | Instruction::FloatAddDouble { d, .. }
        | Instruction::FloatSubtractDouble { d, .. }
        | Instruction::FloatMultiplyAddDouble { d, .. }
        | Instruction::FloatMultiplySubtractDouble { d, .. }
        | Instruction::FloatNegativeMultiplySubtractDouble { d, .. } => Some(*d),
        _ => None,
    }
}

fn float_reads_register(instruction: &Instruction, register: u8) -> bool {
    match instruction {
        Instruction::FloatMultiplyDouble { a, c, .. } => *a == register || *c == register,
        Instruction::FloatAddDouble { a, b, .. }
        | Instruction::FloatSubtractDouble { a, b, .. } => *a == register || *b == register,
        Instruction::FloatMultiplyAddDouble { a, c, b, .. }
        | Instruction::FloatMultiplySubtractDouble { a, c, b, .. }
        | Instruction::FloatNegativeMultiplySubtractDouble { a, c, b, .. } => {
            *a == register || *c == register || *b == register
        }
        _ => false,
    }
}

fn build_tree(
    expression: &Expression,
    params: &[(String, u32)],
    locals: &[(String, usize)],
    seen_literals: &mut Vec<u64>,
) -> Option<Tree> {
    build_tree_with_tables(
        expression,
        params,
        locals,
        seen_literals,
        &mut TableContext::default(),
    )
}

/// The one coefficient table a claim may read (a second table defers), with
/// the offsets seen — the arm materializes ONE lis/addi base.
#[derive(Default)]
pub(super) struct TableContext {
    pub(super) name: Option<String>,
    pub(super) tables: std::collections::HashSet<String>,
}

fn build_tree_with_tables(
    expression: &Expression,
    params: &[(String, u32)],
    locals: &[(String, usize)],
    seen_literals: &mut Vec<u64>,
    table: &mut TableContext,
) -> Option<Tree> {
    if let Expression::Index { base, index } = expression {
        let Expression::Variable(name) = base.as_ref() else {
            return None;
        };
        if !table.tables.contains(name) {
            return None;
        }
        let Expression::IntegerLiteral(element) = index.as_ref() else {
            return None;
        };
        let offset = i16::try_from(element.checked_mul(8)?).ok()?;
        match &table.name {
            Some(existing) if existing != name => return None,
            _ => table.name = Some(name.clone()),
        }
        return Some(Tree::TableConst(offset));
    }
    match expression {
        Expression::Variable(name) => {
            if let Some(&(_, index)) = locals.iter().find(|(local, _)| local == name) {
                return Some(Tree::LocalRef(index));
            }
            let &(_, value) = params.iter().find(|(parameter, _)| parameter == name)?;
            Some(Tree::Param(value))
        }
        Expression::FloatLiteral(value) => {
            let bits = value.to_bits();
            // A duplicated literal's pool/reuse behavior is uncaptured.
            if seen_literals.contains(&bits) {
                return None;
            }
            seen_literals.push(bits);
            Some(Tree::Const(bits))
        }
        Expression::Binary {
            operator: BinaryOperator::Add,
            left,
            right,
        } => {
            // fp_contract: fuse a multiplication side into fmadd. When BOTH
            // sides multiply, the LEFT fuses and the right evaluates as an
            // fmul addend (measured: s1_s2 and s1_s2_shallow both emit
            // fmadd f1,f1,<s1 chain>,<fmul of s2>).
            let left_mul = matches!(
                left.as_ref(),
                Expression::Binary {
                    operator: BinaryOperator::Multiply,
                    ..
                }
            );
            let right_mul = matches!(
                right.as_ref(),
                Expression::Binary {
                    operator: BinaryOperator::Multiply,
                    ..
                }
            );
            match (left_mul, right_mul) {
                (false, false) => {
                    // A plain fadd: one pooled-constant side (canonical A),
                    // the other a claimable non-mul value.
                    let (constant, other) = if matches!(left.as_ref(), Expression::FloatLiteral(_))
                    {
                        (left, right)
                    } else if matches!(right.as_ref(), Expression::FloatLiteral(_)) {
                        (right, left)
                    } else {
                        return None;
                    };
                    let constant =
                        build_tree_with_tables(constant, params, locals, seen_literals, table)?;
                    let other =
                        build_tree_with_tables(other, params, locals, seen_literals, table)?;
                    Some(Tree::Fadd {
                        left: Box::new(constant),
                        right: Box::new(other),
                    })
                }
                (true, _) => {
                    let Expression::Binary {
                        left: x, right: y, ..
                    } = left.as_ref()
                    else {
                        unreachable!()
                    };
                    make_madd(x, y, right, params, locals, seen_literals, table)
                }
                (false, true) => {
                    let Expression::Binary {
                        left: x, right: y, ..
                    } = right.as_ref()
                    else {
                        unreachable!()
                    };
                    make_madd(x, y, left, params, locals, seen_literals, table)
                }
            }
        }
        Expression::Binary {
            operator: BinaryOperator::Subtract,
            left,
            right,
        } => {
            // fp_contract: `b - x*y` contracts to fnmsub, `x*y - b` to
            // fmsub (measured: the root-slot order + dying-door rules fit
            // the simple, deep, and wmul fmsub roots). A constant fmsub
            // FACTOR is uncaptured — deferred inside the branch.
            if let Expression::Binary {
                operator: BinaryOperator::Multiply,
                left: x,
                right: y,
            } = left.as_ref()
            {
                // ONE pooled-constant factor takes the A slot (measured:
                // fmsub f0,f4,f2,f0 = 0.5*y - v*r); both fold — defer.
                if matches!(x.as_ref(), Expression::FloatLiteral(_))
                    && matches!(y.as_ref(), Expression::FloatLiteral(_))
                {
                    return None;
                }
                let factor_left = build_tree_with_tables(x, params, locals, seen_literals, table)?;
                let factor_right = build_tree_with_tables(y, params, locals, seen_literals, table)?;
                let subtrahend =
                    build_tree_with_tables(right, params, locals, seen_literals, table)?;
                return Some(Tree::Fmsub {
                    factor_left: Box::new(factor_left),
                    factor_right: Box::new(factor_right),
                    subtrahend: Box::new(subtrahend),
                });
            }
            let Expression::Binary {
                operator: BinaryOperator::Multiply,
                left: x,
                right: y,
            } = right.as_ref()
            else {
                // Neither side a product: the plain unfused FSUB in source
                // slots (the deep form runs the emission-order regime).
                let minuend = build_tree_with_tables(left, params, locals, seen_literals, table)?;
                let subtrahend =
                    build_tree_with_tables(right, params, locals, seen_literals, table)?;
                return Some(Tree::Fsub {
                    left: Box::new(minuend),
                    right: Box::new(subtrahend),
                });
            };
            let both_const = matches!(x.as_ref(), Expression::FloatLiteral(_))
                && matches!(y.as_ref(), Expression::FloatLiteral(_));
            if both_const {
                return None;
            }
            let base = build_tree_with_tables(left, params, locals, seen_literals, table)?;
            let factor_left = build_tree_with_tables(x, params, locals, seen_literals, table)?;
            let factor_right = build_tree_with_tables(y, params, locals, seen_literals, table)?;
            Some(Tree::Fnmsub {
                factor_left: Box::new(factor_left),
                factor_right: Box::new(factor_right),
                base: Box::new(base),
            })
        }
        Expression::Binary {
            operator: BinaryOperator::Multiply,
            left,
            right,
        } => {
            // ONE pooled-constant fmul factor is measured (fmul f1,f0,f1 —
            // the constant in A); both constant folds and stays deferred.
            let left_const = matches!(left.as_ref(), Expression::FloatLiteral(_));
            let right_const = matches!(right.as_ref(), Expression::FloatLiteral(_));
            if left_const && right_const {
                return None;
            }
            if left_const || right_const {
                let (constant, other) = if left_const {
                    (left, right)
                } else {
                    (right, left)
                };
                if matches!(
                    other.as_ref(),
                    Expression::Binary {
                        operator: BinaryOperator::Multiply,
                        ..
                    }
                ) {
                    return None;
                }
                let constant =
                    build_tree_with_tables(constant, params, locals, seen_literals, table)?;
                let other = build_tree_with_tables(other, params, locals, seen_literals, table)?;
                return Some(Tree::Mul {
                    left: Box::new(constant),
                    right: Box::new(other),
                });
            }
            let is_mul = |side: &Expression| {
                matches!(
                    side,
                    Expression::Binary {
                        operator: BinaryOperator::Multiply,
                        ..
                    }
                )
            };
            match (is_mul(left), is_mul(right)) {
                (false, false) => {
                    let left = build_tree_with_tables(left, params, locals, seen_literals, table)?;
                    let right =
                        build_tree_with_tables(right, params, locals, seen_literals, table)?;
                    Some(Tree::Mul {
                        left: Box::new(left),
                        right: Box::new(right),
                    })
                }
                // The SHALLOW mul-of-mul (measured both source orders emit
                // identically): one factor a leaf param product, the other a
                // single contracted madd — canonicalized chain-left. The
                // deeper chain breaks the register model (the cross-chain
                // product spans the window; float_mul_of_mul_deep) — defer.
                (true, false) | (false, true) => {
                    let (product, chain) = if is_mul(left) {
                        (left, right)
                    } else {
                        (left, right)
                    };
                    let (product, chain) = if is_mul(product.as_ref()) {
                        (product, chain)
                    } else {
                        (chain, product)
                    };
                    let Expression::Binary {
                        operator: BinaryOperator::Multiply,
                        left: x,
                        right: y,
                    } = product.as_ref()
                    else {
                        return None;
                    };
                    if !matches!(x.as_ref(), Expression::Variable(_))
                        || !matches!(y.as_ref(), Expression::Variable(_))
                    {
                        return None;
                    }
                    let chain_tree =
                        build_tree_with_tables(chain, params, locals, seen_literals, table)?;
                    if !matches!(chain_tree, Tree::Madd { .. } | Tree::Fnmsub { .. }) {
                        return None;
                    }
                    let x = build_tree_with_tables(x, params, locals, seen_literals, table)?;
                    let y = build_tree_with_tables(y, params, locals, seen_literals, table)?;
                    let product_tree = Tree::Mul {
                        left: Box::new(x),
                        right: Box::new(y),
                    };
                    Some(Tree::Mul {
                        left: Box::new(chain_tree),
                        right: Box::new(product_tree),
                    })
                }
                (true, true) => None,
            }
        }
        _ => None,
    }
}

/// Build an fmadd from `x*y + addend`, deferring constant-foldable pairs.
fn make_madd(
    x: &Expression,
    y: &Expression,
    addend: &Expression,
    params: &[(String, u32)],
    locals: &[(String, usize)],
    seen_literals: &mut Vec<u64>,
    table: &mut TableContext,
) -> Option<Tree> {
    let both_const =
        matches!(x, Expression::FloatLiteral(_)) && matches!(y, Expression::FloatLiteral(_));
    if both_const {
        return None;
    }
    let factor_left = build_tree_with_tables(x, params, locals, seen_literals, table)?;
    let factor_right = build_tree_with_tables(y, params, locals, seen_literals, table)?;
    let addend = build_tree_with_tables(addend, params, locals, seen_literals, table)?;
    Some(Tree::Madd {
        factor_left: Box::new(factor_left),
        factor_right: Box::new(factor_right),
        addend: Box::new(addend),
    })
}

/// Count arith nodes in a subtree (the shallow mul-of-mul gate).
fn count_arith(tree: &Tree) -> usize {
    let mut refs: Vec<(&Tree, u32)> = Vec::new();
    collect_arith(tree, 0, &mut refs);
    refs.len()
}

/// Collect double literals in source (left-to-right) order — the measured
/// .sdata2 pool order.
fn collect_literals(expression: &Expression, into: &mut Vec<u64>) {
    match expression {
        Expression::FloatLiteral(value) => into.push(value.to_bits()),
        Expression::Binary { left, right, .. } => {
            collect_literals(left, into);
            collect_literals(right, into);
        }
        _ => {}
    }
}

/// Collect arith nodes with their tree level (root 0), factor subtrees
/// before the addend so the stable level sort keeps factor-side-first ties —
/// the measured evaluation order.
fn collect_arith<'tree>(tree: &'tree Tree, level: u32, into: &mut Vec<(&'tree Tree, u32)>) {
    match tree {
        Tree::Madd {
            factor_left,
            factor_right,
            addend,
        }
        | Tree::Fnmsub {
            factor_left,
            factor_right,
            base: addend,
        }
        | Tree::Fmsub {
            factor_left,
            factor_right,
            subtrahend: addend,
        } => {
            into.push((tree, level));
            collect_arith(factor_left, level + 1, into);
            collect_arith(factor_right, level + 1, into);
            collect_arith(addend, level + 1, into);
        }
        Tree::Mul { left, right } | Tree::Fadd { left, right } | Tree::Fsub { left, right } => {
            into.push((tree, level));
            collect_arith(left, level + 1, into);
            collect_arith(right, level + 1, into);
        }
        Tree::Param(_) | Tree::LocalRef(_) | Tree::Const(_) | Tree::TableConst(_) => {}
    }
}

mod core;
mod diamond_dual;
mod punned;
