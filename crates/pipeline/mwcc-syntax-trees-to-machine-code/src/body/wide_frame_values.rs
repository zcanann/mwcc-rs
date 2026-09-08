//! Lower retained 64-bit local values to explicit big-endian frame images.
//!
//! Scalar control flow and frame allocation stay with the ordinary backend.
//! A wide call is captured by one typed store; pair arithmetic is expanded only
//! for side-effect-free operands, with both result words captured before stores.

use mwcc_syntax_trees::{
    ArmBody, BinaryOperator as B, Expression as E, Function, LocalDeclaration, Pointee,
    Statement as S, Type,
};
use std::collections::{HashMap, HashSet};

fn optional<T, U>(value: Option<&T>, mut f: impl FnMut(&T) -> Option<U>) -> Option<Option<U>> {
    Some(match value {
        Some(value) => Some(f(value)?),
        None => None,
    })
}

fn wide(ty: Type) -> bool {
    matches!(ty, Type::LongLong | Type::UnsignedLongLong)
}
fn var(name: &str) -> E {
    E::Variable(name.into())
}
fn bin(operator: B, left: E, right: E) -> E {
    E::Binary {
        operator,
        left: Box::new(left),
        right: Box::new(right),
    }
}
fn cast(ty: Type, operand: E) -> E {
    E::Cast {
        target_type: ty,
        operand: Box::new(operand),
    }
}
fn word(value: u32) -> E {
    cast(Type::UnsignedInt, E::IntegerLiteral(value as i64))
}
fn addr(value: E) -> E {
    E::AddressOf {
        operand: Box::new(value),
    }
}
fn field(base: E, offset: u32) -> E {
    E::Member {
        base: Box::new(base),
        offset,
        member_type: Type::UnsignedInt,
        index_stride: None,
    }
}

struct Lowering<'a> {
    types: HashMap<String, Type>,
    calls: &'a HashMap<String, Type>,
    volatile: &'a HashSet<String>,
    frames: HashSet<String>,
    bindings: HashSet<String>,
    occupied: HashSet<String>,
    temporaries: Vec<LocalDeclaration>,
    pending: Vec<S>,
}

pub(crate) fn materialize(
    function: &Function,
    globals: &HashMap<String, Type>,
    volatile: &HashSet<String>,
    calls: &HashMap<String, Type>,
) -> Option<Function> {
    if wide(function.return_type) || function.parameters.iter().any(|p| wide(p.parameter_type)) {
        return None;
    }
    let frames: HashSet<String> = function
        .locals
        .iter()
        .filter(|l| wide(l.declared_type))
        .map(|l| l.name.clone())
        .collect();
    if frames.is_empty()
        || function.locals.iter().any(|l| {
            frames.contains(&l.name) && (l.is_static || l.is_volatile || l.array_length.is_some())
        })
    {
        return None;
    }
    // Entry initializers are lowered together, in declaration order. Static
    // storage initialization belongs to the object pipeline, not this frame lane.
    if function.locals.iter().any(|l| l.is_static) {
        return None;
    }
    let mut types = globals.clone();
    types.extend(
        function
            .parameters
            .iter()
            .map(|p| (p.name.clone(), p.parameter_type)),
    );
    types.extend(
        function
            .locals
            .iter()
            .map(|l| (l.name.clone(), l.declared_type)),
    );
    let occupied = types.keys().cloned().chain(calls.keys().cloned()).collect();
    let bindings = function
        .locals
        .iter()
        .map(|l| l.name.clone())
        .chain(function.parameters.iter().map(|p| p.name.clone()))
        .collect();
    let mut lowering = Lowering {
        types,
        calls,
        volatile,
        frames,
        bindings,
        occupied,
        temporaries: Vec::new(),
        pending: Vec::new(),
    };
    let mut rewritten = function.clone();
    let mut statements = Vec::new();
    for local in &mut rewritten.locals {
        if lowering.frames.contains(&local.name) {
            local.declared_type = Type::Struct { size: 8, align: 8 };
        }
        if let Some(initializer) = local.initializer.take() {
            if lowering.frames.contains(&local.name) {
                let assigned = lowering.assign(&var(&local.name), &initializer)?;
                statements.append(&mut lowering.pending);
                statements.extend(assigned);
            } else {
                if local.array_length.is_some()
                    || matches!(local.declared_type, Type::Struct { .. })
                {
                    return None;
                }
                let value = lowering.scalar(&initializer)?;
                statements.append(&mut lowering.pending);
                statements.push(S::Assign {
                    name: local.name.clone(),
                    value,
                });
            }
        }
    }
    let mut body = function.statements.clone();
    body.extend(rewritten.guards.drain(..).map(|guard| S::If {
        condition: guard.condition,
        then_body: vec![S::Return(Some(guard.value))],
        else_body: Vec::new(),
    }));
    statements.extend(lowering.statements(&body)?);
    rewritten.return_expression =
        optional(function.return_expression.as_ref(), |e| lowering.scalar(e))?;
    statements.append(&mut lowering.pending);
    rewritten.statements = statements;
    rewritten.locals.extend(lowering.temporaries);
    Some(rewritten)
}

