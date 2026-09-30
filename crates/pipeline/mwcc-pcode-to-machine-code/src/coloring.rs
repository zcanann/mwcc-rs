//! MWCC register coloring, ported from the recovered `SpillCode.c` and
//! `Coloring.c` of GC/1.2.5 (github.com/JackPriceBurns/mwcc, CC0).
//!
//! Per register class: block liveness (with the function result seeded as a
//! use of the exit block), backward interference construction over a
//! triangular bit matrix, dead-definition removal, copy coalescing into the
//! minimum-numbered root, then Chaitin simplify/select:
//!
//! - simplify repeatedly scans virtual registers upward from 32, removing any
//!   uncoalesced node whose degree is below the free-register count; when none
//!   is removable the minimum spill-cost/degree node is removed;
//! - select pops the stack, taking the lowest free color of the class's
//!   volatile mask ({r0, r3..r12} / {f0..f13}) and claiming callee-saved
//!   registers downward from 31 only when the mask is exhausted.
//!
//! The triangular index of a register paired with itself equals its index
//! paired with register 0; MWCC uses that bit to forbid r0 (a D-form base or
//! `addi` source), and this port keeps the representation for that reason.

use mwcc_core::{Compilation, Diagnostic};
use mwcc_pcode::{Block, Class, PCodeFunction, ReturnRegisters, FIRST_VIRTUAL};

/// Outcome of coloring one function.
#[derive(Debug, Clone, Default)]
pub struct ColoringResult {
    /// Callee-saved general registers claimed (descending from r31).
    pub saved_general: Vec<u32>,
    /// Callee-saved float registers claimed (descending from f31).
    pub saved_float: Vec<u32>,
}

struct ClassPolicy {
    class: Class,
    /// Physical registers never available for coloring.
    used: &'static [u32],
    /// Highest register of the initial (volatile) color mask.
    mask_last: u32,
    /// Lowest callee-saved register that may be claimed.
    claim_floor: u32,
}

const GENERAL: ClassPolicy = ClassPolicy {
    class: Class::General,
    used: &[1, 2, 13],
    mask_last: 12,
    claim_floor: 14,
};

const FLOAT: ClassPolicy = ClassPolicy {
    class: Class::Float,
    used: &[],
    mask_last: 13,
    claim_floor: 14,
};

/// Color every virtual register of `function` in place.
pub fn color(function: &mut PCodeFunction, delete_dead: bool) -> Compilation<ColoringResult> {
    let mut result = ColoringResult::default();
    // MWCC colors VRs, then GPRs, then FPRs.
    result.saved_general = color_class(function, &GENERAL, delete_dead)?;
    result.saved_float = color_class(function, &FLOAT, delete_dead)?;
    Ok(result)
}

struct Bits(Vec<u32>);

impl Bits {
    fn new(count: usize) -> Self {
        Bits(vec![0; count.div_ceil(32)])
    }
    fn get(&self, index: usize) -> bool {
        self.0[index >> 5] & (1 << (index & 31)) != 0
    }
    fn set(&mut self, index: usize) {
        self.0[index >> 5] |= 1 << (index & 31);
    }
    fn clear(&mut self, index: usize) {
        self.0[index >> 5] &= !(1 << (index & 31));
    }
}

/// Triangular interference matrix with MWCC's index function.
struct Matrix {
    bits: Bits,
}

impl Matrix {
    fn new(count: usize) -> Self {
        // Every index `larger²/2 + smaller` stays below `count²/2 + count`.
        Matrix { bits: Bits::new(count * count / 2 + count + 1) }
    }

    fn index(first: usize, second: usize) -> usize {
        if first == second {
            return first * first / 2;
        }
        let (larger, smaller) = if first > second { (first, second) } else { (second, first) };
        larger * larger / 2 + smaller
    }

    fn interferes(&self, first: usize, second: usize) -> bool {
        first != second && self.bits.get(Self::index(first, second))
    }

    fn set(&mut self, first: usize, second: usize) {
        if first != second {
            self.bits.set(Self::index(first, second));
        }
    }

