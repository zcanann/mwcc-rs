//! Build the typed IRO representation of a function from its syntax tree,
//! then run the IRO passes (see [`passes`]).
//!
//! The builder resolves every expression's type, makes pointer arithmetic
//! explicit (indices are scaled to bytes), turns member, index and
//! dereference accesses into loads and stores through `base + index +
//! offset`, and turns guards and the final return into statements. It
//! rejects (with a diagnostic) anything the PCode pipeline does not model.

pub mod passes;
mod strength;
mod unroll;

use std::collections::HashMap;

use mwcc_core::{Compilation, Diagnostic};
use mwcc_iro::{
    element_size, is_float, is_general_word, is_narrow, is_unsigned, is_value_type, is_wide, pointee_type, pointer_to, promote,
    BinaryOp, Expr, ExprKind,
    Function, Idiom, IntrinsicOp, Place, Stmt, Type, UnaryOp, Unit, VarId, Variable, VariableKind,
};
use mwcc_syntax_trees as ast;
use mwcc_syntax_trees::{BinaryOperator, Expression, Statement, UnaryOperator};

const ARGUMENT_REGISTERS: usize = 8;

pub fn unsupported(what: impl Into<String>) -> Diagnostic {
    Diagnostic::error(format!("PCode lowering: {} (not yet supported)", what.into()))
}

/// The built function, plus facts the lowering needs about its source form.
pub struct Built {
    pub function: Function,
    /// The source returns from more than its final expression (guards,
    /// `return` statements, a conditional final value): every return goes
    /// through one return variable, as MWCC's single return point does.
    pub returns_through_variable: bool,
}

/// Build the IR for `function` and run the IRO passes.
pub fn build(function: &ast::Function, unit: &Unit<'_>) -> Compilation<Built> {
    passes::FLOAT_NEGATION_ALGEBRA.with(|flag| flag.set(unit.cancels_float_negations));
    passes::UNSIGNED_MAXIMA.with(|flag| flag.set(unit.forwards_stores));
    passes::WIDE_UNROLLING.with(|flag| flag.set(unit.forwards_stores));
    passes::FILL_UNROLLING.with(|flag| flag.set(unit.fill_unrolling && std::env::var_os("MWCC_IRO_NO_PLAIN_FILL_UNROLLING").is_none()));
    let mut built = build_unoptimized(function, unit)?;
    if unit.zero_wide_subtrahends && std::env::var_os("MWCC_IRO_NO_SHARED_SUBTRAHENDS").is_none() {
        passes::shared_wide_subtrahends(&mut built.function.body);
    }
    if std::env::var_os("MWCC_IRO_NO_SCALARIZE").is_none() {
        passes::scalarize(&mut built.function, unit.keeps_struct_stores);
    }
    if unit.cancels_float_negations && std::env::var_os("MWCC_IRO_NO_FLOAT_NEGATIONS").is_none() {
        passes::float_negations(&mut built.function.body);
    }
    passes::run(&mut built.function, unit.branch_preserving, unit.reassociates_sums, unit.unrolling);
    if unit.early_frame && std::env::var_os("MWCC_IRO_NO_ENTRY_ADDRESSES").is_none() {
        let eligible = |name: &str| {
            unit.globals.get(name).is_some_and(|global| {
                !global.small_data && !global.is_function && global.fixed_address.is_none() && global.folded.is_none()
            })
        };
        passes::hoist_call_spanning_addresses(&mut built.function, &eligible);
    }
    if unit.forwards_stores && std::env::var_os("MWCC_IRO_NO_DEAD_STORES").is_none() {
        let names: Vec<String> = built.function.variables.iter().map(|variable| variable.name.clone()).collect();
        let removable = |place: &Place| match place {
            Place::Memory { base, .. } => match &base.kind {
                ExprKind::Var(v) => unit.nonvolatile_pointers.contains(&names[*v]),
                ExprKind::GlobalAddress(name) => unit.globals.get(name).is_some_and(|global| !global.is_volatile),
                _ => false,
            },
            Place::Global(name) => unit.globals.get(name).is_some_and(|global| !global.is_volatile),
        };
        passes::dead_stores(&mut built.function.body, &removable);
        // (A local's last update returned at once is the returned value.)
        if std::env::var_os("MWCC_IRO_NO_RETURNED_UPDATES").is_none() {
            let body = &mut built.function.body;
            let length = body.len();
            if length >= 2 {
                if let (Stmt::Assign { variable, value }, Stmt::SetReturn(Expr { kind: ExprKind::Var(returned), .. })) = (&body[length - 2], &body[length - 1]) {
                    if variable == returned
                        && built.function.variables[*variable].kind == mwcc_iro::VariableKind::Local
                        && built.function.variables[*variable].frame.is_none()
                        && !built.function.variables[*variable].volatile
                        && value.ty == built.function.return_type
                        && value.mentions(*variable)
                    {
                        let value = value.clone();
                        body.truncate(length - 2);
                        body.push(Stmt::SetReturn(value));
                    }
                }
            }
        }
        // (A destructor's vtable stores die with the object when nothing
        // after them reads memory or calls, but `operator delete`.)
        if built.function.name.starts_with("__dt__") && std::env::var_os("MWCC_IRO_KEEP_DESTRUCTOR_VTABLES").is_none() {
            if let [Stmt::If { then_body, else_body, .. }, ..] = built.function.body.as_mut_slice() {
                let listing = format!("{then_body:?}");
                if else_body.is_empty()
                    && !listing.contains("Load {")
                    && listing.matches("Call {").count() == listing.matches("Call { name: \"__dl__FPv\"").count()
                {
                    fn strip(body: &mut Vec<Stmt>) {
                        body.retain(|statement| {
                            !matches!(statement, Stmt::Store { place: Place::Memory { base, index: None, .. }, value, .. }
                                if base.as_var() == Some(0) && matches!(&value.kind, ExprKind::GlobalAddress(name) if name.starts_with("__vt__")))
                        });
                        for statement in body.iter_mut() {
                            if let Stmt::If { then_body, else_body, .. } = statement {
                                strip(then_body);
                                strip(else_body);
                            }
                        }
                        // (A test left guarding nothing goes too.)
                        body.retain(|statement| {
                            !matches!(statement, Stmt::If { condition, then_body, else_body }
                                if then_body.is_empty() && else_body.is_empty() && passes::speculable(condition))
                        });
                    }
                    strip(then_body);
                }
            }
        }
    }
    if !unit.branch_preserving && std::env::var_os("MWCC_IRO_NO_BIT_TESTS").is_none() {
        passes::bit_tests(&mut built.function.body);
        passes::narrowing(&mut built.function);
    }

    if std::env::var_os("MWCC_IRO_NO_FRAME_BASES").is_none() {
        passes::fold_frame_bases(&mut built.function);
    }
    if std::env::var_os("MWCC_IRO_KEEP_UNREAD_STORES").is_none() {
        passes::remove_unread_frame_stores(&mut built.function);
    }
    if std::env::var_os("MWCC_IRO_KEEP_UNREAD_CALL_RESULTS").is_none() {
        passes::unread_call_results(&mut built.function);
    }
    // (Without an explicit speed goal, -O3/-O4 count in CTR unrolled.)
    if unit.strength_reduction && !unit.branch_preserving && std::env::var_os("MWCC_IRO_NO_PARTIAL_UNROLL").is_none() {
        unroll::unroll_partially(&mut built.function, unit.unrolling, unit.forwards_stores);
        passes::for_each_expression(&mut built.function.body, &mut |expression| passes::fold(expression));
        passes::displacements(&mut built.function.body, unit.reassociates_sums);
    }
    if unit.strength_reduction && std::env::var_os("MWCC_IRO_NO_CONSTANT_PROPAGATION").is_none() {
        strength::propagate_constants(&mut built.function);
        // (Propagated literals fold: `x + k + m` -> `x + 7`.)
        if std::env::var_os("MWCC_IRO_NO_PROPAGATED_FOLD").is_none() {
            passes::for_each_expression(&mut built.function.body, &mut |expression| {
                passes::algebra(expression);
                passes::fold(expression);
            });
        }
        // (And a test of a propagated constant goes.)
        if !passes::has_label(&built.function.body) && std::env::var_os("MWCC_IRO_NO_PROPAGATED_BRANCHES").is_none() {
            passes::constant_branches(&mut built.function.body);
        }
    }
    if unit.strength_reduction && std::env::var_os("MWCC_IRO_NO_STRENGTH_REDUCTION").is_none() {
        strength::set_loop_addresses(
            unit.globals
                .iter()
                .filter(|(_, global)| !global.small_data && !global.is_function)
                .map(|(name, _)| name.clone())
                .collect(),
            unit.reassociates_sums,
        );
        strength::COUNT_LAST.with(|flag| flag.set(!unit.schedules && std::env::var_os("MWCC_IRO_UNSCHEDULED_COUNT_FIRST").is_none()));
        strength::strength_reduce(&mut built.function, !unit.branch_preserving, unit.unrolling);
        strength::remove_dead_inductions(&mut built.function);
        strength::remove_dead_counted_updates(&mut built.function);
    }
    if unit.branch_preserving && std::env::var_os("MWCC_IRO_NO_UNINDEXED").is_none() {
        let absolute = |name: &str| unit.globals.get(name).is_some_and(|global| !global.small_data);
        passes::unindexed_absolute(&mut built.function.body, &absolute);
    }
    Ok(built)
}

/// Build the IR for an unoptimized (`-O0`) compile: no IRO passes beyond the
/// front end's literal folding, and every `return` leaves directly.
pub fn build_unoptimized_compile(function: &ast::Function, unit: &Unit<'_>) -> Compilation<Built> {
    passes::FLOAT_NEGATION_ALGEBRA.with(|flag| flag.set(unit.cancels_float_negations));
    passes::UNSIGNED_MAXIMA.with(|flag| flag.set(false));
    let mut built = build_unoptimized(function, unit)?;
    if unit.cancels_float_negations && std::env::var_os("MWCC_IRO_NO_FLOAT_NEGATIONS").is_none() {
        passes::float_negations(&mut built.function.body);
    }
    passes::run_unoptimized(&mut built.function);
    if !unit.branch_preserving && std::env::var_os("MWCC_IRO_NO_BIT_TESTS").is_none() {
        passes::bit_tests(&mut built.function.body);
        passes::narrowing(&mut built.function);
    }

    built.returns_through_variable = false;
    Ok(built)
}