/// Nested loops own their continues; switches and conditionals do not.
fn continues_current_loop(statements: &[S]) -> bool {
    statements.iter().any(|statement| match statement {
        S::Continue => true,
        S::If {
            then_body,
            else_body,
            ..
        } => continues_current_loop(then_body) || continues_current_loop(else_body),
        S::Switch { arms, default, .. } => {
            arms.iter().map(|arm| &arm.body).chain(default.iter()).any(
                |body| matches!(body, ArmBody::Statements(body) if continues_current_loop(body)),
            )
        }
        _ => false,
    })
}

impl Lowering<'_> {
    fn ty(&self, e: &E) -> Option<Type> {
        match e {
            E::IntegerLiteral(value) => Some(if i32::try_from(*value).is_ok() {
                Type::Int
            } else if u32::try_from(*value).is_ok() {
                Type::UnsignedInt
            } else {
                Type::LongLong
            }),
            E::Variable(name) => self.types.get(name).copied(),
            E::Call { name, .. } => self.calls.get(name).copied(),
            E::Cast { target_type, .. } => Some(*target_type),
            E::Binary {
                operator:
                    B::Add
                    | B::Subtract
                    | B::Multiply
                    | B::Divide
                    | B::Modulo
                    | B::BitAnd
                    | B::BitOr
                    | B::BitXor,
                left,
                right,
            } => {
                let a = self.ty(left)?;
                let b = self.ty(right)?;
                if ![a, b].into_iter().all(|ty| {
                    matches!(
                        ty,
                        Type::Int
                            | Type::UnsignedInt
                            | Type::Char
                            | Type::UnsignedChar
                            | Type::Short
                            | Type::UnsignedShort
                            | Type::LongLong
                            | Type::UnsignedLongLong
                    )
                }) {
                    return None;
                }
                Some(
                    if a == Type::UnsignedLongLong || b == Type::UnsignedLongLong {
                        Type::UnsignedLongLong
                    } else if a == Type::LongLong || b == Type::LongLong {
                        Type::LongLong
                    } else if a == Type::UnsignedInt || b == Type::UnsignedInt {
                        Type::UnsignedInt
                    } else {
                        Type::Int
                    },
                )
            }
            E::Dereference { pointer } => match self.ty(pointer)? {
                Type::Pointer(p) => Some(p.element()),
                _ => None,
            },
            E::Member { member_type, .. } => Some(*member_type),
            _ => None,
        }
    }
    fn mentions_wide(&self, e: &E) -> bool {
        let mut result = false;
        crate::body::callee_saved::structured_expression_visit::visit_expression(e, &mut |e| {
            result |= self.ty(e).is_some_and(wide);
        });
        result
    }
    fn pair(&mut self, e: &E) -> Option<(E, E)> {
        if !matches!(e, E::IntegerLiteral(_)) && !self.mentions_wide(e) {
            let ty = self.ty(e)?;
            if !matches!(
                ty,
                Type::Int
                    | Type::UnsignedInt
                    | Type::Char
                    | Type::UnsignedChar
                    | Type::Short
                    | Type::UnsignedShort
            ) || crate::analysis::expression_has_side_effect(e)
            {
                return None;
            }
            // Apply the word operation before widening: 32-bit arithmetic
            // still wraps at its own width when compared with a wide value.
            let low = self.capture(cast(Type::UnsignedInt, e.clone()));
            let high = if matches!(
                ty,
                Type::UnsignedInt | Type::UnsignedChar | Type::UnsignedShort
            ) {
                word(0)
            } else {
                self.capture(cast(
                    Type::UnsignedInt,
                    bin(B::ShiftRight, cast(Type::Int, low.clone()), word(31)),
                ))
            };
            return Some((high, low));
        }
        match e {
            E::IntegerLiteral(v) => Some((word((*v as u64 >> 32) as u32), word(*v as u32))),
            E::Variable(name) if self.ty(e).is_some_and(wide) && !self.volatile.contains(name) => {
                let base = addr(var(name));
                Some((
                    self.capture(field(base.clone(), 0)),
                    self.capture(field(base, 4)),
                ))
            }
            E::Cast {
                target_type,
                operand,
            } if wide(*target_type) => self.pair(operand),
            E::Binary {
                operator: B::Subtract,
                left,
                right,
            } => {
                let (ah, al) = self.pair(left)?;
                let (bh, bl) = self.pair(right)?;
                let borrow = self.capture(bin(B::Less, al.clone(), bl.clone()));
                let high_difference = self.capture(bin(B::Subtract, ah, bh));
                Some((
                    self.capture(bin(B::Subtract, high_difference, borrow)),
                    self.capture(bin(B::Subtract, al, bl)),
                ))
            }
            E::Binary {
                operator: B::Add,
                left,
                right,
            } => {
                let (ah, al) = self.pair(left)?;
                let (bh, bl) = self.pair(right)?;
                let low = self.capture(bin(B::Add, al.clone(), bl));
                let carry = self.capture(bin(B::Less, low.clone(), al));
                let high_sum = self.capture(bin(B::Add, ah, bh));
                Some((self.capture(bin(B::Add, high_sum, carry)), low))
            }
            _ => None,
        }
    }
    fn compare(&mut self, op: B, left: &E, right: &E) -> Option<E> {
        let unsigned = self.ty(left) == Some(Type::UnsignedLongLong)
            || self.ty(right) == Some(Type::UnsignedLongLong);
        let (ah, al) = self.pair(left)?;
        let (bh, bl) = self.pair(right)?;
        let (ah, bh) = if unsigned {
            (ah, bh)
        } else {
            (cast(Type::Int, ah), cast(Type::Int, bh))
        };
        let equal_high = bin(B::Equal, ah.clone(), bh.clone());
        Some(match op {
            B::Equal => bin(B::LogicalAnd, equal_high, bin(B::Equal, al, bl)),
            B::NotEqual => bin(
                B::LogicalOr,
                bin(B::NotEqual, ah, bh),
                bin(B::NotEqual, al, bl),
            ),
            B::Less | B::LessEqual => bin(
                B::LogicalOr,
                bin(B::Less, ah, bh),
                bin(B::LogicalAnd, equal_high, bin(op, al, bl)),
            ),
            B::Greater | B::GreaterEqual => bin(
                B::LogicalOr,
                bin(B::Greater, ah, bh),
                bin(B::LogicalAnd, equal_high, bin(op, al, bl)),
            ),
            _ => return None,
        })
    }
    fn scalar(&mut self, e: &E) -> Option<E> {
        if !self.mentions_wide(e) {
            return Some(e.clone());
        }
        if crate::analysis::expression_has_side_effect(e) {
            return None;
        }
        match e {
            E::Binary {
                operator,
                left,
                right,
            } if matches!(
                operator,
                B::Equal | B::NotEqual | B::Less | B::LessEqual | B::Greater | B::GreaterEqual
            ) && (self.ty(left).is_some_and(wide) || self.ty(right).is_some_and(wide)) =>
            {
                self.compare(*operator, left, right)
            }

            E::Cast {
                target_type,
                operand,
            } if matches!(
                target_type,
                Type::Int
                    | Type::UnsignedInt
                    | Type::Short
                    | Type::UnsignedShort
                    | Type::Char
                    | Type::UnsignedChar
            ) =>
            {
                Some(cast(*target_type, self.pair(operand)?.1))
            }
            _ => None,
        }
    }
    fn capture(&mut self, value: E) -> E {
        let name = self.temp();
        self.pending.push(S::Assign {
            name: name.clone(),
            value,
        });
        var(&name)
    }
    fn temp(&mut self) -> String {
        let name = (0..)
            .map(|i| format!("__mwcc_pair_word_{i}"))
            .find(|n| !self.occupied.contains(n))
            .unwrap();
        self.occupied.insert(name.clone());
        self.temporaries.push(LocalDeclaration {
            name: name.clone(),
            declared_type: Type::UnsignedInt,
            initializer: None,
            is_volatile: false,
            array_length: None,
            is_static: false,
            data_bytes: None,
            data_relocations: Vec::new(),
            is_const: false,
            attribute_alignment: None,
            row_bytes: None,
        });
        name
    }
    fn assign(&mut self, target: &E, value: &E) -> Option<Vec<S>> {
        let E::Variable(name) = target else {
            return None;
        };
        if let E::Call { .. } = value {
            if !self.ty(value).is_some_and(wide) {
                return None;
            }
            let target = if self.frames.contains(name) {
                E::Dereference {
                    pointer: Box::new(cast(
                        Type::Pointer(Pointee::UnsignedLongLong),
                        addr(target.clone()),
                    )),
                }
            } else {
                target.clone()
            };
            return Some(vec![S::Store {
                target,
                value: value.clone(),
            }]);
        }
        let (high, low) = self.pair(value)?;
        let high_name = self.temp();
        let low_name = self.temp();
        let base = addr(target.clone());
        Some(vec![
            S::Assign {
                name: high_name.clone(),
                value: high,
            },
            S::Assign {
                name: low_name.clone(),
                value: low,
            },
            S::Store {
                target: field(base.clone(), 4),
                value: var(&low_name),
            },
            S::Store {
                target: field(base, 0),
                value: var(&high_name),
            },
        ])
    }
    fn statements(&mut self, statements: &[S]) -> Option<Vec<S>> {
        let mut out = Vec::new();
        for statement in statements {
            let lowered = match statement {
                S::Assign { name, value } if self.types.get(name).copied().is_some_and(wide) => {
                    let result = self.assign(&var(name), value)?;
                    out.append(&mut self.pending);
                    out.extend(result);
                    continue;
                }
                S::Store { target, value } if self.ty(target).is_some_and(wide) => {
                    let result = self.assign(target, value)?;
                    out.append(&mut self.pending);
                    out.extend(result);
                    continue;
                }
                S::Assign { name, value } => {
                    let value = self.scalar(value)?;
                    if self.types.contains_key(name) && !self.bindings.contains(name) {
                        S::Store {
                            target: var(name),
                            value,
                        }
                    } else {
                        S::Assign {
                            name: name.clone(),
                            value,
                        }
                    }
                }
                S::Store { target, value } => S::Store {
                    target: self.scalar(target)?,
                    value: self.scalar(value)?,
                },
                S::Expression(e) => S::Expression(self.scalar(e)?),
                S::Return(value) => S::Return(optional(value.as_ref(), |v| self.scalar(v))?),
                S::If {
                    condition,
                    then_body,
                    else_body,
                } => {
                    let condition = self.scalar(condition)?;
                    out.append(&mut self.pending);
                    S::If {
                        condition,
                        then_body: self.statements(then_body)?,
                        else_body: self.statements(else_body)?,
                    }
                }
                S::Loop {
                    kind,
                    initializer,
                    condition,
                    step,
                    body,
                } => {
                    if initializer
                        .iter()
                        .chain(step)
                        .any(|e| self.mentions_wide(e))
                    {
                        return None;
                    }
                    let mut lowered_body = self.statements(body)?;
                    let condition = if condition.as_ref().is_some_and(|e| self.mentions_wide(e)) {
                        // Every post-test iteration computes both words after
                        // its body. A continue targeting this test would need
                        // its own prelude; retain that diagnostic until modeled.
                        if *kind != mwcc_syntax_trees::LoopKind::DoWhile
                            || initializer.is_some()
                            || step.is_some()
                            || continues_current_loop(body)
                        {
                            return None;
                        }
                        let condition = self.scalar(condition.as_ref().unwrap())?;
                        lowered_body.append(&mut self.pending);
                        Some(condition)
                    } else {
                        condition.clone()
                    };
                    S::Loop {
                        kind: *kind,
                        initializer: initializer.clone(),
                        condition,
                        step: step.clone(),
                        body: lowered_body,
                    }
                }
                S::Switch {
                    scrutinee,
                    arms,
                    default,
                } => {
                    let scrutinee = self.scalar(scrutinee)?;
                    out.append(&mut self.pending);
                    let mut arms = arms.clone();
                    for arm in &mut arms {
                        arm.body = self.arm(&arm.body)?;
                    }
                    S::Switch {
                        scrutinee,
                        arms,
                        default: optional(default.as_ref(), |b| self.arm(b))?,
                    }
                }
                other => other.clone(),
            };
            out.append(&mut self.pending);
            out.push(lowered);
        }
        Some(out)
    }

    fn arm(&mut self, body: &ArmBody) -> Option<ArmBody> {
        Some(match body {
            ArmBody::Statements(body) => ArmBody::Statements(self.statements(body)?),
            ArmBody::Return(e) => ArmBody::Return(self.scalar(e)?),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_lowering<'a>(
        calls: &'a HashMap<String, Type>,
        volatile: &'a HashSet<String>,
    ) -> Lowering<'a> {
        Lowering {
            types: HashMap::from([
                ("stamp".into(), Type::LongLong),
                ("limit".into(), Type::UnsignedInt),
                ("real".into(), Type::Float),
            ]),
            calls,
            volatile,
            frames: HashSet::from(["stamp".into()]),
            bindings: HashSet::from(["stamp".into(), "limit".into()]),
            occupied: HashSet::from(["stamp".into(), "limit".into()]),
            temporaries: Vec::new(),
            pending: Vec::new(),
        }
    }

    #[test]
    fn pair_test_preludes_stay_after_each_posttest_body() {
        let calls = HashMap::from([("tick".into(), Type::LongLong)]);
        let volatile = HashSet::new();
        let mut lowering = test_lowering(&calls, &volatile);
        let source = S::Loop {
            kind: mwcc_syntax_trees::LoopKind::DoWhile,
            initializer: None,
            condition: Some(bin(
                B::Less,
                var("stamp"),
                bin(B::Divide, var("limit"), word(4)),
            )),
            step: None,
            body: vec![S::Assign {
                name: "stamp".into(),
                value: E::Call {
                    name: "tick".into(),
                    arguments: Vec::new(),
                },
            }],
        };
        let lowered = lowering.statements(&[source.clone()]).unwrap();
        let [S::Loop {
            body,
            condition: Some(condition),
            ..
        }] = lowered.as_slice()
        else {
            panic!("one retained loop");
        };
        assert!(
            matches!(body.first(), Some(S::Store { value: E::Call { name, .. }, .. }) if name == "tick")
        );
        assert!(body.len() > 1);
        assert!(!lowering.mentions_wide(condition));
        assert!(lowering.pending.is_empty());
        let mut continued = source;
        let S::Loop { body, .. } = &mut continued else {
            unreachable!()
        };
        body.push(S::Continue);
        assert!(test_lowering(&calls, &volatile)
            .statements(&[continued])
            .is_none());
        assert!(!continues_current_loop(&[S::Loop {
            kind: mwcc_syntax_trees::LoopKind::While,
            initializer: None,
            condition: Some(word(1)),
            step: None,
            body: vec![S::Continue],
        }]));
    }

    #[test]
    fn word_arithmetic_wraps_before_its_wide_promotion() {
        let calls = HashMap::new();
        let volatile = HashSet::new();
        let mut lowering = test_lowering(&calls, &volatile);
        let value = bin(B::Add, var("limit"), word(1));
        let (high, low) = lowering.pair(&value).unwrap();
        assert!(crate::analysis::structurally_equal(&high, &word(0)));
        let [S::Assign { name, value: saved }] = lowering.pending.as_slice() else {
            panic!("one word evaluation");
        };
        assert!(crate::analysis::structurally_equal(
            saved,
            &cast(Type::UnsignedInt, value)
        ));
        assert!(crate::analysis::structurally_equal(&low, &var(name)));
        assert!(lowering
            .pair(&bin(B::Add, var("limit"), var("real")))
            .is_none());
    }

    #[test]
    fn does_not_hoist_pair_reads_out_of_short_circuit_or_call_expressions() {
        let calls = HashMap::from([("poll".into(), Type::Int)]);
        let volatile = HashSet::new();
        let mut lowering = Lowering {
            types: HashMap::from([("stamp".into(), Type::LongLong)]),
            calls: &calls,
            volatile: &volatile,
            frames: HashSet::from(["stamp".into()]),
            bindings: HashSet::from(["stamp".into()]),
            occupied: HashSet::from(["stamp".into()]),
            temporaries: Vec::new(),
            pending: Vec::new(),
        };
        let compare = bin(B::Less, var("stamp"), E::IntegerLiteral(10));
        for first in [
            var("enabled"),
            E::Call {
                name: "poll".into(),
                arguments: Vec::new(),
            },
        ] {
            assert!(lowering
                .scalar(&bin(B::LogicalAnd, first, compare.clone()))
                .is_none());
            assert!(lowering.pending.is_empty());
        }
    }
}