    fn set_raw(&mut self, first: usize, second: usize) {
        self.bits.set(Self::index(first, second));
    }
}

struct Node {
    degree: i32,
    neighbors: Vec<usize>,
    /// Color once selected; for a coalesced child, its root register.
    physical: i32,
    coalesced: bool,
    simplified: bool,
    spill_cost: i64,
}

fn color_class(
    function: &mut PCodeFunction,
    policy: &ClassPolicy,
    delete_dead: bool,
) -> Compilation<Vec<u32>> {
    let class = policy.class;
    let count = function.register_count(class) as usize;
    if count <= FIRST_VIRTUAL as usize {
        return Ok(Vec::new());
    }

    let live_out = solve_liveness(function, class, count);
    if delete_dead {
        remove_dead_definitions(function, class, &live_out);
    }
    let live_out = solve_liveness(function, class, count);
    let mut matrix = construct_interference(function, class, count, &live_out);
    // `-O0` register variables occupy their registers everywhere.
    let everywhere = if class == Class::General { &function.exit_uses } else { &function.exit_float_uses };
    for &register in everywhere {
        for virtual_register in FIRST_VIRTUAL as usize..count {
            matrix.set(virtual_register, register as usize);
        }
    }
    let roots = coalesce_copies(function, class, count, &mut matrix);
    let mut nodes = materialize(count, &matrix, &roots);
    for (register, node) in nodes.iter_mut().enumerate().take(FIRST_VIRTUAL as usize) {
        node.physical = register as i32;
    }

    let available = 32 - policy.used.len() as i32;
    let stack = simplify(function, class, &mut nodes, available);
    let saved = select(&mut nodes, &stack, policy)?;
    commit(function, class, &nodes);
    Ok(saved)
}

fn operand_registers(function: &PCodeFunction) -> impl Iterator<Item = &mwcc_pcode::PInstr> {
    function.blocks.iter().flat_map(|block| block.instructions.iter())
}

/// Block-level liveness; returns each block's live-out set.
fn solve_liveness(function: &PCodeFunction, class: Class, count: usize) -> Vec<Bits> {
    let blocks = &function.blocks;
    let mut uses: Vec<Bits> = (0..blocks.len()).map(|_| Bits::new(count)).collect();
    let mut defs: Vec<Bits> = (0..blocks.len()).map(|_| Bits::new(count)).collect();
    for (index, block) in blocks.iter().enumerate() {
        for instruction in &block.instructions {
            for register in instruction.uses(class) {
                if !defs[index].get(register as usize) {
                    uses[index].set(register as usize);
                }
            }
            for register in instruction.defs(class) {
                if !uses[index].get(register as usize) {
                    defs[index].set(register as usize);
                }
            }
        }
    }
    // The function result is a use of the exit (last) block.
    if let Some(exit) = blocks.len().checked_sub(1) {
        match (class, function.returns) {
            (Class::General, ReturnRegisters::General) => uses[exit].set(3),
            (Class::General, ReturnRegisters::GeneralPair) => {
                uses[exit].set(3);
                uses[exit].set(4);
            }
            (Class::Float, ReturnRegisters::Float) => uses[exit].set(1),
            _ => {}
        }
        let everywhere = if class == Class::General { &function.exit_uses } else { &function.exit_float_uses };
        for &register in everywhere {
            uses[exit].set(register as usize);
        }
    }

    let mut live_in: Vec<Bits> = (0..blocks.len()).map(|_| Bits::new(count)).collect();
    let mut live_out: Vec<Bits> = (0..blocks.len()).map(|_| Bits::new(count)).collect();
    loop {
        let mut changed = false;
        for index in (0..blocks.len()).rev() {
            let block: &Block = &blocks[index];
            let mut out = Bits::new(count);
            for &successor in &block.successors {
                for (word, value) in out.0.iter_mut().zip(&live_in[successor].0) {
                    *word |= *value;
                }
            }
            for word in 0..out.0.len() {
                let value = (!defs[index].0[word] & out.0[word]) | uses[index].0[word];
                if value != live_in[index].0[word] {
                    live_in[index].0[word] = value;
                    changed = true;
                }
            }
            live_out[index] = out;
        }
        if !changed {
            break;
        }
    }
    live_out
}