/// Build the IR without running the IRO passes.
pub fn build_unoptimized(function: &ast::Function, unit: &Unit<'_>) -> Compilation<Built> {
    if std::env::var("MWCC_SYNTAX_DUMP").is_ok_and(|name| name == function.name) {
        for local in &function.locals {
            if let Some(initializer) = &local.initializer {
                eprintln!("local {} = {:#?}", local.name, initializer);
            }
        }
        eprintln!("{:#?}", function.statements);
        eprintln!("return {:#?}", function.return_expression);
    }
    if function.asm_body.is_some() || !function.inline_asm_blocks.is_empty() {
        return Err(unsupported("inline assembly"));
    }
    // (A local aligned past the frame's 8 bytes realigns the stack.)
    if function.locals.iter().any(|local| !local.is_static && local.attribute_alignment.is_some_and(|align| align > 8)) {
        return Err(unsupported("an over-aligned local (dynamic stack alignment)"));
    }
    if function.return_type != Type::Void && !is_value_type(function.return_type) && !is_wide(function.return_type) {
        return Err(unsupported(format!("return type {:?}", function.return_type)));
    }
    let mut variables = Vec::new();
    for parameter in &function.parameters {
        // A struct passed by value arrives as the address of the caller's
        // copy: the parameter is that pointer.
        let ty = match parameter.parameter_type {
            Type::Struct { size, .. } if std::env::var_os("MWCC_IRO_NO_STRUCT_PARAMETERS").is_none() => {
                Type::StructPointer { element_size: size }
            }
            ty => ty,
        };
        if !is_value_type(ty) && !is_wide(ty) {
            return Err(unsupported(format!("parameter type {:?}", parameter.parameter_type)));
        }
        variables.push(Variable {
            name: parameter.name.clone(),
            ty,
            kind: VariableKind::Parameter,
            frame: None,
            initialized: false,
            raw: false,
        volatile: false,
        });
    }
    let floats = function.parameters.iter().filter(|p| is_float(p.parameter_type)).count();
    // (Words past r10 arrive in the caller's frame; not wide or floating ones.)
    let words_on_stack = function.parameters.len() - floats > ARGUMENT_REGISTERS
        && !function.parameters.iter().any(|p| is_wide(p.parameter_type))
        && std::env::var_os("MWCC_IRO_NO_STACK_PARAMETERS").is_none();
    if (function.parameters.len() - floats > ARGUMENT_REGISTERS && !words_on_stack) || floats > ARGUMENT_REGISTERS {
        return Err(unsupported("stack-passed parameters"));
    }
    let taken = addresses_taken(function, unit.early_frame);
    // (A stack parameter's address is its incoming slot: not modeled.)
    if words_on_stack
        && function
            .parameters
            .iter()
            .filter(|p| !is_float(p.parameter_type))
            .skip(ARGUMENT_REGISTERS)
            .any(|p| taken.contains(&p.name))
    {
        return Err(unsupported("address of a stack parameter"));
    }
    // -O0: a local only initialized at its declaration or assigned (never
    // read) is no register variable: it lives in the frame.
    let mentioned = if unit.unoptimized && std::env::var_os("MWCC_IRO_O0_NO_INIT_ONLY_FRAME").is_none() {
        format!(
            "{:?}{:?}{:?}{:?}",
            function.statements,
            function.guards,
            function.return_expression,
            function.locals.iter().map(|local| &local.initializer).collect::<Vec<_>>()
        )
    } else {
        String::new()
    };
    let initialized_only = |local: &ast::LocalDeclaration| {
        unit.unoptimized
            // (Or one assigned exactly once and never read: its store stays.)
            && (local.initializer.is_some()
                || std::env::var_os("MWCC_IRO_O0_WRITE_ONLY_REGISTERS").is_none()
                    && mentioned.matches(&format!("Assign {{ name: {:?}", local.name)).count() == 1)
            && local.array_length.is_none()
            && !mentioned.is_empty()
            && !mentioned.contains(&format!("Variable({:?})", local.name))
    };
    // A `static const` scalar local with a literal initializer (an inlined
    // `sqrtf`'s `_half`) is its constant.
    let mut folded: HashMap<String, Expr> = HashMap::new();
    for local in &function.locals {
        if local.is_static {
            let value = (local.is_const && local.array_length.is_none())
                .then(|| local.initializer.as_ref().and_then(|value| literal_value(value, local.declared_type)))
                .flatten();
            match value {
                Some(value) => {
                    folded.insert(local.name.clone(), value);
                    continue;
                }
                None => return Err(unsupported("static or volatile locals")),
            }
        }
        // Arrays, structs and scalars whose address is taken live in the frame.
        let element = match local.declared_type {
            Type::Struct { size, align } => Some((size, u32::from(align).max(1))),
            ty if is_value_type(ty) || is_wide(ty) => Some((mwcc_iro::width(ty), mwcc_iro::width(ty))),
            _ => None,
        };
        // (An array or struct slot is word-aligned at least.)
        // (Early frames, and -O0 before GC/3.x, keep natural alignment.)
        let aggregate_align = |size: u32, align: u32| {
            if unit.early_frame
                || (unit.unoptimized && !unit.doubleword_aggregates)
                || std::env::var_os("MWCC_IRO_NATURAL_FRAME_ALIGN").is_some()
            {
                align
            } else if unit.doubleword_aggregates && size % 8 == 0 {
                align.max(8)
            } else {
                align.max(4)
            }
        };
        let frame = match (local.array_length, element) {
            (Some(length), Some((size, align))) => Some((size * u32::from(length), aggregate_align(size * u32::from(length), align))),
            (None, Some((size, align))) if matches!(local.declared_type, Type::Struct { .. }) => Some((size, aggregate_align(size, align))),
            (None, Some(object))
                if taken.contains(&local.name)
                    || initialized_only(local)
                    || local.is_volatile =>
            {
                Some(object)
            }
            (None, Some(_)) => None,
            _ => return Err(unsupported(format!("local type {:?}", local.declared_type))),
        };
        // An initialized frame array copies a constant image (its pointer
        // elements would need relocations).
        if frame.is_some()
            && local.data_bytes.is_some()
            && (!local.data_relocations.is_empty()
                // (A struct copies its image too.)
                || (local.array_length.is_none()
                    && (!matches!(local.declared_type, Type::Struct { .. }) || std::env::var_os("MWCC_IRO_NO_STRUCT_IMAGES").is_some()))
                || std::env::var_os("MWCC_IRO_NO_IMAGES").is_some())
        {
            return Err(unsupported("an initialized frame array"));
        }
        variables.push(Variable {
            name: local.name.clone(),
            ty: local.declared_type,
            kind: VariableKind::Local,
            frame,
            initialized: local.initializer.is_some(),
            raw: false,
            volatile: local.is_volatile,
        });
    }
    // A parameter whose address is taken lives in a frame slot (below the
    // locals' slots) that the incoming register is stored to on entry; the
    // register itself becomes a hidden parameter.
    let mut parameter_homes: Vec<(VarId, VarId)> = Vec::new();
    if function.parameters.iter().any(|parameter| taken.contains(&parameter.name)) {
        if unit.unoptimized || (unit.early_frame && std::env::var_os("MWCC_IRO_NO_EARLY_PARAMETER_HOMES").is_some()) {
            return Err(unsupported("address of a parameter"));
        }
        for (index, parameter) in function.parameters.iter().enumerate().rev() {
            if !taken.contains(&parameter.name) {
                continue;
            }
            let width = mwcc_iro::width(parameter.parameter_type);
            variables[index].name = format!("{}$in", parameter.name);
            parameter_homes.push((index, variables.len()));
            variables.push(Variable {
                name: parameter.name.clone(),
                ty: parameter.parameter_type,
                kind: VariableKind::Local,
                frame: Some((width, width)),
                initialized: false,
            raw: false,
            volatile: false,
            });
        }
    }
    let arrays = function
        .locals
        .iter()
        .filter(|local| local.array_length.is_some())
        .filter_map(|local| variables.iter().position(|variable| variable.name == local.name && variable.kind == VariableKind::Local))
        .collect();
    // Multi-dimensional local arrays: the byte stride of one row.
    let rows = function
        .locals
        .iter()
        .filter_map(|local| {
            let row = u32::from(local.row_bytes?);
            let id = variables.iter().position(|variable| variable.name == local.name && variable.kind == VariableKind::Local)?;
            Some((id, row))
        })
        .collect();
    let mut builder = Builder {
        expansions: 0,
        struct_parameters: function
            .parameters
            .iter()
            .enumerate()
            .filter(|(_, parameter)| matches!(parameter.parameter_type, Type::Struct { .. }))
            .map(|(index, _)| index)
            .collect(),
        arrays,
        rows,
        folded,
        return_type: function.return_type,
        function_name: &function.name,
        unit,
        names: variables.iter().enumerate().map(|(id, variable)| (variable.name.clone(), id)).collect(),
        variables: &variables,
        pending: Vec::new(),
        post: Vec::new(),
        memory_steps: Vec::new(),
        strings: Vec::new(),
        images: Vec::new(),
        guarded: 0,
        argument_guards: 0,
        testing_assignment: false,
        comparing_field: false,
        calling_arguments: 0,
        temporaries: Vec::new(),
        direct_return: false,
    };
    let mut body = Vec::new();
    for &(incoming, home) in parameter_homes.iter().rev() {
        let ty = builder.variables[home].ty;
        body.push(Stmt::Store {
            place: Place::Memory { base: Box::new(builder.local_address(home)), index: None, offset: 0 },
            ty,
            value: Expr { kind: ExprKind::Var(incoming), ty },
            compound: false,
        });
    }
    // (Early frames: locals whose initializer is a constant, an inline
    // expansion's value or a floating-to-integer conversion.)
    let mut slotted: Vec<VarId> = Vec::new();
    let mut inline_slotted: Vec<VarId> = Vec::new();
    let source_listing = format!("{:?}", function);
    for local in &function.locals {
        // (A folded static has no variable and no runtime initialization.)
        if builder.folded.contains_key(&local.name) {
            continue;
        }
        if let (Some(bytes), Some((size, align))) = (&local.data_bytes, builder.names.get(&local.name).and_then(|&id| builder.variables[id].frame)) {
            // The image is the array's bytes (zero-padded; a string exactly the
            // array's length drops its NUL).
            let variable = builder.names[&local.name];
            let mut image = bytes.clone();
            image.resize(size as usize, 0);
            builder.images.push(image);
            let ty = Type::Struct { size, align: u8::try_from(align).unwrap_or(4) };
            let source = Expr { kind: ExprKind::Image(builder.images.len() - 1), ty: Type::StructPointer { element_size: size } };
            body.push(Stmt::Store {
                place: Place::Memory { base: Box::new(builder.local_address(variable)), index: None, offset: 0 },
                ty,
                value: Expr { kind: ExprKind::Load { base: Box::new(source), index: None, offset: 0 }, ty },
                compound: false,
            });
            continue;
        }
        // A struct-typed local initialized from another object copies it.
        if let (Some(initializer), Type::Struct { .. }) = (&local.initializer, local.declared_type) {
            if local.array_length.is_none() && std::env::var_os("MWCC_IRO_NO_STRUCT_COPY").is_none() {
                body.extend(builder.assignment(&Expression::Variable(local.name.clone()), initializer)?);
                body.append(&mut builder.pending);
                continue;
            }
        }
        if let Some(initializer) = &local.initializer {
            let variable = builder.names[&local.name];
            let expansions = builder.expansions;
            let value = assigned(builder.expression(initializer)?, local.declared_type);
            let converts_float = format!("{value:?}").contains("Convert(Expr { kind") && {
                fn floating_conversion(e: &Expr) -> bool {
                    match &e.kind {
                        ExprKind::Convert(operand) => (is_float(operand.ty) && !is_float(e.ty)) || floating_conversion(operand),
                        ExprKind::Binary(_, left, right) => floating_conversion(left) || floating_conversion(right),
                        ExprKind::Unary(_, operand) => floating_conversion(operand),
                        _ => false,
                    }
                }
                floating_conversion(&value)
            };
            let inline = builder.expansions != expansions || unit.inline_initialized_locals.contains(&local.name);
            // (A value derived from an inline-initialized local is the
            // expansion too, once MWCC propagates that local into it.)
            // (Only a local read just there is propagated into it.)
            let derived = inline_slotted.iter().any(|&id| {
                value.mentions(id)
                    && source_listing.matches(&format!("Variable({:?})", variables[id].name)).count() == 1
            })
                && std::env::var_os("MWCC_IRO_NO_DERIVED_INLINE_SLOTS").is_none();
            if inline || derived {
                inline_slotted.push(variable);
            }
            if literal_value(initializer, local.declared_type).is_some() || inline || derived || converts_float {
                slotted.push(variable);
            }
            body.append(&mut builder.pending);
            body.push(match builder.variables[variable].frame {
                Some(_) if local.array_length.is_none() && is_value_type(local.declared_type) => Stmt::Store {
                    place: Place::Memory { base: Box::new(builder.local_address(variable)), index: None, offset: 0 },
                    ty: local.declared_type,
                    value,
                    compound: false,
                },
                Some(_) => return Err(unsupported("an initialized frame aggregate")),
                None => Stmt::Assign { variable, value },
            });
            body.append(&mut builder.post);
        }
    }
    for statement in &function.statements {
        body.extend(builder.statement(statement)?);
    }
    for guard in &function.guards {
        let mut condition = promoted(builder.tested(&guard.condition)?);
        body.append(&mut builder.pending);
        // (Steps in the condition run before the branch; the tested value is
        // copied first when they change it.)
        if !builder.post.is_empty() {
            let after = std::mem::take(&mut builder.post);
            condition = builder.frozen(condition, &after, &mut body);
            body.extend(after);
        }
        let value = builder.returned(&guard.value)?;
        let then_body: Vec<Stmt> = builder.pending.drain(..).chain([Stmt::Return(Some(value))]).collect();
        body.push(Stmt::If { condition, then_body, else_body: Vec::new() });
    }
    if let Some(value) = &function.return_expression {
        // The final value falls through to the exit.
        let value = builder.returned(value)?;
        body.append(&mut builder.pending);
        body.push(Stmt::SetReturn(value));
        // A register variable's step after the final value is dead.
        builder.post.clear();
    }
    let temporaries = std::mem::take(&mut builder.temporaries);
    let mut builder_strings = std::mem::take(&mut builder.strings);
    let builder_images = std::mem::take(&mut builder.images);
    let mut variables = variables;
    for (id, variable) in variables.iter_mut().enumerate() {
        variable.initialized = slotted.contains(&id) || (variable.initialized && std::env::var_os("MWCC_IRO_ALL_INITIALIZED_SLOTTED").is_some());
    }
    variables.extend(temporaries);
    let returns_through_variable = function.return_type != Type::Void
        && (!function.guards.is_empty()
            || has_return(&function.statements)
            || matches!(function.return_expression, Some(Expression::Conditional { .. })));
    Ok(Built {
        function: Function {
            strings: std::mem::take(&mut builder_strings),
            images: builder_images,
            name: function.name.clone(),
            return_type: function.return_type,
            parameter_count: function.parameters.len(),
            variables,
            body,
        },
        returns_through_variable,
    })
}

/// An lvalue whose address has no side effects (evaluating it twice is
/// harmless).
fn pure_lvalue(expression: &Expression) -> bool {
    match expression {
        Expression::Variable(_) | Expression::IntegerLiteral(_) => true,
        Expression::Dereference { pointer } => pure_lvalue(pointer),
        Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => pure_lvalue(base),
        Expression::Index { base, index } => pure_lvalue(base) && pure_lvalue(index),
        Expression::Cast { operand, .. } => pure_lvalue(operand),
        Expression::Binary { left, right, .. } => pure_lvalue(left) && pure_lvalue(right),
        // (`(&g->thaga)->head_p`: an address computed, nothing read but the base.)
        Expression::AddressOf { operand } => {
            matches!(operand.as_ref(), Expression::Member { .. } | Expression::Index { .. }) && pure_lvalue(operand)
        }
        _ => false,
    }
}

/// A call evaluated for its effects (an expanded void call leaves nothing).
fn evaluated(call: Expr) -> Vec<Stmt> {
    if call.ty == Type::Void && matches!(call.kind, ExprKind::Int(_)) {
        Vec::new()
    } else {
        vec![Stmt::Eval(call)]
    }
}

/// Whether any statement assigns `variable`.
fn body_assigns(body: &[Stmt], variable: VarId) -> bool {
    body.iter().any(|statement| match statement {
        Stmt::Assign { variable: assigned, .. } => *assigned == variable,
        Stmt::If { then_body, else_body, .. } => body_assigns(then_body, variable) || body_assigns(else_body, variable),
        Stmt::Loop { body, step, effects, .. } => body_assigns(body, variable) || body_assigns(step, variable) || body_assigns(effects, variable),
        Stmt::Switch { arms, .. } => arms.iter().any(|arm| body_assigns(arm, variable)),
        _ => false,
    })
}

/// Whether a return sits inside a loop or switch (its `break` would not
/// leave the expansion).
fn returns_in_breakable(body: &[Stmt]) -> bool {
    body.iter().any(|statement| match statement {
        Stmt::If { then_body, else_body, .. } => returns_in_breakable(then_body) || returns_in_breakable(else_body),
        Stmt::Loop { body, step, effects, .. } => returns_anywhere(body) || returns_anywhere(step) || returns_anywhere(effects),
        Stmt::Switch { arms, .. } => arms.iter().any(|arm| returns_anywhere(arm)),
        _ => false,
    })
}

/// An expansion's returns as result assignments and `break`s out of the
/// enclosing `do ... while (0)`.
fn early_returns(body: Vec<Stmt>, result: Option<VarId>, ty: Type) -> Vec<Stmt> {
    let mut out = Vec::with_capacity(body.len());
    for statement in body {
        match statement {
            Stmt::Return(value) => {
                if let (Some(id), Some(value)) = (result, value) {
                    out.push(Stmt::Assign { variable: id, value: assigned(value, ty) });
                }
                out.push(Stmt::Break);
            }
            Stmt::If { condition, then_body, else_body } => out.push(Stmt::If {
                condition,
                then_body: early_returns(then_body, result, ty),
                else_body: early_returns(else_body, result, ty),
            }),
            other => out.push(other),
        }
    }
    out
}

/// An expansion's returns (inside loops and switches too) as result
/// assignments and jumps to `label`, placed after it.
fn returns_to_label(body: Vec<Stmt>, result: Option<VarId>, ty: Type, label: &str) -> Vec<Stmt> {
    let mut out = Vec::with_capacity(body.len());
    for statement in body {
        match statement {
            Stmt::Return(value) => {
                if let (Some(id), Some(value)) = (result, value) {
                    out.push(Stmt::Assign { variable: id, value: assigned(value, ty) });
                }
                out.push(Stmt::Goto(label.to_owned()));
            }
            Stmt::SetReturn(value) => {
                if let Some(id) = result {
                    out.push(Stmt::Assign { variable: id, value: assigned(value, ty) });
                }
            }
            Stmt::If { condition, then_body, else_body } => out.push(Stmt::If {
                condition,
                then_body: returns_to_label(then_body, result, ty, label),
                else_body: returns_to_label(else_body, result, ty, label),
            }),
            Stmt::Loop { test_first, condition, body, step, effects } => out.push(Stmt::Loop {
                test_first,
                condition,
                body: returns_to_label(body, result, ty, label),
                step: returns_to_label(step, result, ty, label),
                effects: returns_to_label(effects, result, ty, label),
            }),
            Stmt::Counted { count, guard, body } => out.push(Stmt::Counted {
                count,
                guard,
                body: returns_to_label(body, result, ty, label),
            }),
            Stmt::Switch { value, cases, arms, default } => out.push(Stmt::Switch {
                value,
                cases,
                arms: arms.into_iter().map(|arm| returns_to_label(arm, result, ty, label)).collect(),
                default,
            }),
            other => out.push(other),
        }
    }
    out
}

/// Whether any statement returns (or sets the return value).
fn returns_anywhere(body: &[Stmt]) -> bool {
    body.iter().any(|statement| match statement {
        Stmt::Return(_) | Stmt::SetReturn(_) => true,
        Stmt::If { then_body, else_body, .. } => returns_anywhere(then_body) || returns_anywhere(else_body),
        Stmt::Loop { body, step, effects, .. } => returns_anywhere(body) || returns_anywhere(step) || returns_anywhere(effects),
        Stmt::Switch { arms, .. } => arms.iter().any(|arm| returns_anywhere(arm)),
        _ => false,
    })
}

fn has_return(statements: &[Statement]) -> bool {
    statements.iter().any(|statement| match statement {
        Statement::Return(_) => true,
        Statement::Switch { arms, default, .. } => arms
            .iter()
            .map(|arm| &arm.body)
            .chain(default.iter())
            .any(|body| match body {
                ast::ArmBody::Return(_) => true,
                ast::ArmBody::Statements(statements) => has_return(statements),
            }),
        Statement::If { then_body, else_body, .. } => has_return(then_body) || has_return(else_body),
        Statement::Loop { body, .. } => has_return(body),
        _ => false,
    })
}

struct Builder<'a, 'u> {
    /// Frame variables that are arrays.
    arrays: Vec<VarId>,
    /// Multi-dimensional frame arrays: variable -> row bytes.
    rows: HashMap<VarId, u32>,
    /// Folded `static const` scalar locals: name -> value.
    folded: HashMap<String, Expr>,
    return_type: Type,
    /// The function's name (its pointer variables' signatures are keyed by it).
    function_name: &'a str,
    unit: &'a Unit<'u>,
    names: HashMap<String, VarId>,
    variables: &'a [Variable],
    /// Parameters passed as structs by value (held as their address).
    struct_parameters: Vec<VarId>,
    /// Inline expansions made so far.
    expansions: usize,
    /// Assignments inside the expression being built, hoisted before the
    /// statement that contains it.
    pending: Vec<Stmt>,
    /// Post-increments inside the expression being built, applied after the
    /// statement that contains it.
    post: Vec<Stmt>,
    /// Positions in `pending` of memory post-steps (`p->n++`'s store).
    memory_steps: Vec<usize>,
    /// Inside a conditionally or repeatedly evaluated operand, where an
    /// assignment cannot be hoisted.
    guarded: usize,
    /// How many of those guards are call arguments (evaluated once, in an
    /// unspecified order): a register variable's post-step still applies
    /// after the statement there.
    argument_guards: usize,
    /// Building an `if` condition that is itself an assignment.
    testing_assignment: bool,
    /// Building a bit-field read that is a comparison's direct operand.
    comparing_field: bool,
    /// Enclosing calls with an argument that itself calls.
    calling_arguments: usize,
    /// Registers holding assigned values, numbered after `variables`.
    temporaries: Vec<Variable>,
    /// String literals by bytes, in first-use order.
    strings: Vec<Vec<u8>>,
    /// Constant images of initialized frame objects (with inline callees').
    images: Vec<Vec<u8>>,
    /// The call being built is the returned value itself.
    direct_return: bool,
}

impl Builder<'_, '_> {
    fn statements(&mut self, statements: &[Statement]) -> Compilation<Vec<Stmt>> {
        let mut out = Vec::new();
        for statement in statements {
            out.extend(self.statement(statement)?);
        }
        Ok(out)
    }