/// `SpillCode_MarkLastUses`' dead-instruction removal: an instruction whose
/// every definition is of this class and dead, with no side effect, goes.
fn remove_dead_definitions(function: &mut PCodeFunction, class: Class, live_out: &[Bits]) {
    for (index, block) in function.blocks.iter_mut().enumerate() {
        let mut live = Bits(live_out[index].0.clone());
        let mut keep = vec![true; block.instructions.len()];
        for (position, instruction) in block.instructions.iter().enumerate().rev() {
            let defs = instruction.defs(class);
            let other_defs = instruction.defs(other_class(class));
            let dead = !instruction.has_side_effects()
                && !defines_condition_register(instruction)
                && other_defs.is_empty()
                && !defs.is_empty()
                && defs.iter().all(|register| !live.get(*register as usize));
            if dead {
                keep[position] = false;
                continue;
            }
            for register in defs {
                live.clear(register as usize);
            }
            for register in instruction.uses(class) {
                live.set(register as usize);
            }
        }
        let mut position = 0;
        block.instructions.retain(|_| {
            let kept = keep[position];
            position += 1;
            kept
        });
    }
}

fn other_class(class: Class) -> Class {
    match class {
        Class::General => Class::Float,
        Class::Float => Class::General,
    }
}

fn defines_condition_register(instruction: &mwcc_pcode::PInstr) -> bool {
    let name = format!("{:?}", instruction.instruction);
    let head = name.split([' ', '{', '(']).next().unwrap_or("");
    head.starts_with("Compare") || head.ends_with("Record") || head.starts_with("FloatCompare")
}

fn construct_interference(
    function: &PCodeFunction,
    class: Class,
    count: usize,
    live_out: &[Bits],
) -> Matrix {
    let mut matrix = Matrix::new(count);
    for first in 0..32 {
        for second in 0..32 {
            matrix.set_raw(first, second);
        }
    }
    for (index, block) in function.blocks.iter().enumerate() {
        let mut live = Bits(live_out[index].0.clone());
        for instruction in block.instructions.iter().rev() {
            let copy = instruction.copy(class);
            for register in instruction.defs(class) {
                live.clear(register as usize);
                for other in 0..count {
                    // A copy's source may share the destination's home.
                    let copy_source = copy.is_some_and(|(_, source)| source as usize == other);
                    if live.get(other) && !copy_source {
                        matrix.set(register as usize, other);
                    }
                }
            }
            for register in instruction.uses(class) {
                live.set(register as usize);
            }
            if class == Class::General {
                for &register in &instruction.not_r0 {
                    if register >= FIRST_VIRTUAL {
                        matrix.set(register as usize, 0);
                    }
                }
            }
        }
    }
    matrix
}

fn coalesce_root(roots: &[usize], mut register: usize) -> usize {
    while roots[register] != register {
        register = roots[register];
    }
    register
}

fn coalesce_copies(
    function: &mut PCodeFunction,
    class: Class,
    count: usize,
    matrix: &mut Matrix,
) -> Vec<usize> {
    let mut roots: Vec<usize> = (0..count).collect();
    let window = function.coalesce_first(class) as usize;
    for block in &mut function.blocks {
        let mut keep = vec![true; block.instructions.len()];
        for (position, instruction) in block.instructions.iter().enumerate() {
            let Some((destination, source)) = instruction.copy(class) else {
                continue;
            };
            if instruction.flags.coalesce_disabled {
                continue;
            }
            let first = coalesce_root(&roots, destination as usize);
            let second = coalesce_root(&roots, source as usize);
            if first == second {
                keep[position] = false;
            } else if !matrix.interferes(first, second)
                && (first < FIRST_VIRTUAL as usize
                    || second < FIRST_VIRTUAL as usize
                    || (first >= window
                        && second >= window
                        && !function.outside_window.contains(&(class, first as u32))
                        && !function.outside_window.contains(&(class, second as u32))))
            {
                let (root, child) = if first < second { (first, second) } else { (second, first) };
                roots[child] = root;
                for other in 0..count {
                    if matrix.interferes(child, other) {
                        matrix.set(root, other);
                    }
                }
                keep[position] = false;
            }
        }
        let mut position = 0;
        block.instructions.retain(|_| {
            let kept = keep[position];
            position += 1;
            kept
        });
    }
    for block in &mut function.blocks {
        for instruction in &mut block.instructions {
            rewrite_registers(instruction, class, |register| coalesce_root(&roots, register as usize) as u32);
        }
    }
    roots
}

fn rewrite_registers(
    instruction: &mut mwcc_pcode::PInstr,
    class: Class,
    mut map: impl FnMut(u32) -> u32,
) {
    mwcc_vreg::for_each_register(&mut instruction.instruction, |_, operand_class, field| {
        if operand_class == class {
            *field = map(*field);
        }
    });
    for register in instruction
        .implicit_uses
        .iter_mut()
        .chain(instruction.implicit_defs.iter_mut())
    {
        if register.class == class {
            register.number = map(register.number);
        }
    }
    if class == Class::General {
        for register in &mut instruction.not_r0 {
            *register = map(*register);
        }
    }
}

fn materialize(count: usize, matrix: &Matrix, roots: &[usize]) -> Vec<Node> {
    let mut nodes: Vec<Node> = (0..count)
        .map(|register| {
            let neighbors: Vec<usize> =
                (0..count).filter(|&other| matrix.interferes(register, other)).collect();
            Node {
                degree: neighbors.len() as i32,
                neighbors,
                physical: -1,
                coalesced: false,
                simplified: false,
                spill_cost: 0,
            }
        })
        .collect();
    for register in 0..count {
        if roots[register] != register {
            nodes[register].coalesced = true;
            nodes[register].physical = coalesce_root(roots, register) as i32;
        }
    }
    nodes
}

fn simplify_low_degree(
    nodes: &mut [Node],
    available: i32,
    stack: &mut Vec<usize>,
    remaining: &mut Vec<usize>,
) -> bool {
    let mut changed = false;
    remaining.clear();
    for register in FIRST_VIRTUAL as usize..nodes.len() {
        if nodes[register].simplified || nodes[register].coalesced {
            continue;
        }
        if nodes[register].degree < available {
            let neighbors = nodes[register].neighbors.clone();
            for neighbor in neighbors {
                nodes[neighbor].degree -= 1;
            }
            nodes[register].simplified = true;
            stack.push(register);
            changed = true;
        } else {
            // MWCC prepends survivors while scanning upward.
            remaining.insert(0, register);
        }
    }
    changed
}

fn compute_spill_costs(function: &PCodeFunction, class: Class, nodes: &mut [Node]) {
    for block in &function.blocks {
        let weight = i64::from(block.weight.max(1));
        for instruction in &block.instructions {
            for register in instruction.uses(class) {
                nodes[register as usize].spill_cost += 2 * weight;
            }
            for register in instruction.defs(class) {
                nodes[register as usize].spill_cost += weight;
            }
        }
    }
}

/// Returns the simplify stack, bottom first (select pops from the end).
fn simplify(function: &PCodeFunction, class: Class, nodes: &mut [Node], available: i32) -> Vec<usize> {
    let mut stack = Vec::new();
    let mut remaining = Vec::new();
    while simplify_low_degree(nodes, available, &mut stack, &mut remaining) {}
    if !remaining.is_empty() {
        compute_spill_costs(function, class, nodes);
    }
    while !remaining.is_empty() {
        let score = |node: &Node| node.spill_cost as f32 / node.degree as f32;
        let mut candidate = remaining[0];
        let mut candidate_score = score(&nodes[candidate]);
        for &register in &remaining[1..] {
            let register_score = score(&nodes[register]);
            if register_score < candidate_score {
                candidate = register;
                candidate_score = register_score;
            }
        }
        let neighbors = nodes[candidate].neighbors.clone();
        for neighbor in neighbors {
            nodes[neighbor].degree -= 1;
        }
        nodes[candidate].simplified = true;
        stack.push(candidate);
        while simplify_low_degree(nodes, available, &mut stack, &mut remaining) {}
    }
    stack
}