    /// An expression evaluated for its effects, as statements (preceded by
    /// the assignments hoisted out of it).
    fn effects(&mut self, expression: &Expression) -> Compilation<Vec<Stmt>> {
        let outer = std::mem::take(&mut self.pending);
        let outer_post = std::mem::take(&mut self.post);
        let outer_steps = std::mem::take(&mut self.memory_steps);
        let result = self.effects_inner(expression);
        let mut hoisted = std::mem::replace(&mut self.pending, outer);
        let mut after = std::mem::replace(&mut self.post, outer_post);
        let steps = std::mem::replace(&mut self.memory_steps, outer_steps);
        let mut out = result?;
        self.settle_memory_steps(&out, &mut hoisted, &mut after, &steps);
        if !hoisted.is_empty() {
            out.splice(0..0, hoisted);
        }
        out.extend(after);
        Ok(out)
    }

    /// A tested value read before `after` (post-steps) runs: the operands
    /// they change are copied first into `before`, a narrow one kept raw
    /// (extended at the test).
    fn frozen(&mut self, value: Expr, after: &[Stmt], before: &mut Vec<Stmt>) -> Expr {
        let changed = (0..self.variables.len() + self.temporaries.len())
            .any(|id| body_assigns(after, id) && value.mentions(id));
        if !changed {
            return value;
        }
        match value.kind {
            ExprKind::Binary(op, left, right) if op.is_comparison() => {
                let left = self.frozen(*left, after, before);
                let right = self.frozen(*right, after, before);
                Expr::binary(op, left, right, value.ty)
            }
            ExprKind::Unary(UnaryOp::LogicalNot, operand) => {
                let operand = self.frozen(*operand, after, before);
                Expr::unary(UnaryOp::LogicalNot, operand, value.ty)
            }
            kind => {
                let value = Expr { kind, ty: value.ty };
                let (inner, ty) = match &value.kind {
                    ExprKind::Convert(inner) if is_narrow(inner.ty) => ((**inner).clone(), value.ty),
                    _ => (value.clone(), value.ty),
                };
                let id = self.temporary(inner.ty);
                let local = id - self.variables.len();
                self.temporaries[local].raw = is_narrow(inner.ty);
                let copy = Expr { kind: ExprKind::Var(id), ty: inner.ty };
                before.push(Stmt::Assign { variable: id, value: inner.clone() });
                if inner.ty == ty { copy } else { Expr { kind: ExprKind::Convert(Box::new(copy)), ty } }
            }
        }
    }

    /// An operand evaluated on its own path: its value, and the statements
    /// that must run before it is used (its effects, then its post-steps,
    /// the value copied first where they change it).
    fn operand_with_effects(&mut self, operand: &Expression) -> Compilation<(Expr, Vec<Stmt>)> {
        let outer = std::mem::take(&mut self.pending);
        let outer_post = std::mem::take(&mut self.post);
        let value = self.expression(operand);
        let effects = std::mem::replace(&mut self.pending, outer);
        let after = std::mem::replace(&mut self.post, outer_post);
        let value = value?;
        // The value is read before the post-steps: copy what they change.
        let mut before = effects;
        let value = self.frozen(value, &after, &mut before);
        before.extend(after);
        Ok((value, before))
    }

    /// A discarded operand's effects; one without effects (a value) is none.
    fn effects_or_nothing(&mut self, expression: &Expression) -> Compilation<Vec<Stmt>> {
        match expression {
            Expression::IntegerLiteral(_) | Expression::Variable(_) => Ok(Vec::new()),
            Expression::Cast { operand, .. } if matches!(operand.as_ref(), Expression::IntegerLiteral(_) | Expression::Variable(_)) => {
                Ok(Vec::new())
            }
            other => self.effects(other),
        }
    }

    fn effects_inner(&mut self, expression: &Expression) -> Compilation<Vec<Stmt>> {
        Ok(match expression {
            Expression::Call { name, arguments } => evaluated(self.call(name, arguments, true)?),
            Expression::CallThrough { target, arguments, .. } => vec![Stmt::Eval(self.indirect_call(target, arguments, true, None)?)],
            Expression::Cast { target_type: Type::Void, operand } => match operand.as_ref() {
                Expression::Call { name, arguments } => evaluated(self.call(name, arguments, true)?),
                // A discarded variable still counts as a reference (-O0
                // register-variable ranking).
                Expression::Variable(name) => match self.names.get(name) {
                    Some(&id) => vec![Stmt::Eval(Expr { kind: ExprKind::Var(id), ty: self.variables[id].ty })],
                    None => Vec::new(),
                },
                Expression::IntegerLiteral(_) => Vec::new(),
                other if !self.unit.unoptimized && std::env::var_os("MWCC_IRO_NO_DISCARDED_VALUES").is_none() => self.discarded(other)?,
                other => return Err(unsupported(format!("statement expression void {}", expression_name(other)))),
            },
            Expression::Assign { target, value } => self.assignment(target, value)?,
            Expression::PostStep { target, operator, pointer_link: None } => {
                let step = Expression::Binary {
                    operator: *operator,
                    left: target.clone(),
                    right: Box::new(Expression::IntegerLiteral(1)),
                };
                self.assignment(target, &step)?
            }
            Expression::Comma { left, right } => {
                let mut out = self.effects_or_nothing(left)?;
                out.extend(self.effects_or_nothing(right)?);
                out
            }
            // `c ? a() : b();` and `x && f();` as statements are branches.
            Expression::Conditional { condition, when_true, when_false, .. } => {
                let condition = promoted(self.expression(condition)?);
                vec![Stmt::If {
                    condition,
                    then_body: self.effects_or_nothing(when_true)?,
                    else_body: self.effects_or_nothing(when_false)?,
                }]
            }
            Expression::Binary { operator: operator @ (BinaryOperator::LogicalAnd | BinaryOperator::LogicalOr), left, right } => {
                let condition = promoted(self.expression(left)?);
                let effects = self.effects_or_nothing(right)?;
                let condition = if *operator == BinaryOperator::LogicalAnd {
                    condition
                } else {
                    Expr::unary(UnaryOp::LogicalNot, condition, Type::Int)
                };
                vec![Stmt::If { condition, then_body: effects, else_body: Vec::new() }]
            }
            other if !self.unit.unoptimized && std::env::var_os("MWCC_IRO_NO_DISCARDED_VALUES").is_none() => self.discarded(other)?,
            other => return Err(unsupported(format!("statement expression {}", expression_name(other)))),
        })
    }

    /// A discarded value without effects: only its reads of memory that may
    /// be volatile remain (each a load into a scratch register).
    fn discarded(&mut self, expression: &Expression) -> Compilation<Vec<Stmt>> {
        let value = self.expression(expression)?;
        let mut reads = Vec::new();
        self.volatile_reads(&value, &mut reads);
        Ok(reads.into_iter().map(Stmt::Eval).collect())
    }

    fn volatile_reads(&self, value: &Expr, reads: &mut Vec<Expr>) {
        match &value.kind {
            ExprKind::Call { .. } => reads.push(value.clone()),
            ExprKind::Global(name) => {
                if self.unit.globals.get(name).is_none_or(|global| global.is_volatile) {
                    reads.push(value.clone());
                }
            }
            ExprKind::Load { base, index, .. } => {
                let mut pointer = base.as_ref();
                while let ExprKind::Binary(BinaryOp::Add, left, right) = &pointer.kind {
                    if right.as_int().is_none() {
                        break;
                    }
                    pointer = left;
                }
                let nonvolatile = match &pointer.kind {
                    ExprKind::Var(id) => self.unit.nonvolatile_pointers.contains(&self.variables[*id].name),
                    ExprKind::GlobalAddress(name) => self.unit.globals.get(name).is_some_and(|global| !global.is_volatile),
                    _ => false,
                };
                if nonvolatile {
                    self.volatile_reads(base, reads);
                    if let Some(index) = index {
                        self.volatile_reads(index, reads);
                    }
                } else {
                    reads.push(value.clone());
                }
            }
            _ => {
                let mut copy = value.clone();
                passes::children(&mut copy, &mut |child| self.volatile_reads(child, reads));
            }
        }
    }

    /// `field = value` for a bit-field: the containing unit is loaded, the
    /// value inserted (`rlwimi`) and the unit stored; a field filling its
    /// whole unit is a plain store.
    fn bit_field_store(&mut self, storage: &Expression, shift: u8, width: u8, value: &Expression) -> Compilation<Vec<Stmt>> {
        if self.unit.bit_field_declared_units {
            return Err(unsupported("bit-field store (declared-type units)"));
        }
        let (place, ty) = self.place(storage)?;
        let bits = 8 * mwcc_iro::width(ty) as u8;
        let value = promoted(self.expression(value)?);
        if is_float(value.ty) || shift + width > bits {
            return Err(unsupported("bit-field store of this value"));
        }
        if shift == 0 && width == bits {
            return Ok(vec![Stmt::Store { place, ty, value: assigned(value, ty), compound: false }]);
        }
        // (A base that calls is evaluated once, for both the read and the
        // write of the unit.)
        let (place, old) = match place {
            Place::Memory { base, index, offset } if format!("{place:?}").contains("Call {") => {
                let ty_base = base.ty;
                let id = self.temporary(ty_base);
                self.pending.push(Stmt::Assign { variable: id, value: *base });
                let base = Box::new(Expr { kind: ExprKind::Var(id), ty: ty_base });
                let old = Expr { kind: ExprKind::Load { base: base.clone(), index: index.clone(), offset }, ty };
                (Place::Memory { base, index, offset }, old)
            }
            place => (place, self.expression(storage)?),
        };
        let begin = 31 - (shift + width - 1);
        let end = 31 - shift;
        let inserted = Expr {
            kind: ExprKind::Idiom(mwcc_iro::Idiom::Insert { base: Box::new(old), value: Box::new(value), shift, begin, end }),
            ty: Type::Int,
        };
        Ok(vec![Stmt::Store { place, ty, value: inserted, compound: false }])
    }

    /// `target = value` as a statement.
    fn assignment(&mut self, target: &Expression, value: &Expression) -> Compilation<Vec<Stmt>> {
        if let Expression::BitFieldRead { storage, shift, width, .. } = target {
            return self.bit_field_store(storage, *shift, *width, value);
        }
        if let Expression::Variable(name) = target {
            if let Some(&variable) = self.names.get(name).filter(|&&id| self.variables[id].frame.is_none()) {
                let ty = self.variables[variable].ty;
                return Ok(vec![Stmt::Assign { variable, value: assigned(self.expression(value)?, ty) }]);
            }
        }
        let (place, ty) = self.place(target)?;
        // A struct assignment copies the source object's bytes.
        if matches!(ty, Type::Struct { .. }) && std::env::var_os("MWCC_IRO_NO_STRUCT_COPY").is_none() {
            let source = self.address_of(value)?;
            let value = Expr { kind: ExprKind::Load { base: Box::new(source), index: None, offset: 0 }, ty };
            return Ok(vec![Stmt::Store { place, ty, value, compound: false }]);
        }
        Ok(vec![Stmt::Store { place, ty, value: assigned(self.expression(value)?, ty), compound: matches!(value, Expression::IndexedUpdateValue { .. }) }])
    }

    /// A statement, preceded by the assignments hoisted out of its
    /// expressions.
    fn statement(&mut self, statement: &Statement) -> Compilation<Vec<Stmt>> {
        let outer = std::mem::take(&mut self.pending);
        let outer_post = std::mem::take(&mut self.post);
        let outer_steps = std::mem::take(&mut self.memory_steps);
        let result = self.statement_inner(statement);
        let mut hoisted = std::mem::replace(&mut self.pending, outer);
        let mut after = std::mem::replace(&mut self.post, outer_post);
        let steps = std::mem::replace(&mut self.memory_steps, outer_steps);
        let mut out = result?;
        self.settle_memory_steps(&out, &mut hoisted, &mut after, &steps);
        if !hoisted.is_empty() {
            out.splice(0..0, hoisted);
        }
        if !after.is_empty() {
            match statement {
                // The step follows the statement that read the old value.
                Statement::Assign { .. } | Statement::Store { .. } | Statement::Expression(_) => out.extend(after),
                // A register variable's step after `return` is dead.
                Statement::Return(_) => {}
                // `if (*p++)`: the steps run before the branch, the tested
                // value copied first when they change it.
                Statement::If { .. } if matches!(out.last(), Some(Stmt::If { .. })) => {
                    let Some(Stmt::If { condition, .. }) = out.last_mut() else { unreachable!() };
                    let value = std::mem::replace(condition, Expr::int(0));
                    let mut before = Vec::new();
                    let value = self.frozen(value, &after, &mut before);
                    let Some(Stmt::If { condition, .. }) = out.last_mut() else { unreachable!() };
                    *condition = value;
                    before.extend(after);
                    let at = out.len() - 1;
                    out.splice(at..at, before);
                }
                _ => return Err(unsupported("post-increment in a control condition")),
            }
        }
        Ok(out)
    }

    /// A register for an intermediate value.
    fn temporary(&mut self, ty: Type) -> VarId {
        let id = self.variables.len() + self.temporaries.len();
        self.temporaries.push(Variable { name: format!("@a{id}"), ty, kind: VariableKind::Temporary, frame: None, initialized: false, raw: false, volatile: false });
        id
    }

    /// A store through a pointer comes before the memory post-steps its
    /// operands make (`*dl->ptr++ = v`, `*o = s->n++`): they move from before
    /// the statement (`hoisted`, at `steps`) to after it, their address
    /// computed before it.
    fn settle_memory_steps(&mut self, out: &[Stmt], hoisted: &mut Vec<Stmt>, after: &mut Vec<Stmt>, steps: &[usize]) {
        // (Never past a call: the callee may read the stepped value.)
        let through_pointer = matches!(out, [Stmt::Store { place: Place::Memory { base, .. }, .. }]
            if !matches!(base.kind, ExprKind::GlobalAddress(_) | ExprKind::LocalAddress(_) | ExprKind::Int(_)))
            && !format!("{out:?}").contains("Call {");
        if !through_pointer || !self.unit.steps_after_pointer_stores || std::env::var_os("MWCC_IRO_STEPS_BEFORE_POINTER_STORES").is_some() {
            return;
        }
        let mut moved = Vec::new();
        for &position in steps.iter().rev() {
            if !matches!(hoisted.get(position), Some(Stmt::Store { compound: true, .. })) {
                continue;
            }
            let mut step = hoisted.remove(position);
            if let Stmt::Store { place: Place::Memory { base, .. }, .. } = &mut step {
                if !matches!(base.kind, ExprKind::Var(_) | ExprKind::GlobalAddress(_) | ExprKind::LocalAddress(_) | ExprKind::Int(_)) {
                    let ty = base.ty;
                    let id = self.temporary(ty);
                    let value = std::mem::replace(base.as_mut(), Expr { kind: ExprKind::Var(id), ty });
                    hoisted.insert(position, Stmt::Assign { variable: id, value });
                }
            }
            moved.insert(0, step);
        }
        moved.append(after);
        *after = moved;
    }

    /// A struct argument: copied into a caller frame object (before the
    /// call), whose address is passed.
    fn struct_argument(&mut self, argument: &Expression, size: u32, align: u8) -> Compilation<Expr> {
        if std::env::var_os("MWCC_IRO_NO_STRUCT_ARGUMENTS").is_some() {
            return Err(unsupported("struct argument"));
        }
        let ty = Type::Struct { size, align };
        let source = self.address_of(argument)?;
        if !matches!(source.ty, Type::StructPointer { element_size } if element_size == size) {
            return Err(unsupported("struct argument expression"));
        }
        let id = self.variables.len() + self.temporaries.len();
        self.temporaries.push(Variable {
            name: format!("@s{id}"),
            ty,
            kind: VariableKind::Local,
            frame: Some((size, u32::from(align).max(1))),
            initialized: false,
            raw: false,
        volatile: false,
        });
        let pointer_type = Type::StructPointer { element_size: size };
        let copy = Expr { kind: ExprKind::LocalAddress(id), ty: pointer_type };
        let value = Expr { kind: ExprKind::Load { base: Box::new(source), index: None, offset: 0 }, ty };
        let store = Stmt::Store { place: Place::Memory { base: Box::new(copy.clone()), index: None, offset: 0 }, ty, value, compound: false };
        self.pending.push(store);
        if !self.unit.unoptimized {
            return Ok(copy);
        }
        // -O0: the copy's address is a register variable, taken after it.
        let id = self.variables.len() + self.temporaries.len();
        self.temporaries.push(Variable { name: format!("@p{id}"), ty: pointer_type, kind: VariableKind::Local, frame: None, initialized: false, raw: false, volatile: false });
        self.pending.push(Stmt::Assign { variable: id, value: copy });
        Ok(Expr { kind: ExprKind::Var(id), ty: pointer_type })
    }

    /// Whether a syntax expression denotes a whole struct object (its value
    /// is evaluated as its address).
    /// A struct-valued expression's size and alignment.
    fn struct_shape(&self, expression: &Expression) -> Option<(u32, u8)> {
        if !self.struct_valued(expression) {
            return None;
        }
        let ty = match expression {
            Expression::Variable(name) => match self.names.get(name) {
                Some(&id) => self.variables[id].ty,
                None => self.unit.globals.get(name)?.ty,
            },
            Expression::Member { member_type, .. } => *member_type,
            _ => return None,
        };
        match ty {
            Type::Struct { size, align } => Some((size, align)),
            _ => None,
        }
    }

    fn struct_valued(&self, expression: &Expression) -> bool {
        match expression {
            Expression::Variable(name) => match self.names.get(name) {
                Some(&id) => matches!(self.variables[id].frame, Some(_))
                    && !self.is_array(id)
                    && matches!(self.variables[id].ty, Type::Struct { .. }),
                None => self
                    .unit
                    .globals
                    .get(name)
                    .is_some_and(|global| !global.is_array && matches!(global.ty, Type::Struct { .. })),
            },
            Expression::Member { member_type: Type::Struct { .. }, .. } => true,
            _ => false,
        }
    }

    /// An expression a test reads directly (a compared or truth-tested
    /// bit-field narrows to its unit at -O0).
    fn tested(&mut self, expression: &Expression) -> Compilation<Expr> {
        let outer = self.comparing_field;
        self.comparing_field = matches!(expression, Expression::BitFieldRead { .. });
        let result = self.expression(expression);
        self.comparing_field = outer;
        result
    }

    /// `target = value` used as a value: the assignment is hoisted and the
    /// assigned value is read from a register.
    fn assignment_value(&mut self, target: &Expression, value: &Expression) -> Compilation<Expr> {
        // (In call arguments too, when no argument of an enclosing call
        // calls: the assignment then precedes the calls either way.)
        let in_arguments = self.guarded == self.argument_guards
            && self.calling_arguments == 0
            && std::env::var_os("MWCC_IRO_NO_ARGUMENT_ASSIGN_VALUE").is_none();
        if (self.guarded > 0 && !in_arguments) || std::env::var_os("MWCC_IRO_NO_ASSIGN_VALUE").is_some() {
            return Err(unsupported("expression Assign"));
        }
        if let Expression::Variable(name) = target {
            if let Some(&variable) = self.names.get(name).filter(|&&id| self.variables[id].frame.is_none()) {
                let ty = self.variables[variable].ty;
                let value = assigned(self.expression(value)?, ty);
                self.pending.push(Stmt::Assign { variable, value });
                return Ok(Expr { kind: ExprKind::Var(variable), ty });
            }
        }
        let (place, ty) = self.place(target)?;
        let value = self.expression(value)?;
        // A narrow integer target stores the raw value; reading the
        // assignment's value converts it.
        if is_narrow(ty) && !is_float(value.ty) && is_value_type(value.ty) && !is_narrow(value.ty) {
            let raw = value.ty;
            let temporary = self.temporary(raw);
            self.pending.push(Stmt::Assign { variable: temporary, value });
            let read = Expr { kind: ExprKind::Var(temporary), ty: raw };
            self.pending.push(Stmt::Store { place, ty, value: read.clone(), compound: false });
            return Ok(Expr { kind: ExprKind::Convert(Box::new(read)), ty });
        }
        let value = assigned(value, ty);
        // (A constant is its own value.)
        // (GC/1.0-1.2.5n test an assigned constant's register: `if ((x = 1))`.)
        if value.as_int().is_some()
            && !is_float(ty)
            && !self.unit.unoptimized
            && !(self.unit.branch_preserving && self.testing_assignment)
            && std::env::var_os("MWCC_IRO_ASSIGNED_CONSTANT_COPY").is_none() {
            self.pending.push(Stmt::Store { place, ty, value: value.clone(), compound: false });
            return Ok(value);
        }
        let temporary = self.temporary(ty);
        self.pending.push(Stmt::Assign { variable: temporary, value });
        self.pending.push(Stmt::Store { place, ty, value: Expr { kind: ExprKind::Var(temporary), ty }, compound: false });
        Ok(Expr { kind: ExprKind::Var(temporary), ty })
    }

    /// An expression built where hoisting is not allowed.
    fn guarded_expression(&mut self, expression: &Expression) -> Compilation<Expr> {
        self.guarded += 1;
        let result = self.expression(expression);
        self.guarded -= 1;
        result
    }

    /// A call argument: guarded, but evaluated exactly once.
    fn argument_expression(&mut self, expression: &Expression) -> Compilation<Expr> {
        self.argument_guards += 1;
        let result = self.guarded_expression(expression);
        self.argument_guards -= 1;
        result
    }

    /// `switch`: arms in source order (the default last), empty labels
    /// sharing the next arm's body.
    fn switch(&mut self, scrutinee: &Expression, arms: &[ast::SwitchArm], default: Option<&ast::ArmBody>) -> Compilation<Stmt> {
        let mut value = promoted(self.expression(scrutinee)?);
        // (MWCC dispatches an unsigned word like an int: `cmpwi`.)
        if value.ty == Type::UnsignedInt {
            value.ty = Type::Int;
        }
        if !matches!(value.ty, Type::Int) {
            return Err(unsupported(format!("switch on {:?}", value.ty)));
        }
        let body = |builder: &mut Self, body: &ast::ArmBody, falls_through: bool| -> Compilation<Vec<Stmt>> {
            Ok(match body {
                ast::ArmBody::Return(value) => vec![Stmt::Return(Some(builder.returned(value)?))],
                ast::ArmBody::Statements(statements) => {
                    let mut out = builder.statements(statements)?;
                    let ends = matches!(out.last(), Some(Stmt::Return(_) | Stmt::Break | Stmt::Continue));
                    if !falls_through && !ends {
                        out.push(Stmt::Break);
                    }
                    out
                }
            })
        };
        let mut cases = Vec::new();
        let mut bodies: Vec<Vec<Stmt>> = Vec::new();
        let mut pending_labels: Vec<i64> = Vec::new();
        for arm in arms {
            if cases.iter().any(|&(value, _)| value == arm.value) || pending_labels.contains(&arm.value) {
                return Err(unsupported("duplicate case value"));
            }
            let empty = matches!(&arm.body, ast::ArmBody::Statements(statements) if statements.is_empty());
            if empty && arm.falls_through {
                pending_labels.push(arm.value);
                continue;
            }
            let index = bodies.len();
            for label in pending_labels.drain(..) {
                cases.push((label, index));
            }
            cases.push((arm.value, index));
            bodies.push(body(self, &arm.body, arm.falls_through)?);
        }
        if !pending_labels.is_empty() {
            // Trailing empty labels fall out of the switch.
            let index = bodies.len();
            for label in pending_labels.drain(..) {
                cases.push((label, index));
            }
            bodies.push(vec![Stmt::Break]);
        }
        let default = match default {
            Some(default) => {
                // The default is laid out last: a final case arm that falls
                // through leaves the switch instead of entering it.
                if let Some(last) = bodies.last_mut() {
                    if !matches!(last.last(), Some(Stmt::Return(_) | Stmt::Break | Stmt::Continue)) {
                        last.push(Stmt::Break);
                    }
                }
                bodies.push(body(self, default, true)?);
                Some(bodies.len() - 1)
            }
            None => None,
        };
        Ok(Stmt::Switch { value, cases, arms: bodies, default })
    }

    fn statement_inner(&mut self, statement: &Statement) -> Compilation<Vec<Stmt>> {
        Ok(vec![match statement {
            Statement::Switch { scrutinee, arms, default } => self.switch(scrutinee, arms, default.as_ref())?,
            // A frame variable, or a file-scope object (a function static
            // the parser saw as a local): a store.
            Statement::Assign { name, value }
                if self.names.get(name).is_some_and(|&id| self.variables[id].frame.is_some())
                    || (!self.names.contains_key(name) && self.unit.globals.contains_key(name)) =>
            {
                return self.assignment(&Expression::Variable(name.clone()), value);
            }
            Statement::Assign { name, value } => {
                let Some(&variable) = self.names.get(name) else {
                    return Err(unsupported(format!("assignment to non-local '{name}'")));
                };
                // (A register variable's address stored into a local nothing
                // reads: the dead store goes with it.)
                if let Expression::AddressOf { operand } = value {
                    if let Expression::Variable(target) = operand.as_ref() {
                        if self.names.get(target).is_some_and(|&id| self.variables[id].frame.is_none())
                            && std::env::var_os("MWCC_IRO_DEAD_ADDRESSES_TAKEN").is_none()
                        {
                            return Ok(Vec::new());
                        }
                    }
                }
                let ty = self.variables[variable].ty;
                Stmt::Assign { variable, value: assigned(self.expression(value)?, ty) }
            }
            Statement::Expression(expression) => return self.effects(expression),
            Statement::Loop { kind, initializer, condition, step, body } => {
                let mut out = match initializer {
                    Some(initializer) => self.effects(initializer)?,
                    None => Vec::new(),
                };
                let (pending_mark, post_mark) = (self.pending.len(), self.post.len());
                let guarded = condition.as_ref().map(|c| self.guarded_expression(c)).transpose();
                if guarded.is_err() {
                    // (Undo whatever the failed attempt queued.)
                    self.pending.truncate(pending_mark);
                    self.post.truncate(post_mark);
                }
                let (condition, effects) = match guarded {
                    Ok(condition) => (condition.map(promoted), Vec::new()),
                    // A condition with effects (`while ((c = *p++))`): they run
                    // before each test (in the loop's test block).
                    Err(_) if std::env::var_os("MWCC_IRO_NO_EFFECT_CONDITIONS").is_none() => {
                        let condition = condition.as_ref().expect("a failed condition exists");
                        let (pending, post) = (std::mem::take(&mut self.pending), std::mem::take(&mut self.post));
                        let value = promoted(self.expression(condition)?);
                        let mut effects = std::mem::replace(&mut self.pending, pending);
                        let after = std::mem::replace(&mut self.post, post);
                        let value = self.frozen(value, &after, &mut effects);
                        effects.extend(after);
                        (Some(value), effects)
                    }
                    Err(error) => return Err(error),
                };
                let body = self.statements(body)?;
                let step = match step {
                    Some(step) => self.effects(step)?,
                    None => Vec::new(),
                };
                out.push(Stmt::Loop { test_first: *kind != ast::LoopKind::DoWhile, condition, body, step, effects });
                return Ok(out);
            }
            Statement::Break => Stmt::Break,
            Statement::Goto(name) => Stmt::Goto(name.clone()),
            Statement::Label(name) => Stmt::Label(name.clone()),
            Statement::Continue => Stmt::Continue,
            Statement::Store { target, value } if matches!(target, Expression::BitFieldRead { .. }) => {
                return self.assignment(target, value);
            }
            // (A struct store copies its source through `assignment`.)
            Statement::Store { target, value } => return self.assignment(target, value),
            Statement::If { condition, then_body, else_body } => {
                let testing = matches!(condition, Expression::Assign { .. });
                let outer = std::mem::replace(&mut self.testing_assignment, testing);
                let condition = self.tested(condition);
                self.testing_assignment = outer;
                Stmt::If {
                    condition: promoted(condition?),
                    then_body: self.statements(then_body)?,
                    else_body: self.statements(else_body)?,
                }
            }
            Statement::Return(value) => {
                Stmt::Return(value.as_ref().map(|value| self.returned(value)).transpose()?)
            }
            other => return Err(unsupported(format!("statement {}", statement_name(other)))),
        }])
    }

    /// A store target and the type it holds.
    fn place(&mut self, target: &Expression) -> Compilation<(Place, Type)> {
        match target {
            Expression::Member { base, offset, member_type, index_stride } => {
                let base = self.member_base(base, *index_stride)?;
                let (base, index, offset, ty) = displaced(base, *offset as i32, *member_type);
                Ok((Place::Memory { base, index, offset }, ty))
            }
            // An element of an embedded array of structs (`s->w[i]`): the
            // object itself.
            Expression::Index { base: array, .. }
                if matches!(array.as_ref(), Expression::Member { member_type: Type::Struct { .. }, .. }) =>
            {
                let Expression::Member { member_type: Type::Struct { size, align }, .. } = array.as_ref() else { unreachable!() };
                let (size, align) = (*size, *align);
                let address = self.address_of(target)?;
                Ok((Place::Memory { base: Box::new(address), index: None, offset: 0 }, Type::Struct { size, align }))
            }
            Expression::Index { base, index } => {
                let pointer = self.pointer_sum(base, index)?;
                match self.element(pointer)? {
                    Some((base, index, offset, ty)) => Ok((Place::Memory { base, index, offset }, ty)),
                    None => Err(unsupported("store to an index of a non-pointer")),
                }
            }
            Expression::Dereference { pointer } => {
                let pointer = self.expression(pointer)?;
                // `*p` of a struct pointer: the object itself (a block copy's
                // destination; its alignment is unknown, which -O0 needs).
                if let Type::StructPointer { element_size } = pointer.ty {
                    if element_size > 0 && !self.unit.unoptimized {
                        let ty = Type::Struct { size: element_size, align: 4 };
                        return Ok((Place::Memory { base: Box::new(pointer), index: None, offset: 0 }, ty));
                    }
                }
                match self.element(pointer)? {
                    Some((base, index, offset, ty)) => Ok((Place::Memory { base, index, offset }, ty)),
                    None => Err(unsupported("store through a non-scalar pointer")),
                }
            }
            Expression::Variable(name) if self.names.get(name).is_some_and(|&id| self.variables[id].frame.is_some()) => {
                let id = self.names[name];
                let ty = self.variables[id].ty;
                Ok((Place::Memory { base: Box::new(self.local_address(id)), index: None, offset: 0 }, ty))
            }
            Expression::Variable(name) if self.unit.globals.contains_key(name) && !self.names.contains_key(name) => {
                let global = self.unit.globals[name];
                if global.is_array {
                    return Err(unsupported("store to an array global"));
                }
                Ok((Place::Global(name.clone()), global.ty))
            }
            _ => Err(unsupported("store target")),
        }
    }

    /// `base + index` as pointer arithmetic (for `base[index]`).
    fn pointer_sum(&mut self, base: &Expression, index: &Expression) -> Compilation<Expr> {
        let base = self.expression(base)?;
        let index = self.expression(index)?;
        if !matches!(base.ty, Type::Pointer(_) | Type::StructPointer { .. }) {
            return Err(unsupported("index of a non-pointer"));
        }
        self.pointer_arithmetic(BinaryOp::Add, base, index)
    }

    /// Split a pointer value into a scalar element access:
    /// `(base, scaled index, offset, element type)`.
    #[allow(clippy::type_complexity)]
    fn element(&mut self, pointer: Expr) -> Compilation<Option<(Box<Expr>, Option<Box<Expr>>, i32, Type)>> {
        let Type::Pointer(pointee) = pointer.ty else { return Ok(None) };
        let Some(loaded) = pointee_type(pointee) else { return Err(unsupported("pointee type")) };
        if let ExprKind::Binary(BinaryOp::Add, left, right) = &pointer.kind {
            // `p + i` or the commuted `i + p`.
            let (base, index) = if matches!(right.ty, Type::Pointer(_)) { (right, left) } else { (left, right) };
            if matches!(base.ty, Type::Pointer(_)) && element_size(index.ty).is_none() {
                if let Some(offset) = index.as_int() {
                    if let Ok(offset) = i16::try_from(offset) {
                        return Ok(Some((base.clone(), None, i32::from(offset), loaded)));
                    }
                } else {
                    return Ok(Some((base.clone(), Some(index.clone()), 0, loaded)));
                }
            }
        }
        Ok(Some(displaced(pointer, 0, loaded)))
    }

    /// Whether a frame variable is an array (its name is its address).
    fn is_array(&self, id: VarId) -> bool {
        self.arrays.contains(&id)
    }

    /// The address of a frame variable, typed as a pointer to its element.
    fn local_address(&self, id: VarId) -> Expr {
        let ty = pointer_to(self.variables[id].ty).unwrap_or(Type::StructPointer { element_size: 0 });
        Expr { kind: ExprKind::LocalAddress(id), ty }
    }

    /// A returned value: floating results convert to the return type.
    fn returned(&mut self, value: &Expression) -> Compilation<Expr> {
        self.direct_return = matches!(value, Expression::Call { .. });
        let value = self.expression(value);
        self.direct_return = false;
        let value = value?;
        Ok(if is_float(self.return_type) || is_float(value.ty) { converted(value, self.return_type) } else { value })
    }