fn select(nodes: &mut [Node], stack: &[usize], policy: &ClassPolicy) -> Compilation<Vec<u32>> {
    let mut used = [false; 32];
    for &register in policy.used {
        used[register as usize] = true;
    }
    let mut mask: u32 = 0;
    for register in 0..=policy.mask_last {
        if !used[register as usize] {
            mask |= 1 << register;
        }
    }
    let mut claimed = Vec::new();
    for &register in stack.iter().rev() {
        let mut available = mask;
        for &neighbor in &nodes[register].neighbors {
            let color = nodes[neighbor].physical;
            if color != -1 && color < 32 {
                available &= !(1u32 << color);
            }
        }
        if available != 0 {
            nodes[register].physical = available.trailing_zeros() as i32;
        } else {
            let claim = (policy.claim_floor..32).rev().find(|&candidate| !used[candidate as usize]);
            let Some(color) = claim else {
                return Err(Diagnostic::error(
                    "PCode coloring needs spill code (not yet ported)",
                ));
            };
            used[color as usize] = true;
            claimed.push(color);
            nodes[register].physical = color as i32;
            mask |= 1 << color;
        }
    }
    Ok(claimed)
}

fn commit(function: &mut PCodeFunction, class: Class, nodes: &[Node]) {
    for block in &mut function.blocks {
        for instruction in &mut block.instructions {
            rewrite_registers(instruction, class, |register| {
                let mut color = nodes[register as usize].physical;
                // Coalesced children point at their (already rewritten) root.
                while color >= FIRST_VIRTUAL as i32 {
                    color = nodes[color as usize].physical;
                }
                if register < FIRST_VIRTUAL { register } else { color as u32 }
            });
        }
        block.instructions.retain(|instruction| {
            instruction
                .copy(class)
                .is_none_or(|(destination, source)| destination != source)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mwcc_machine_code::Instruction;
    use mwcc_pcode::{PInstr, Register};

    fn instruction(instruction: Instruction) -> PInstr {
        PInstr::new(instruction)
    }

    /// `int f(int a, int b) { return (a + 1) * b; }` as unoptimized PCode:
    /// parameters are copied out of r3/r4 into virtuals, the result copied
    /// into r3. Coalescing folds every copy onto its physical register.
    #[test]
    fn coalesces_parameter_and_result_copies_into_abi_registers() {
        let mut function = PCodeFunction::new("f", ReturnRegisters::General);
        let a = function.fresh(Class::General);
        let b = function.fresh(Class::General);
        let sum = function.fresh(Class::General);
        let product = function.fresh(Class::General);
        let body = &mut function.blocks[0].instructions;
        body.push(instruction(Instruction::Or { a, s: 3, b: 3 }));
        body.push(instruction(Instruction::Or { a: b, s: 4, b: 4 }));
        let mut add = instruction(Instruction::AddImmediate { d: sum, a, immediate: 1 });
        add.not_r0.push(a);
        body.push(add);
        body.push(instruction(Instruction::MultiplyLow { d: product, a: sum, b }));
        body.push(instruction(Instruction::Or { a: 3, s: product, b: product }));
        function.blocks.push(mwcc_pcode::Block { weight: 1, ..Default::default() });
        function.blocks[0].successors.push(1);

        color(&mut function, true).unwrap();
        let emitted: Vec<_> = function.blocks[0].instructions.iter().map(|i| i.instruction.clone()).collect();
        // sum is colored r0 (lowest volatile color): `addi r0,r3,1`;
        // product coalesces into r3: `mullw r3,r0,r4`.
        assert_eq!(
            emitted,
            vec![
                Instruction::AddImmediate { d: 0, a: 3, immediate: 1 },
                Instruction::MultiplyLow { d: 3, a: 0, b: 4 },
            ]
        );
        let _ = Register::general(3);
    }
}