    /// `pointer ± integer` with the integer scaled to bytes.
    fn pointer_arithmetic(&mut self, op: BinaryOp, left: Expr, right: Expr) -> Compilation<Expr> {
        let left_size = element_size(left.ty);
        let right_size = element_size(right.ty);
        match (left_size, right_size) {
            // `p - q`: the byte difference divided (signed) by the element size.
            (Some(size), Some(other)) if op == BinaryOp::Subtract && size == other && size > 0 => {
                let as_int = |e: Expr| Expr { ty: Type::Int, ..e };
                let bytes = Expr::binary(BinaryOp::Subtract, as_int(left), as_int(right), Type::Int);
                Ok(if size == 1 { bytes } else { Expr::binary(BinaryOp::Divide, bytes, Expr::int(i64::from(size)), Type::Int) })
            }
            (Some(_), Some(_)) => Err(unsupported("pointer difference")),
            (Some(0), None) | (None, Some(0)) => Err(unsupported("arithmetic on an unsized pointee")),
            // `p + 0` (`&a[0]`) is the pointer itself.
            (Some(_), None) if right.as_int() == Some(0) && std::env::var_os("MWCC_IRO_NO_ZERO_OFFSET_FOLD").is_none() => Ok(left),
            (Some(size), None) => {
                let ty = left.ty;
                let right = promoted(right);
                // (A flattened row index `p + (i*K + j)`: the row first,
                // `(p + i*K*S) + j*S`.)
                if let (BinaryOp::Add, ExprKind::Binary(BinaryOp::Add, row, column)) = (op, &right.kind) {
                    if let ExprKind::Binary(BinaryOp::Multiply, i, k) = &row.kind {
                        if let Some(k) = k.as_int().filter(|_| size > 1 && std::env::var_os("MWCC_IRO_FLAT_ROW_SCALE").is_none()) {
                            let rows = Expr::binary(BinaryOp::Multiply, (**i).clone(), Expr::int(k * i64::from(size)), promote(i.ty));
                            let base = Expr::binary(BinaryOp::Add, left, rows, ty);
                            return Ok(Expr::binary(BinaryOp::Add, base, scale((**column).clone(), size), ty));
                        }
                    }
                }
                Ok(Expr::binary(op, left, scale(right, size), ty))
            }
            (None, Some(size)) if op == BinaryOp::Add => {
                let ty = right.ty;
                Ok(Expr::binary(op, scale(promoted(left), size), right, ty))
            }
            (None, Some(_)) => Err(unsupported("integer minus pointer")),
            (None, None) => unreachable!("pointer arithmetic needs a pointer"),
        }
    }

    fn expression(&mut self, expression: &Expression) -> Compilation<Expr> {
        Ok(match expression {
            Expression::IntegerLiteral(value) => Expr::int(*value),
            // A bare floating literal is `float`; a `double` one is cast.
            Expression::FloatLiteral(value) => Expr { kind: ExprKind::Float(*value), ty: Type::Float },
            Expression::Variable(name) if self.folded.contains_key(name) => self.folded[name].clone(),
            Expression::Variable(name) => {
                if let Some(&id) = self.names.get(name) {
                    let variable = &self.variables[id];
                    if variable.frame.is_some() {
                        let address = self.local_address(id);
                        // An array or struct names its address; a scalar loads.
                        if !matches!(address.ty, Type::StructPointer { .. }) && is_value_type(variable.ty) && !self.is_array(id) {
                            return Ok(Expr {
                                kind: ExprKind::Load { base: Box::new(address), index: None, offset: 0 },
                                ty: variable.ty,
                            });
                        }
                        return Ok(address);
                    }
                    return Ok(Expr { kind: ExprKind::Var(id), ty: self.variables[id].ty });
                }
                let Some(global) = self.unit.globals.get(name) else {
                    return Err(unsupported(format!("unknown variable '{name}'")));
                };
                // A folded `static const` scalar is its value.
                if let Some(bits) = global.folded {
                    return Ok(match global.ty {
                        Type::Double => Expr { kind: ExprKind::Float(f64::from_bits(bits as u64)), ty: Type::Double },
                        Type::Float => Expr { kind: ExprKind::Float(f64::from(f32::from_bits(bits as u32))), ty: Type::Float },
                        ty if is_narrow(ty) => Expr::int(bits),
                        ty if is_value_type(ty) => Expr::typed_int(bits, ty),
                        _ => return Err(unsupported("a folded constant of this type")),
                    });
                }
                // A fixed-address array is its constant address.
                if let Some(address) = global.fixed_address {
                    let ty = pointer_to(global.ty).ok_or_else(|| unsupported("fixed-address array of this element type"))?;
                    return Ok(Expr { kind: ExprKind::Int(address), ty });
                }
                // An array (or aggregate) global denotes its address.
                if global.is_array || matches!(global.ty, Type::Struct { .. }) {
                    let ty = pointer_to(global.ty).ok_or_else(|| unsupported("array global of this element type"))?;
                    return Ok(Expr { kind: ExprKind::GlobalAddress(name.clone()), ty });
                }
                Expr { kind: ExprKind::Global(name.clone()), ty: global.ty }
            }
            Expression::Binary { operator, left, right } => {
                let op = binary_op(*operator);
                let (left_source, right_source) = (left.as_ref(), right.as_ref());
                // (A field compared with a literal, or truth-tested.)
                let literal = |e: &Expression| matches!(e, Expression::IntegerLiteral(_));
                let left = if (op.is_comparison() && literal(right_source)) || matches!(op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr) {
                    self.tested(left)?
                } else {
                    self.expression(left)?
                };
                let right = if matches!(op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr) {
                    let mark = (self.pending.len(), self.post.len());
                    match self.guarded_expression(right) {
                        Ok(right) => right,
                        // (A right operand with effects: it runs only when
                        // the left one decides nothing; a temporary holds
                        // the truth value.)
                        Err(_) if self.guarded == 0 && std::env::var_os("MWCC_IRO_NO_BRANCHY_OPERANDS").is_none() => {
                            self.pending.truncate(mark.0);
                            self.post.truncate(mark.1);
                            let (right, mut body) = self.operand_with_effects(right)?;
                            let id = self.temporary(Type::Int);
                            let truth = match &right.kind {
                                ExprKind::Binary(op, ..) if op.is_comparison() || matches!(op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr) => right,
                                ExprKind::Unary(UnaryOp::LogicalNot, _) => right,
                                _ => Expr::binary(BinaryOp::NotEqual, promoted(right), Expr::int(0), Type::Int),
                            };
                            body.push(Stmt::Assign { variable: id, value: truth });
                            let and = op == BinaryOp::LogicalAnd;
                            self.pending.push(Stmt::Assign { variable: id, value: Expr::int(i64::from(!and)) });
                            let condition = promoted(left);
                            let condition = if and { condition } else { Expr::unary(UnaryOp::LogicalNot, condition, Type::Int) };
                            self.pending.push(Stmt::If { condition, then_body: body, else_body: Vec::new() });
                            return Ok(Expr { kind: ExprKind::Var(id), ty: Type::Int });
                        }
                        Err(error) => return Err(error),
                    }
                } else if op.is_comparison() && literal(left_source) {
                    self.tested(right)?
                } else {
                    self.expression(right)?
                };
                if matches!(op, BinaryOp::Add | BinaryOp::Subtract)
                    && (element_size(left.ty).is_some() || element_size(right.ty).is_some())
                {
                    return self.pointer_arithmetic(op, left, right);
                }
                // Floating operands meet at the wider floating type.
                if (is_float(left.ty) || is_float(right.ty)) && !matches!(op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr) {
                    let common = if left.ty == Type::Double || right.ty == Type::Double { Type::Double } else { Type::Float };
                    let ty = if op.is_comparison() { Type::Int } else { common };
                    return Ok(Expr::binary(op, converted(left, common), converted(right, common), ty));
                }
                // (An unsigned bit-field compared with a nonnegative
                // literal compares unsigned: `cmplwi`.)
                let unsigned_field = |source: &Expression, built: &Expr| {
                    matches!(source, Expression::BitFieldRead { .. })
                        && match &built.kind {
                            ExprKind::Binary(BinaryOp::BitAnd, _, mask) => mask.as_int().is_some_and(|m| m >= 0),
                            ExprKind::Binary(BinaryOp::ShiftRight, value, _) => {
                                matches!(&value.kind, ExprKind::Convert(inner) if is_unsigned(inner.ty))
                            }
                            _ => false,
                        }
                };
                let as_unsigned = |e: Expr| Expr { kind: ExprKind::Convert(Box::new(e)), ty: Type::UnsignedInt };
                let (left, right) = if op.is_comparison() && std::env::var_os("MWCC_IRO_SIGNED_FIELD_COMPARES").is_none() {
                    if unsigned_field(left_source, &left) && right.as_int().is_some_and(|v| v >= 0) {
                        (as_unsigned(left), Expr::typed_int(right.as_int().expect("checked"), Type::UnsignedInt))
                    } else if unsigned_field(right_source, &right) && left.as_int().is_some_and(|v| v >= 0) {
                        (Expr::typed_int(left.as_int().expect("checked"), Type::UnsignedInt), as_unsigned(right))
                    } else {
                        (left, right)
                    }
                } else {
                    (left, right)
                };
                let ty = if op.is_comparison() || matches!(op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr) {
                    Type::Int
                } else if matches!(op, BinaryOp::ShiftLeft | BinaryOp::ShiftRight) {
                    promote(left.ty)
                } else {
                    arithmetic_type(left.ty, right.ty)
                };
                // The integer promotions are explicit conversions.
                let right = if op == BinaryOp::Subtract && self.unit.zero_wide_subtrahends {
                    wide_subtrahend(promoted(right), ty)
                } else {
                    promoted(right)
                };
                // `long long` division, and shifts by a variable count, call
                // the runtime (literals fold).
                // (GC/1.3+ divide by a power of two inline: an unsigned
                // quotient shifts, a remainder masks; a signed quotient
                // shifts with rounding toward zero.)
                if is_wide(ty) && matches!(op, BinaryOp::Divide | BinaryOp::Modulo) && !self.unit.branch_preserving {
                    let mut divisor = right.clone();
                    passes::fold(&mut divisor);
                    if let Some(power) = divisor.as_int().filter(|d| *d > 1 && d.count_ones() == 1).map(|d| i64::from(d.trailing_zeros())) {
                        let left = converted(promoted(left.clone()), ty);
                        let unsigned = ty == Type::UnsignedLongLong;
                        match op {
                            BinaryOp::Divide if unsigned => {
                                return Ok(Expr::binary(BinaryOp::ShiftRight, left, Expr::int(power), ty));
                            }
                            BinaryOp::Modulo if unsigned => {
                                return Ok(Expr::binary(BinaryOp::BitAnd, left, Expr::typed_int((1i64 << power) - 1, ty), ty));
                            }
                            _ if power < 32 => {
                                let quotient = Expr::binary(BinaryOp::Divide, left.clone(), Expr::typed_int(1i64 << power, ty), ty);
                                if op == BinaryOp::Divide {
                                    return Ok(quotient);
                                }
                                let multiple = Expr::binary(BinaryOp::ShiftLeft, quotient, Expr::int(power), ty);
                                return Ok(Expr::binary(BinaryOp::Subtract, left, multiple, ty));
                            }
                            _ => {}
                        }
                    }
                }
                if is_wide(ty) && matches!(op, BinaryOp::Divide | BinaryOp::Modulo | BinaryOp::ShiftLeft | BinaryOp::ShiftRight) {
                    let constant = |e: &Expr| {
                        let mut folded = e.clone();
                        passes::fold(&mut folded);
                        folded.as_int().is_some()
                    };
                    let shift = matches!(op, BinaryOp::ShiftLeft | BinaryOp::ShiftRight);
                    // (GC/1.0-1.2.5n shift by a constant through the runtime too.)
                    let inline_shift = shift && !self.unit.branch_preserving;
                    if !(constant(&right) && (inline_shift || constant(&left))) {
                        let unsigned = ty == Type::UnsignedLongLong;
                        let name = match op {
                            BinaryOp::Divide if unsigned => "__div2u",
                            BinaryOp::Divide => "__div2i",
                            BinaryOp::Modulo if unsigned => "__mod2u",
                            BinaryOp::Modulo => "__mod2i",
                            BinaryOp::ShiftLeft => "__shl2i",
                            _ if unsigned => "__shr2u",
                            _ => "__shr2i",
                        };
                        let left = converted(promoted(left), ty);
                        let right = if shift { right } else { converted(right, ty) };
                        return Ok(Expr { kind: ExprKind::Call { name: name.to_owned(), arguments: vec![left, right] }, ty });
                    }
                }
                Expr::binary(op, promoted(left), right, ty)
            }
            Expression::Unary { operator, operand } => {
                let operand = if *operator == UnaryOperator::LogicalNot { self.tested(operand)? } else { self.expression(operand)? };
                let operand = promoted(operand);
                match operator {
                    UnaryOperator::Negate => {
                        let ty = promote(operand.ty);
                        Expr::unary(UnaryOp::Negate, operand, ty)
                    }
                    UnaryOperator::BitNot => {
                        let ty = promote(operand.ty);
                        Expr::unary(UnaryOp::BitNot, operand, ty)
                    }
                    UnaryOperator::LogicalNot => Expr::unary(UnaryOp::LogicalNot, operand, Type::Int),
                }
            }
            // -O0: the value is a register variable assigned on each path.
            Expression::Conditional { condition, when_true, when_false, .. }
                if self.unit.unoptimized && self.guarded == 0 && std::env::var_os("MWCC_IRO_O0_SELECT_VALUE").is_none() =>
            {
                let condition = promoted(self.expression(condition)?);
                let outer = std::mem::take(&mut self.pending);
                let posts = self.post.len();
                let when_true = promoted(self.expression(when_true)?);
                let mut then_body = std::mem::take(&mut self.pending);
                let when_false = promoted(self.expression(when_false)?);
                let mut else_body = std::mem::take(&mut self.pending);
                self.pending = outer;
                if self.post.len() != posts {
                    return Err(unsupported("post-increment in a conditional operand"));
                }
                let ty = if is_float(when_true.ty) || is_float(when_false.ty) {
                    if when_true.ty == Type::Double || when_false.ty == Type::Double { Type::Double } else { Type::Float }
                } else {
                    arithmetic_type(when_true.ty, when_false.ty)
                };
                let id = self.variables.len() + self.temporaries.len();
                self.temporaries.push(Variable { name: format!("@c{id}"), ty, kind: VariableKind::Local, frame: None, initialized: false, raw: false, volatile: false });
                then_body.push(Stmt::Assign { variable: id, value: converted(when_true, ty) });
                else_body.push(Stmt::Assign { variable: id, value: converted(when_false, ty) });
                self.pending.push(Stmt::If { condition, then_body, else_body });
                Expr { kind: ExprKind::Var(id), ty }
            }
            Expression::Conditional { condition, when_true, when_false, .. } => {
                let condition = promoted(self.expression(condition)?);
                let mark = (self.pending.len(), self.post.len());
                let arms = self
                    .guarded_expression(when_true)
                    .and_then(|when_true| Ok((when_true, self.guarded_expression(when_false)?)));
                let (when_true, when_false) = match arms {
                    Ok(arms) => arms,
                    // (Operands with effects: branches assign a temporary.)
                    Err(_) if self.guarded == 0 && std::env::var_os("MWCC_IRO_NO_BRANCHY_OPERANDS").is_none() => {
                        self.pending.truncate(mark.0);
                        self.post.truncate(mark.1);
                        let (when_true, mut then_body) = self.operand_with_effects(when_true)?;
                        let (when_false, mut else_body) = self.operand_with_effects(when_false)?;
                        let ty = if is_float(when_true.ty) || is_float(when_false.ty) {
                            if when_true.ty == Type::Double || when_false.ty == Type::Double { Type::Double } else { Type::Float }
                        } else {
                            arithmetic_type(when_true.ty, when_false.ty)
                        };
                        let id = self.temporary(ty);
                        then_body.push(Stmt::Assign { variable: id, value: converted(when_true, ty) });
                        else_body.push(Stmt::Assign { variable: id, value: converted(when_false, ty) });
                        self.pending.push(Stmt::If { condition, then_body, else_body });
                        return Ok(Expr { kind: ExprKind::Var(id), ty });
                    }
                    Err(error) => return Err(error),
                };
                let when_true = promoted(when_true);
                let when_false = promoted(when_false);
                let ty = arithmetic_type(when_true.ty, when_false.ty);
                Expr {
                    kind: ExprKind::Select {
                        condition: Box::new(condition),
                        when_true: Box::new(when_true),
                        when_false: Box::new(when_false),
                    },
                    ty,
                }
            }
            Expression::Cast { target_type, operand } if is_value_type(*target_type) || is_wide(*target_type) => {
                let operand = self.expression(operand)?;
                Expr { kind: ExprKind::Convert(Box::new(operand)), ty: *target_type }
            }
            Expression::Member { base, offset, member_type, index_stride } => {
                let base = self.member_base(base, *index_stride)?;
                if !is_value_type(*member_type) && !is_wide(*member_type) {
                    return Err(unsupported(format!("load of {member_type:?}")));
                }
                let (base, index, offset, ty) = displaced(base, *offset as i32, *member_type);
                Expr { kind: ExprKind::Load { base, index, offset }, ty }
            }
            Expression::AddressOf { operand } => self.address_of(operand)?,
            // Provenance wrappers: the value is the wrapped expression.
            Expression::IndexedUpdateValue { value } => {
                let built = self.expression(value)?;
                // (A floating `x += k` keeps its operands in source order.)
                let constant = |e: &Expr| match &e.kind {
                    ExprKind::Float(_) => true,
                    ExprKind::Convert(inner) => matches!(inner.kind, ExprKind::Float(_) | ExprKind::Int(_)),
                    _ => false,
                };
                match built.kind {
                    ExprKind::Binary(op @ (BinaryOp::Add | BinaryOp::Multiply), left, right)
                        if is_float(built.ty)
                            // (-O0: a variable's update; a memory one keeps
                            // its compound address reuse.)
                            && (!self.unit.unoptimized || matches!(left.kind, ExprKind::Var(_)))
                            && left.ty == built.ty
                            && constant(&right)
                            && std::env::var_os("MWCC_IRO_NO_FLOAT_UPDATES").is_none() =>
                    {
                        Expr { kind: ExprKind::Idiom(mwcc_iro::Idiom::Update(op, left, right)), ty: built.ty }
                    }
                    kind => Expr { kind, ty: built.ty },
                }
            }
            Expression::BitFieldRead { extracted, promoted_type, .. } => {
                let mut value = self.expression(extracted)?;
                mark_bit_field_shift(&mut value);
                // (-O0 narrows a compared field to its storage unit first.)
                if self.unit.unoptimized && self.comparing_field && std::env::var_os("MWCC_IRO_O0_UNNARROWED_FIELDS").is_none() {
                    fn unit_type(e: &Expr) -> Option<Type> {
                        if let ExprKind::Load { .. } = e.kind {
                            return Some(e.ty);
                        }
                        let mut found = None;
                        let mut copy = e.clone();
                        passes::children(&mut copy, &mut |child| {
                            if found.is_none() {
                                found = unit_type(child);
                            }
                        });
                        found
                    }
                    if let Some(storage) = unit_type(&value).filter(|&ty| is_narrow(ty)) {
                        value = Expr { kind: ExprKind::Convert(Box::new(value)), ty: storage };
                    }
                }
                converted(promoted(value), *promoted_type)
            }
            Expression::MemberAddress { base, offset, element, index_stride: None } => {
                let base = self.aggregate_address(base)?;
                let ty = pointee_type(*element).and_then(pointer_to).unwrap_or(Type::Pointer(*element));
                // (A member at offset 0 is the base itself, retyped.)
                if *offset == 0 && std::env::var_os("MWCC_IRO_MEMBER_ZERO_ADD").is_none() {
                    return Ok(Expr { ty, ..base });
                }
                Expr::binary(BinaryOp::Add, base, Expr::int(i64::from(*offset)), ty)
            }
            // A row of a multi-dimensional member array is its address
            // (`p->m[i]`: the member plus `i` rows).
            Expression::Index { base, index }
                if matches!(base.as_ref(), Expression::MemberAddress { index_stride: Some(_), .. })
                    && std::env::var_os("MWCC_IRO_NO_MEMBER_ROWS").is_none() =>
            {
                let Expression::MemberAddress { base, offset, element, index_stride: Some(stride) } = base.as_ref() else { unreachable!() };
                // (The rows first, the member offset outermost: it becomes
                // the access's displacement.)
                let address = self.aggregate_address(base)?;
                let ty = pointee_type(*element).and_then(pointer_to).unwrap_or(Type::Pointer(*element));
                let index = self.expression(index)?;
                let row = Expr::binary(BinaryOp::Add, Expr { ty, ..address }, scale(promoted(index), *stride), ty);
                if *offset == 0 {
                    row
                } else {
                    Expr::binary(BinaryOp::Add, row, Expr::int(i64::from(*offset)), ty)
                }
            }
            // A row of a multi-dimensional local array is its address.
            Expression::Index { base, index }
                if matches!(base.as_ref(), Expression::Variable(name) if self.names.get(name).is_some_and(|id| self.rows.contains_key(id))) =>
            {
                let Expression::Variable(name) = base.as_ref() else { unreachable!() };
                let id = self.names[name];
                let row = self.rows[&id];
                let address = self.local_address(id);
                let index = self.expression(index)?;
                let ty = address.ty;
                Expr::binary(BinaryOp::Add, address, scale(promoted(index), row), ty)
            }
            Expression::Index { base, index } => {
                let pointer = self.pointer_sum(base, index)?;
                let Some((base, index, offset, ty)) = self.element(pointer)? else {
                    return Err(unsupported("index of a non-pointer"));
                };
                Expr { kind: ExprKind::Load { base, index, offset }, ty }
            }
            Expression::Dereference { pointer } => {
                let pointer = self.expression(pointer)?;
                let Some((base, index, offset, ty)) = self.element(pointer)? else {
                    return Err(unsupported("dereference of a non-scalar pointer"));
                };
                Expr { kind: ExprKind::Load { base, index, offset }, ty }
            }
            Expression::Call { name, arguments } => self.call(name, arguments, false)?,
            Expression::CallThrough { target, arguments, return_type } => {
                self.indirect_call(target, arguments, false, *return_type)?
            }
            Expression::StringLiteral(bytes) => {
                if self.unit.strings_packed {
                    return Err(unsupported("packed string literals"));
                }
                let index = match self.strings.iter().position(|known| known == bytes) {
                    Some(index) => index,
                    None => {
                        self.strings.push(bytes.clone());
                        self.strings.len() - 1
                    }
                };
                Expr { kind: ExprKind::StringAddress(index), ty: Type::Pointer(mwcc_iro::Pointee::Char) }
            }
            Expression::Assign { target, value } => self.assignment_value(target, value)?,
            // `x++` as a value: the old value; the step follows the statement.
            // `(*p)++` / `s->n++` as a value: the old value is loaded into a
            // temporary and `old + 1` stored back before the statement.
            // (A variable in the frame steps like memory.)
            Expression::PostStep { target, operator, pointer_link: None }
                if pure_lvalue(target)
                    && (!matches!(target.as_ref(), Expression::Variable(_))
                        || matches!(target.as_ref(), Expression::Variable(name)
                            if self.names.get(name).map_or(
                                // (A global steps like memory too.)
                                self.unit.globals.contains_key(name) && std::env::var_os("MWCC_IRO_NO_GLOBAL_POST_VALUE").is_none(),
                                |&id| self.variables[id].frame.is_some()))) =>
            {
                // (In call arguments too, when no argument of an enclosing call
                // calls: the step then precedes the calls either way.)
                let in_arguments = self.guarded == self.argument_guards
                    && self.calling_arguments == 0
                    && std::env::var_os("MWCC_IRO_NO_ARGUMENT_POST_STEP").is_none();
                if (self.guarded > 0 && !in_arguments) || std::env::var_os("MWCC_IRO_NO_MEMORY_POST_VALUE").is_some() {
                    return Err(unsupported("expression PostStep"));
                }
                let old = self.expression(target)?;
                let ty = old.ty;
                let id = self.temporary(ty);
                self.pending.push(Stmt::Assign { variable: id, value: old });
                let (place, stored) = self.place(target)?;
                let current = Expr { kind: ExprKind::Var(id), ty };
                let op = if *operator == BinaryOperator::Subtract { BinaryOp::Subtract } else { BinaryOp::Add };
                let stepped = if matches!(ty, Type::Pointer(_) | Type::StructPointer { .. }) {
                    self.pointer_arithmetic(op, current.clone(), Expr::int(1))?
                } else if is_float(ty) {
                    // (A floating value steps by 1.0, operands in source order.)
                    let one = Expr { kind: ExprKind::Float(1.0), ty };
                    if op == BinaryOp::Add && !self.unit.unoptimized {
                        Expr { kind: ExprKind::Idiom(mwcc_iro::Idiom::Update(op, Box::new(current.clone()), Box::new(one))), ty }
                    } else {
                        Expr::binary(op, current.clone(), one, ty)
                    }
                } else {
                    Expr::binary(op, promoted(current.clone()), Expr::int(1), promote(ty))
                };
                self.memory_steps.push(self.pending.len());
                self.pending.push(Stmt::Store { place, ty: stored, value: assigned(stepped, stored), compound: true });
                current
            }
            Expression::PostStep { target, operator, pointer_link: None }
                if matches!(target.as_ref(), Expression::Variable(name)
                    if self.names.get(name).is_some_and(|&id| self.variables[id].frame.is_none())) =>
            {
                if self.guarded > self.argument_guards || std::env::var_os("MWCC_IRO_NO_POST_VALUE").is_some() {
                    return Err(unsupported("expression PostStep"));
                }
                let Expression::Variable(name) = target.as_ref() else { unreachable!() };
                let id = self.names[name];
                let step = Expression::Binary {
                    operator: *operator,
                    left: target.clone(),
                    right: Box::new(Expression::IntegerLiteral(1)),
                };
                let statements = self.assignment(target, &step)?;
                self.post.extend(statements);
                Expr { kind: ExprKind::Var(id), ty: self.variables[id].ty }
            }
            other => return Err(unsupported(format!("expression {}", expression_name(other)))),
        })
    }

    /// The address a member access is based on: a struct pointer value, or
    /// a struct object named directly (`s.m`).
    fn aggregate_address(&mut self, base: &Expression) -> Compilation<Expr> {
        match base {
            Expression::AddressOf { operand } => self.address_of(operand),
            // An element of an embedded array of structs (`s.homes[i].m`).
            Expression::Index { base: array, .. }
                if matches!(array.as_ref(), Expression::Member { member_type: Type::Struct { .. }, .. }) =>
            {
                self.address_of(base)
            }
            // An indexed struct element (`p[i].m`): the element's address;
            // an indexed pointer (`p[i]->m`) is loaded.
            Expression::Index { base: array, index } => {
                let sum = self.pointer_sum(array, index)?;
                if matches!(sum.ty, Type::StructPointer { .. }) {
                    Ok(sum)
                } else {
                    self.expression(base)
                }
            }
            other => self.expression(other),
        }
    }

    /// The struct address a member access is based on; `a[i].m` (with the
    /// struct size as `stride`) addresses `a + i*stride`.
    fn member_base(&mut self, base: &Expression, stride: Option<u32>) -> Compilation<Expr> {
        let Some(stride) = stride else { return self.aggregate_address(base) };
        let Expression::Index { base: array, index } = base else {
            return Err(unsupported("member of an indexed element"));
        };
        // An embedded array of structs (`o->pts[i].y`): the member's address,
        // then the scaled index.
        if let Expression::Member { base: outer, offset, member_type: Type::Struct { size, .. }, index_stride } = array.as_ref() {
            if *size == stride && std::env::var_os("MWCC_IRO_NO_MEMBER_STRUCT_ARRAYS").is_none() {
                // (The member offset last, so it joins the displacement.)
                let outer = self.member_base(outer, *index_stride)?;
                let ty = Type::StructPointer { element_size: stride };
                let index = self.expression(index)?;
                let element = Expr::binary(BinaryOp::Add, Expr { ty, ..outer }, scale(promoted(index), stride), ty);
                return Ok(Expr::binary(BinaryOp::Add, element, Expr::int(i64::from(*offset)), ty));
            }
        }
        let pointer = self.expression(array)?;
        // `a[i]->m` also records a stride: there the element is a pointer
        // to load, and only an array of the structs themselves is indexed.
        if !matches!(pointer.ty, Type::StructPointer { element_size } if element_size == stride) {
            return self.aggregate_address(base);
        }
        let index = self.expression(index)?;
        let ty = Type::StructPointer { element_size: stride };
        let pointer = Expr { ty, ..pointer };
        Ok(Expr::binary(BinaryOp::Add, pointer, scale(promoted(index), stride), ty))
    }

    /// `&operand`.
    fn address_of(&mut self, operand: &Expression) -> Compilation<Expr> {
        match operand {
            Expression::Variable(name) if !self.names.contains_key(name) => {
                let Some(global) = self.unit.globals.get(name) else {
                    return Err(unsupported(format!("unknown variable '{name}'")));
                };
                let ty = pointer_to(global.ty).ok_or_else(|| unsupported("address of this global type"))?;
                // (A folded `static const` has no object to address.)
                if global.folded.is_some() {
                    return Err(unsupported("address of a folded static constant"));
                }
                if let Some(address) = global.fixed_address {
                    return Ok(Expr { kind: ExprKind::Int(address), ty });
                }
                Ok(Expr { kind: ExprKind::GlobalAddress(name.clone()), ty })
            }
            Expression::Variable(name) => match self.names.get(name) {
                Some(&id) if self.variables[id].frame.is_some() => Ok(self.local_address(id)),
                // (A struct parameter is its address.)
                Some(&id) if self.struct_parameters.contains(&id) => {
                    Ok(Expr { kind: ExprKind::Var(id), ty: self.variables[id].ty })
                }
                _ => Err(unsupported("address of a register variable")),
            },
            Expression::Member { base, offset, member_type, index_stride } => {
                let base = self.member_base(base, *index_stride)?;
                let ty = pointer_to(*member_type).ok_or_else(|| unsupported("address of this member type"))?;
                Ok(Expr::binary(BinaryOp::Add, base, Expr::int(i64::from(*offset)), ty))
            }
            // `&o->pts[i]`: an element of an embedded array of structs.
            Expression::Index { base, index }
                if matches!(base.as_ref(), Expression::Member { member_type: Type::Struct { .. }, .. }) =>
            {
                let Expression::Member { member_type: Type::Struct { size, .. }, .. } = base.as_ref() else { unreachable!() };
                self.member_base(operand, Some(*size))
            }
            Expression::Index { base, index } => self.pointer_sum(base, index),
            Expression::Dereference { pointer } => self.expression(pointer),
            other => Err(unsupported(format!("address of {}", expression_name(other)))),
        }
    }

    /// A call; `discarded` when its result is unused (a `void` callee is fine).
    /// A call through a function pointer: `mtctr` + `bctrl`. Its value is
    /// modeled when the pointer's declared return type is known.
    fn indirect_call(
        &mut self,
        target: &Expression,
        arguments: &[Expression],
        discarded: bool,
        return_type: Option<Type>,
    ) -> Compilation<Expr> {
        let ty = match return_type {
            _ if discarded => Type::Void,
            Some(ty) if is_value_type(ty) => ty,
            _ => return Err(unsupported("value of an indirect call")),
        };
        if arguments.len() > ARGUMENT_REGISTERS {
            return Err(unsupported("stack-passed arguments"));
        }
        let target = self.guarded_expression(target)?;
        if !is_general_word(target.ty) {
            return Err(unsupported("call target of this type"));
        }
        let mut values = vec![target];
        let calling = arguments.iter().any(|argument| self.calls_outside_expansions(argument));
        self.calling_arguments += usize::from(calling);
        let evaluated: Compilation<()> = (|| {
            for argument in arguments {
                // (A struct by value: a caller copy, by address.)
                if let Some((size, align)) = self.struct_shape(argument).filter(|_| std::env::var_os("MWCC_IRO_NO_INDIRECT_STRUCT_ARGUMENTS").is_none()) {
                    values.push(self.struct_argument(argument, size, align)?);
                    continue;
                }
                let value = promoted(self.argument_expression(argument)?);
                // (Floating arguments go in f1.., as for a direct call.)
                let floating = is_float(value.ty) && std::env::var_os("MWCC_IRO_NO_INDIRECT_FLOAT_ARGUMENTS").is_none();
                if (is_float(value.ty) && !floating) || !is_value_type(value.ty) {
                    return Err(unsupported("indirect call argument of this type"));
                }
                values.push(value);
            }
            Ok(())
        })();
        self.calling_arguments -= usize::from(calling);
        evaluated?;
        Ok(Expr { kind: ExprKind::Call { name: mwcc_iro::INDIRECT_CALL.to_owned(), arguments: values }, ty })
    }

    /// Whether an argument calls a function MWCC does not expand inline
    /// (an expansion's body runs before the statement either way).
    fn calls_outside_expansions(&self, argument: &Expression) -> bool {
        let listing = format!("{argument:?}");
        if std::env::var_os("MWCC_IRO_EXPANSIONS_COUNT_AS_CALLS").is_some() {
            return listing.contains("Call");
        }
        if listing.contains("CallThrough") || listing.contains("VirtualCall") {
            return true;
        }
        listing.match_indices("Call { name: \"").any(|(at, head)| {
            let rest = &listing[at + head.len()..];
            let name = &rest[..rest.find('"').unwrap_or(0)];
            !self.unit.inline_bodies.contains_key(name)
        })
    }

    /// A call MWCC expands inline: the arguments are assigned to the
    /// callee's parameters, its body (renumbered into this function's
    /// temporaries) runs before the calling statement, and the call's value
    /// is the callee's final return value.
    fn inline_call(&mut self, callee: &ast::Function, arguments: &[Expression]) -> Compilation<Expr> {
        self.expansions += 1;
        thread_local! {
            static DEPTH: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
        }
        // An expansion that is the returned value leaves its result in a
        // temporary (not a -O0 register variable).
        let direct_return = std::mem::take(&mut self.direct_return);
        // (In call arguments too, when no argument of an enclosing call
        // calls: the expansion then precedes the calls either way.)
        let in_arguments = self.guarded == self.argument_guards
            && self.calling_arguments == 0
            && std::env::var_os("MWCC_IRO_NO_ARGUMENT_EXPANSIONS").is_none();
        if self.guarded > 0 && !in_arguments {
            return Err(unsupported("inline expansion in a conditional operand"));
        }
        if callee.parameters.len() != arguments.len() {
            return Err(unsupported("inline expansion with mismatched arguments"));
        }
        if DEPTH.with(|depth| depth.get()) >= 4 {
            return Err(unsupported("nested inline expansion"));
        }
        DEPTH.with(|depth| depth.set(depth.get() + 1));
        let built = build_unoptimized(callee, self.unit);
        DEPTH.with(|depth| depth.set(depth.get() - 1));
        let built = built?;
        let mut inlined = built.function;
        // The expansion's string literals join the caller's.
        if !inlined.strings.is_empty() {
            let indices: Vec<usize> = inlined
                .strings
                .iter()
                .map(|bytes| match self.strings.iter().position(|known| known == bytes) {
                    Some(index) => index,
                    None => {
                        self.strings.push(bytes.clone());
                        self.strings.len() - 1
                    }
                })
                .collect();
            passes::map_strings(&mut inlined.body, &|index| indices[index]);
        }
        if inlined.variables.iter().any(|variable| variable.frame.is_some())
            && ((!inlined.images.is_empty() && std::env::var_os("MWCC_IRO_NO_INLINE_IMAGES").is_some())
                || std::env::var_os("MWCC_IRO_NO_INLINE_FRAME_OBJECTS").is_some())
        {
            return Err(unsupported("inline expansion with frame objects"));
        }
        // (So do its initializer images.)
        if !inlined.images.is_empty() {
            let base = self.images.len();
            self.images.extend(std::mem::take(&mut inlined.images));
            passes::map_images(&mut inlined.body, &|index| index + base);
        }
        // Only a final return value: an early return would need a jump.
        let result = match inlined.body.last() {
            Some(Stmt::SetReturn(_)) => inlined.body.pop().map(|statement| match statement {
                Stmt::SetReturn(value) => value,
                _ => unreachable!("matched above"),
            }),
            _ => None,
        };
        // Early returns: the body becomes `do { ... } while (0)`, each
        // `return v` an assignment of the result and a `break` (not from
        // inside a loop or switch, where `break` means something else).
        let early = built.returns_through_variable || returns_anywhere(&inlined.body);
        // (A return inside a loop or switch jumps to the expansion's end.)
        let jumps = early && returns_in_breakable(&inlined.body);
        if early && (std::env::var_os("MWCC_IRO_NO_INLINE_EARLY_RETURN").is_some() || jumps && std::env::var_os("MWCC_IRO_NO_INLINE_RETURN_JUMPS").is_some()) {
            return Err(unsupported("inline expansion with an early return"));
        }
        // At -O0 the expansion's variables are register variables.
        let kind = if self.unit.unoptimized { VariableKind::Local } else { VariableKind::Temporary };
        let base = self.variables.len() + self.temporaries.len();
        for variable in &inlined.variables {
            // (A frame object stays one: a local of the caller's frame.)
            self.temporaries.push(Variable {
                name: format!("{}${}", callee.name, variable.name),
                ty: variable.ty,
                kind: if variable.frame.is_some() { VariableKind::Local } else { kind },
                frame: variable.frame,
                initialized: false,
            raw: false,
            volatile: false,
            });
        }
        // Arguments are evaluated in order into the parameters; a constant
        // argument of a parameter the body never assigns is propagated.
        // The return value is renumbered and substituted with the body.
        let returns_value = result.is_some();
        if let Some(value) = result {
            inlined.body.push(Stmt::SetReturn(value));
        }
        let map = |id: VarId| id + base;
        passes::map_variables(&mut inlined.body, &map);
        // (As for a call: the arguments that call are evaluated first.)
        let calls = |argument: &Expression| format!("{argument:?}").contains("Call");
        let mut order: Vec<usize> = (0..arguments.len()).filter(|&index| calls(&arguments[index])).collect();
        order.extend((0..arguments.len()).filter(|&index| !calls(&arguments[index])));
        if std::env::var_os("MWCC_IRO_INLINE_ARGUMENTS_IN_ORDER").is_some() {
            order = (0..arguments.len()).collect();
        }
        for index in order {
            let argument = &arguments[index];
            let value = self.expression(argument)?;
            let ty = inlined.variables[index].ty;
            let value = assigned(promoted(value), ty);
            let constant = value.as_int().is_some() && !is_float(ty);
            // (And a register variable of the same type.)
            let variable = matches!(value.kind, ExprKind::Var(id)
                if id < self.variables.len() && self.variables[id].frame.is_none() && self.variables[id].ty == ty)
                && std::env::var_os("MWCC_IRO_NO_INLINE_VARIABLES").is_none();
            // (And arithmetic on those, re-evaluated at each use.)
            fn pure(e: &Expr, variables: &[Variable]) -> bool {
                match &e.kind {
                    // (A frame object's address is a constant too.)
                    ExprKind::Int(_) => true,
                    ExprKind::LocalAddress(_) => std::env::var_os("MWCC_IRO_NO_INLINE_ADDRESSES").is_none(),
                    ExprKind::Var(id) => *id < variables.len() && variables[*id].frame.is_none(),
                    ExprKind::Binary(op, a, b) => {
                        !matches!(op, BinaryOp::Divide | BinaryOp::Modulo | BinaryOp::LogicalAnd | BinaryOp::LogicalOr)
                            && pure(a, variables)
                            && pure(b, variables)
                    }
                    ExprKind::Unary(_, a) | ExprKind::Convert(a) => pure(a, variables),
                    _ => false,
                }
            }
            let expression = !is_float(ty)
                && !is_float(value.ty)
                && pure(&value, &self.variables)
                && std::env::var_os("MWCC_IRO_NO_INLINE_EXPRESSIONS").is_none();
            if (constant || variable || expression)
                && !body_assigns(&inlined.body, base + index)
                && std::env::var_os("MWCC_IRO_NO_INLINE_CONSTANTS").is_none()
            {
                // (Converted to the parameter's type where it differs.)
                let substituted = if expression && !constant && value.ty != ty {
                    Expr { kind: ExprKind::Convert(Box::new(value.clone())), ty }
                } else {
                    Expr { kind: value.kind.clone(), ty }
                };
                passes::substitute(&mut inlined.body, base + index, &substituted);
                continue;
            }
            // (A function's address: calls through the parameter call it.)
            if let ExprKind::GlobalAddress(callee_name) = &value.kind {
                if !body_assigns(&inlined.body, base + index) && std::env::var_os("MWCC_IRO_NO_INLINE_DIRECT_CALLS").is_none() {
                    let parameter = base + index;
                    let name = callee_name.clone();
                    passes::for_each_expression(&mut inlined.body, &mut |e| direct_calls(e, parameter, &name));
                    if references_variable(&inlined.body, parameter) {
                        self.pending.push(Stmt::Assign { variable: parameter, value });
                    }
                    continue;
                }
            }
            self.pending.push(Stmt::Assign { variable: base + index, value });
        }
        let value = match returns_value.then(|| inlined.body.pop()).flatten() {
            Some(Stmt::SetReturn(value)) => Some(value),
            _ => None,
        };
        if early {
            let ty = callee.return_type;
            let id = self.variables.len() + self.temporaries.len();
            self.temporaries.push(Variable { name: format!("{}$result", callee.name), ty, kind, frame: None, initialized: false, raw: false, volatile: false });
            let result = (ty != Type::Void).then_some(id);
            if jumps {
                let label = format!("@inline_end{id}");
                let mut body = returns_to_label(std::mem::take(&mut inlined.body), result, ty, &label);
                if let (Some(id), Some(value)) = (result, value) {
                    body.push(Stmt::Assign { variable: id, value: assigned(value, ty) });
                }
                body.push(Stmt::Label(label));
                self.pending.extend(body);
                return Ok(match result {
                    Some(id) => Expr { kind: ExprKind::Var(id), ty },
                    None => Expr { kind: ExprKind::Int(0), ty: Type::Void },
                });
            }
            let mut body = early_returns(std::mem::take(&mut inlined.body), result, ty);
            if let (Some(id), Some(value)) = (result, value) {
                body.push(Stmt::Assign { variable: id, value: assigned(value, ty) });
            }
            self.pending.push(Stmt::Loop {
                test_first: false,
                condition: Some(Expr::int(0)),
                body,
                step: Vec::new(),
                effects: Vec::new(),
            });
            return Ok(match result {
                Some(id) => Expr { kind: ExprKind::Var(id), ty },
                None => Expr { kind: ExprKind::Int(0), ty: Type::Void },
            });
        }
        let body_was_empty = inlined.body.is_empty() && !std::env::var_os("MWCC_IRO_INLINE_VALUE_TEMPORARY").is_some();
        self.pending.extend(inlined.body);
        let inlined = mwcc_iro::Function { body: Vec::new(), ..inlined };
        let Some(value) = value else {
            return Ok(Expr { kind: ExprKind::Int(0), ty: Type::Void });
        };
        // (Optimized, an expansion that is only its value is that value.)
        let only_value = !self.unit.unoptimized && inlined.body.is_empty() && body_was_empty;
        if direct_return || only_value {
            // (Still converted to the callee's return type.)
            return Ok(converted(value, callee.return_type));
        }
        // The value is held in a temporary of the callee's return type.
        let id = self.variables.len() + self.temporaries.len();
        self.temporaries.push(Variable {
            name: format!("{}$result", callee.name),
            ty: callee.return_type,
            kind,
            frame: None,
            initialized: false,
            raw: false,
        volatile: false,
        });
        self.pending.push(Stmt::Assign { variable: id, value: assigned(value, callee.return_type) });
        Ok(Expr { kind: ExprKind::Var(id), ty: callee.return_type })
    }

    /// A compiler intrinsic: `__rlwimi` inserts, `__cntlzw` counts,
    /// `__fabs` takes the absolute value.
    fn intrinsic(&mut self, name: &str, arguments: &[Expression]) -> Compilation<Expr> {
        match (name, arguments) {
            ("__rlwimi", [base, value, shift, begin, end]) => {
                let constant = |e: &Expression| match e {
                    Expression::IntegerLiteral(v) if (0..32).contains(v) => Some(*v as u8),
                    _ => None,
                };
                let (Some(shift), Some(begin), Some(end)) = (constant(shift), constant(begin), constant(end)) else {
                    return Err(unsupported("intrinsic '__rlwimi' with variable fields"));
                };
                let base = self.expression(base)?;
                let value = self.expression(value)?;
                Ok(Expr {
                    kind: ExprKind::Idiom(Idiom::Insert {
                        base: Box::new(base),
                        value: Box::new(value),
                        shift,
                        begin,
                        end,
                    }),
                    ty: Type::Int,
                })
            }
            ("__cntlzw", [value]) => {
                let value = self.expression(value)?;
                Ok(Expr { kind: ExprKind::Idiom(Idiom::Unary(IntrinsicOp::CountLeadingZeros, Box::new(value))), ty: Type::Int })
            }
            ("__sync" | "__isync" | "__eieio", []) => {
                let op = match name {
                    "__sync" => IntrinsicOp::Synchronize,
                    "__isync" => IntrinsicOp::InstructionSynchronize,
                    _ => IntrinsicOp::EnforceInOrderIo,
                };
                Ok(Expr { kind: ExprKind::Idiom(Idiom::Unary(op, Box::new(Expr::int(0)))), ty: Type::Void })
            }
            ("__fabs", [value]) => {
                let value = converted(self.expression(value)?, Type::Double);
                Ok(Expr { kind: ExprKind::Idiom(Idiom::Unary(IntrinsicOp::FloatAbsolute, Box::new(value))), ty: Type::Double })
            }
            _ => Err(unsupported(format!("intrinsic '{name}'"))),
        }
    }

    fn call(&mut self, name: &str, arguments: &[Expression], discarded: bool) -> Compilation<Expr> {
        // A variadic definition's `__builtin_va_info(&ap)`.
        if let ("__builtin_va_info", [list]) = (name, arguments) {
            if !self.unit.variadic {
                return Err(unsupported("__builtin_va_info outside a variadic definition"));
            }
            let address = self.expression(list)?;
            // (Three words: the named register counts, the caller's argument
            // area, the register save area.)
            let parameters: Vec<Type> =
                self.variables.iter().filter(|variable| variable.kind == VariableKind::Parameter).map(|variable| variable.ty).collect();
            let generals = parameters.iter().filter(|&&ty| !is_float(ty)).count() as i64;
            let floats = parameters.iter().filter(|&&ty| is_float(ty)).count() as i64;
            let pointer = Type::Pointer(mwcc_iro::Pointee::Char);
            let area = |op| Expr { kind: ExprKind::Idiom(Idiom::Unary(op, Box::new(Expr::int(0)))), ty: pointer };
            for (offset, value) in [
                (0, Expr::typed_int((generals << 24) | (floats << 16), Type::Int)),
                (4, area(IntrinsicOp::VaIncoming)),
                (8, area(IntrinsicOp::VaSaveArea)),
            ] {
                self.pending.push(Stmt::Store {
                    place: Place::Memory { base: Box::new(address.clone()), index: None, offset },
                    ty: if offset == 0 { Type::Int } else { pointer },
                    value,
                    compound: false,
                });
            }
            return Ok(Expr::int(0));
        }
        let direct_return = std::mem::take(&mut self.direct_return);
        // A call through a pointer variable (local, parameter or global).
        let pointer_variable = self.names.contains_key(name)
            || self.unit.globals.get(name).is_some_and(|global| !global.is_function);
        if pointer_variable {
            let owner = if self.names.contains_key(name) { self.function_name } else { "" };
            let return_type = (self.unit.pointer_return_type)(owner, name);
            return self.indirect_call(&Expression::Variable(name.to_owned()), arguments, discarded, return_type);
        }
        {
            {
                let ty = self.unit.call_return_types.get(name).copied().unwrap_or(Type::Int);
                if !is_value_type(ty) && !is_wide(ty) && !(discarded && ty == Type::Void) {
                    return Err(unsupported("non-integer call result"));
                }
                if (self.unit.is_intrinsic)(name, arguments.len()) {
                    return self.intrinsic(name, arguments);
                }
                let variadic = self.unit.variadic_callees.contains(name);
                let prototyped = self.unit.prototyped.contains(name);
                if (self.unit.has_body)(name) {
                    return match self.unit.inline_bodies.get(name).copied() {
                        Some(callee) if std::env::var_os("MWCC_IRO_NO_INLINE").is_none() => {
                            self.direct_return = direct_return;
                            self.inline_call(callee, arguments)
                        }
                        _ => Err(unsupported("call to a function this unit defines (inlining not modeled)")),
                    };
                }
                if arguments.len() > 2 * ARGUMENT_REGISTERS {
                    return Err(unsupported("stack-passed arguments"));
                }
                // An expanded call among arguments that are otherwise literals
                // or register variables may run its body first.
                let simple = |builder: &Self, argument: &Expression| match argument {
                    Expression::IntegerLiteral(_) | Expression::FloatLiteral(_) => true,
                    Expression::Variable(name) => {
                        builder.names.get(name).is_some_and(|&id| builder.variables[id].frame.is_none())
                    }
                    _ => false,
                };
                let parameter_types = self.unit.call_parameter_types.get(name).cloned();
                let mut values = Vec::with_capacity(arguments.len());
                // (A step hoisted out of an argument stays ahead of no call.)
                let calling = arguments.iter().any(|argument| self.calls_outside_expansions(argument));
                self.calling_arguments += usize::from(calling);
                for (index, argument) in arguments.iter().enumerate() {
                    // A struct passed by value: a caller copy, by address.
                    match parameter_types.as_ref().and_then(|types| types.get(index)).copied() {
                        Some(Type::Struct { size, align }) if !variadic || index < parameter_types.as_ref().map_or(0, Vec::len) => {
                            values.push(self.struct_argument(argument, size, align)?);
                            continue;
                        }
                        // (A struct-array object (`va_list ap`) passed to a pointer
                        // parameter decays to its address.)
                        Some(Type::StructPointer { .. } | Type::Pointer(_))
                            if self.struct_valued(argument) && std::env::var_os("MWCC_IRO_NO_STRUCT_DECAY").is_none() =>
                        {
                            values.push(self.address_of(argument)?);
                            continue;
                        }
                        _ if self.struct_valued(argument) => return Err(unsupported("struct argument without a struct parameter")),
                        _ => {}
                    }
                    let expanded = matches!(argument, Expression::Call { name, .. } if self.unit.inline_bodies.contains_key(name));
                    let others_simple = arguments.iter().enumerate().all(|(other, value)| other == index || simple(self, value));
                    values.push(if expanded && others_simple && self.guarded == 0 {
                        self.expression(argument)?
                    } else {
                        self.argument_expression(argument)?
                    });
                }
                self.calling_arguments -= usize::from(calling);
                let mut arguments = values;
                if variadic {
                    // Arguments past the fixed parameters take the default
                    // promotions (floating ones widen to double).
                    let fixed = self.unit.call_parameter_types.get(name).map_or(0, |types| types.len());
                    for argument in arguments.iter_mut().skip(fixed) {
                        let value = std::mem::replace(argument, Expr::int(0));
                        *argument = if is_float(value.ty) { converted(value, Type::Double) } else { promoted(value) };
                    }
                }
                if !prototyped {
                    // Default argument promotions (a floating one widens to
                    // double; MWCC sets no CR bit for these calls).
                    let arguments = arguments
                        .into_iter()
                        .map(|argument| if is_float(argument.ty) { converted(argument, Type::Double) } else { promoted(argument) })
                        .collect();
                    return Ok(Expr { kind: ExprKind::Call { name: name.to_owned(), arguments }, ty });
                }
                // The caller converts an argument to a narrow parameter's type.
                if let Some(types) = self.unit.call_parameter_types.get(name) {
                    for (argument, &parameter) in arguments.iter_mut().zip(types) {
                        if is_narrow(parameter)
                            && argument.ty != parameter
                            && !passes::fits_unconverted(argument, parameter)
                        {
                            let value = std::mem::replace(argument, Expr::int(0));
                            *argument = Expr { kind: ExprKind::Convert(Box::new(value)), ty: parameter };
                        } else if is_float(parameter) || is_float(argument.ty) {
                            let value = std::mem::replace(argument, Expr::int(0));
                            *argument = converted(value, parameter);
                        } else if is_wide(parameter) || is_wide(argument.ty) {
                            // (To or from `long long`: converted.)
                            let value = std::mem::replace(argument, Expr::int(0));
                            *argument = converted(promoted(value), parameter);
                        } else if !is_narrow(parameter) {
                            let value = std::mem::replace(argument, Expr::int(0));
                            *argument = promoted(value);
                        }
                    }
                }
                if arguments.iter().any(|argument| !is_value_type(argument.ty) && !is_wide(argument.ty)) {
                    return Err(unsupported("argument type"));
                }
                let floats = arguments.iter().filter(|argument| is_float(argument.ty)).count();
                // (A wide argument takes an odd-aligned register pair.)
                let generals = arguments.iter().filter(|argument| !is_float(argument.ty)).fold(0usize, |used, argument| {
                    if is_wide(argument.ty) { used + used % 2 + 2 } else { used + 1 }
                });
                // (Words past r10 go to the outgoing argument area.)
                let words_on_stack = generals > ARGUMENT_REGISTERS
                    && !arguments.iter().any(|argument| is_wide(argument.ty))
                    && std::env::var_os("MWCC_IRO_NO_STACK_ARGUMENTS").is_none();
                if floats > ARGUMENT_REGISTERS || (generals > ARGUMENT_REGISTERS && !words_on_stack) {
                    return Err(unsupported("stack-passed arguments"));
                }
                Ok(Expr { kind: ExprKind::Call { name: name.to_owned(), arguments }, ty })
            }
        }
    }
}

/// `index * size` for pointer arithmetic, folded when the index is constant.
fn scale(index: Expr, size: u32) -> Expr {
    if size == 1 {
        return index;
    }
    match index.as_int() {
        Some(value) => Expr::int(value * i64::from(size)),
        None => {
            let ty = promote(index.ty);
            Expr::binary(BinaryOp::Multiply, index, Expr::int(i64::from(size)), ty)
        }
    }
}

/// The usual arithmetic conversion of two integer operands.
/// MWCC subtracts a signed word converted to (signed) `long long` with a
/// zero high word: the conversion zero-extends there.
fn wide_subtrahend(right: Expr, ty: Type) -> Expr {
    // (Only a signed difference: an unsigned one sign-extends the word.)
    if ty != Type::LongLong || std::env::var_os("MWCC_IRO_WIDE_SUBTRAHEND_EXTENDED").is_some() {
        return right;
    }
    let word = match &right.kind {
        ExprKind::Convert(word) if right.ty == Type::LongLong && !is_wide(word.ty) => (**word).clone(),
        _ if !is_wide(right.ty) => right.clone(),
        _ => return right,
    };
    if is_unsigned(promote(word.ty)) || is_float(word.ty) || word.as_int().is_some() {
        return right;
    }
    let word = promoted(word);
    let unsigned = Expr { kind: ExprKind::Convert(Box::new(word)), ty: Type::UnsignedInt };
    Expr { kind: ExprKind::Convert(Box::new(unsigned)), ty }
}

fn arithmetic_type(left: Type, right: Type) -> Type {
    let (left, right) = (promote(left), promote(right));
    // (`long long` wins; unsigned when either is unsigned `long long`.)
    if is_wide(left) || is_wide(right) {
        return if left == Type::UnsignedLongLong || right == Type::UnsignedLongLong {
            Type::UnsignedLongLong
        } else {
            Type::LongLong
        };
    }
    if matches!(left, Type::Pointer(_) | Type::StructPointer { .. }) {
        return left;
    }
    if is_unsigned(left) || is_unsigned(right) {
        Type::UnsignedInt
    } else {
        left
    }
}

fn binary_op(operator: BinaryOperator) -> BinaryOp {
    match operator {
        BinaryOperator::Add => BinaryOp::Add,
        BinaryOperator::Subtract => BinaryOp::Subtract,
        BinaryOperator::Multiply => BinaryOp::Multiply,
        BinaryOperator::Divide => BinaryOp::Divide,
        BinaryOperator::Modulo => BinaryOp::Modulo,
        BinaryOperator::BitAnd => BinaryOp::BitAnd,
        BinaryOperator::BitOr => BinaryOp::BitOr,
        BinaryOperator::BitXor => BinaryOp::BitXor,
        BinaryOperator::ShiftLeft => BinaryOp::ShiftLeft,
        BinaryOperator::ShiftRight => BinaryOp::ShiftRight,
        BinaryOperator::Less => BinaryOp::Less,
        BinaryOperator::Greater => BinaryOp::Greater,
        BinaryOperator::LessEqual => BinaryOp::LessEqual,
        BinaryOperator::GreaterEqual => BinaryOp::GreaterEqual,
        BinaryOperator::Equal => BinaryOp::Equal,
        BinaryOperator::NotEqual => BinaryOp::NotEqual,
        BinaryOperator::LogicalAnd => BinaryOp::LogicalAnd,
        BinaryOperator::LogicalOr => BinaryOp::LogicalOr,
    }
}

fn statement_name(statement: &Statement) -> &'static str {
    match statement {
        Statement::Store { .. } => "store",
        Statement::Assign { .. } => "assign",
        Statement::Expression(_) => "expression",
        Statement::InlineAsm(_) => "inline asm",
        Statement::If { .. } => "if",
        Statement::Return(_) => "return",
        Statement::Switch { .. } => "switch",
        Statement::Break => "break",
        Statement::Continue => "continue",
        Statement::Goto(_) => "goto",
        Statement::Label(_) => "label",
        Statement::Loop { .. } => "loop",
    }
}

/// The variant name of an expression (for refusal diagnostics).
/// A literal initializer's value as `ty` (a scalar), when it is one.
fn literal_value(value: &Expression, ty: Type) -> Option<Expr> {
    fn number(value: &Expression) -> Option<f64> {
        match value {
            Expression::IntegerLiteral(v) => Some(*v as f64),
            Expression::FloatLiteral(v) => Some(*v),
            Expression::Cast { operand, .. } => number(operand),
            Expression::Unary { operator: ast::UnaryOperator::Negate, operand } => number(operand).map(|v| -v),
            _ => None,
        }
    }
    fn integer(value: &Expression) -> Option<i64> {
        match value {
            Expression::IntegerLiteral(v) => Some(*v),
            Expression::Cast { operand, .. } => integer(operand),
            Expression::Unary { operator: ast::UnaryOperator::Negate, operand } => integer(operand).map(|v| -v),
            _ => None,
        }
    }
    match ty {
        Type::Double => Some(Expr { kind: ExprKind::Float(number(value)?), ty }),
        Type::Float => Some(Expr { kind: ExprKind::Float(f64::from(number(value)? as f32)), ty }),
        ty if is_narrow(ty) => Some(Expr::int(integer(value)?)),
        ty if (is_value_type(ty) || is_wide(ty)) && !is_float(ty) => Some(Expr::typed_int(integer(value)?, ty)),
        _ => None,
    }
}

fn expression_name(expression: &Expression) -> String {
    let debug = format!("{expression:?}");
    let end = debug.find([' ', '(', '{']).unwrap_or(debug.len());
    let mut name = debug[..end].to_owned();
    match expression {
        Expression::Cast { target_type, .. } => name.push_str(&format!(" to {target_type:?}")),
        Expression::Member { index_stride: Some(_), .. } => name.push_str(" of an indexed element"),
        _ => {}
    }
    name
}

/// `e` after the integer promotions: a narrow value converts to `int`.
pub fn promoted(e: Expr) -> Expr {
    if is_narrow(e.ty) {
        let ty = promote(e.ty);
        Expr { kind: ExprKind::Convert(Box::new(e)), ty }
    } else {
        e
    }
}

/// A value assigned or stored as `ty`: a narrow value promotes to a word;
/// a floating value (or destination) converts.
fn assigned(value: Expr, ty: Type) -> Expr {
    if is_float(ty) || is_float(value.ty) {
        converted(value, ty)
    } else if is_narrow(ty) {
        value
    } else {
        promoted(value)
    }
}

/// `value` converted to `ty` (unchanged when it already has that type).
fn converted(value: Expr, ty: Type) -> Expr {
    if value.ty == ty {
        value
    } else {
        Expr { kind: ExprKind::Convert(Box::new(value)), ty }
    }
}

/// `pointer + offset` as a memory operand, a constant addend of the pointer
/// folded into the displacement.
fn displaced(pointer: Expr, offset: i32, ty: Type) -> (Box<Expr>, Option<Box<Expr>>, i32, Type) {
    // Look through a conversion between pointer types.
    if let ExprKind::Convert(operand) = &pointer.kind {
        if matches!(operand.ty, Type::Pointer(_) | Type::StructPointer { .. }) {
            return displaced((**operand).clone(), offset, ty);
        }
    }
    if let ExprKind::Binary(op @ (BinaryOp::Add | BinaryOp::Subtract), base, addend) = &pointer.kind {
        if let Some(value) = addend.as_int() {
            let value = if *op == BinaryOp::Subtract { -value } else { value };
            if let Ok(total) = i16::try_from(i64::from(offset) + value) {
                if matches!(base.ty, Type::Pointer(_) | Type::StructPointer { .. }) {
                    return displaced((**base).clone(), i32::from(total), ty);
                }
            }
        }
    }
    // `p + i` at displacement 0 is an indexed access.
    if let (ExprKind::Binary(BinaryOp::Add, base, index), 0) = (&pointer.kind, offset) {
        if matches!(base.ty, Type::Pointer(_) | Type::StructPointer { .. })
            && element_size(index.ty).is_none()
            && index.as_int().is_none()
            && std::env::var_os("MWCC_IRO_NO_MEMBER_INDEXED").is_none()
        {
            return (base.clone(), Some(index.clone()), 0, ty);
        }
    }
    (Box::new(pointer), None, offset, ty)
}

/// Names whose address the function takes (`&name`).
fn addresses_taken(function: &ast::Function, early: bool) -> std::collections::HashSet<String> {
    fn expression(e: &Expression, out: &mut std::collections::HashSet<String>) {
        if let Expression::AddressOf { operand } = e {
            if let Expression::Variable(name) = operand.as_ref() {
                out.insert(name.clone());
            }
        }
        match e {
            Expression::Binary { left, right, .. } => {
                expression(left, out);
                expression(right, out);
            }
            Expression::Unary { operand, .. }
            | Expression::Cast { operand, .. }
            | Expression::AddressOf { operand } => expression(operand, out),
            Expression::Dereference { pointer } => expression(pointer, out),
            Expression::Index { base, index } => {
                expression(base, out);
                expression(index, out);
            }
            Expression::Member { base, .. } | Expression::MemberAddress { base, .. } => expression(base, out),
            Expression::Conditional { condition, when_true, when_false, .. } => {
                expression(condition, out);
                expression(when_true, out);
                expression(when_false, out);
            }
            Expression::Call { arguments, .. } => arguments.iter().for_each(|a| expression(a, out)),
            Expression::Assign { target, value } => {
                expression(target, out);
                expression(value, out);
            }
            Expression::Comma { left, right } => {
                expression(left, out);
                expression(right, out);
            }
            Expression::IndexedUpdateValue { value } => expression(value, out),
            Expression::PostStep { target, .. } => expression(target, out),
            _ => {}
        }
    }
    // (An address stored only into a local nothing reads is dropped with
    // it: `REF_track = &track;`.)
    let listing = format!("{:?}", function.statements);
    let unread = |name: &str| {
        !early
            && std::env::var_os("MWCC_IRO_DEAD_ADDRESSES_TAKEN").is_none()
            && function.locals.iter().any(|local| local.name == name && local.initializer.is_none() && !local.is_static)
            && !listing.contains(&format!("Variable({name:?})"))
    };
    fn statements(body: &[Statement], out: &mut std::collections::HashSet<String>, unread: &dyn Fn(&str) -> bool) {
        for statement in body {
            match statement {
                Statement::Assign { name, value: Expression::AddressOf { operand } }
                    if matches!(operand.as_ref(), Expression::Variable(_)) && unread(name) => {}
                Statement::Assign { value, .. } => expression(value, out),
                Statement::Store { target, value } => {
                    expression(target, out);
                    expression(value, out);
                }
                Statement::Expression(e) => expression(e, out),
                Statement::If { condition, then_body, else_body } => {
                    expression(condition, out);
                    statements(then_body, out, unread);
                    statements(else_body, out, unread);
                }
                Statement::Return(Some(e)) => expression(e, out),
                Statement::Loop { initializer, condition, step, body, .. } => {
                    for e in [initializer, condition, step].into_iter().flatten() {
                        expression(e, out);
                    }
                    statements(body, out, unread);
                }
                Statement::Switch { scrutinee, arms, default } => {
                    expression(scrutinee, out);
                    for body in arms.iter().map(|arm| &arm.body).chain(default.iter()) {
                        match body {
                            ast::ArmBody::Return(value) => expression(value, out),
                            ast::ArmBody::Statements(body) => statements(body, out, unread),
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = std::collections::HashSet::new();
    statements(&function.statements, &mut out, &unread);
    for guard in &function.guards {
        expression(&guard.condition, &mut out);
        expression(&guard.value, &mut out);
    }
    if let Some(value) = &function.return_expression {
        expression(value, &mut out);
    }
    for local in &function.locals {
        if let Some(initializer) = &local.initializer {
            expression(initializer, &mut out);
        }
    }
    out
}

/// Calls through `parameter` (bound to `name`'s address) call `name`.
fn direct_calls(e: &mut Expr, parameter: VarId, name: &str) {
    passes::children(e, &mut |child| direct_calls(child, parameter, name));
    if let ExprKind::Call { name: called, arguments } = &mut e.kind {
        if called == mwcc_iro::INDIRECT_CALL && arguments.first().and_then(Expr::as_var) == Some(parameter) {
            arguments.remove(0);
            *called = name.to_owned();
        }
    }
}

/// Whether any expression of `body` reads `variable`.
fn references_variable(body: &[Stmt], variable: VarId) -> bool {
    let mut found = false;
    let mut body = body.to_vec();
    passes::for_each_expression(&mut body, &mut |e| found |= e.mentions(variable));
    found
}


/// Marks a bit-field read's extraction: its right-shift count literal is
/// typed `UnsignedChar` (see `mwcc_iro::BIT_FIELD_SHIFT`), so the lowering
/// can extract the field as MWCC does.
fn mark_bit_field_shift(expression: &mut Expr) {
    match &mut expression.kind {
        ExprKind::Binary(BinaryOp::ShiftRight, _, count) if count.as_int().is_some() => count.ty = mwcc_iro::BIT_FIELD_SHIFT,
        ExprKind::Binary(_, left, _) => mark_bit_field_shift(left),
        ExprKind::Convert(operand) => mark_bit_field_shift(operand),
        _ => {}
    }
}
