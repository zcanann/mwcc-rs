//! MWCC's basic-block list scheduler, following the model recovered from
//! GC/1.2.5 (github.com/JackPriceBurns/mwcc `docs/SCHEDULER.md`).
//!
//! Dependences are built walking the block backwards. Registers keep lists of
//! later uses and later definitions: a use gets write-after-read edges to later
//! definitions, a definition gets read-after-write edges (its latency) to later
//! uses and write-after-write edges to later definitions. GPR/FPR WAR/WAW edges
//! have latency 0; condition and special registers keep producer latency.
//! Memory accesses to one named object are ordered; pointer-based accesses are
//! wildcards ordered against every later memory access. Serializing opcodes
//! (branches) order against everything.
//!
//! Heights are critical-path lengths to the block end; deadline = max height -
//! height. Each cycle issues up to two instructions, at most one per
//! functional unit. The pick scans candidates in textual order; a later
//! candidate replaces the current best only by a strict win — urgency (due
//! while best is not), then more newly-released successors, then greater
//! height, then (while registers are still virtual) the smaller opcode rank.

use std::collections::HashMap;

use mwcc_machine_code::{Instruction, RelocationKind, RelocationTarget};
use mwcc_pcode::mnemonic::opcode_info;
use mwcc_pcode::opcodes::ISSUE_WIDTH;
use mwcc_pcode::{Class, PInstr};

/// Register keys: class tag + number. Special registers use distinct tags.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Key {
    General(u32),
    Float(u32),
    Condition,
    Link,
    Count,
    /// XER[CA].
    Carry,
}

impl Key {
    /// WAR/WAW edges on GPR/FPR are free; condition/special keep latency.
    fn ordering_latency(self, producer: u8) -> u8 {
        match self {
            // The stack pointer keeps producer latency (`mtlr` issues before
            // the frame is popped).
            Key::General(1) if !std::env::var_os("MWCC_SCHED_R1_FREE").is_some() => producer,
            Key::General(_) | Key::Float(_) => 0,
            _ => producer,
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
enum Memory {
    None,
    Load(Option<ObjectKey>),
    Store(Option<ObjectKey>),
}

#[derive(Clone, PartialEq, Eq, Hash, Debug)]
enum ObjectKey {
    Symbol(String),
    Frame(i16),
    /// A direct `r1`-based access inside a frame object: (object start,
    /// access offset, access bytes). Accesses to disjoint bytes of one
    /// object are independent.
    FrameBytes(i16, i16, i16),
    /// A D-form access through a base register other than `r1`: (base,
    /// offset, bytes). Disjoint bytes through one base are independent
    /// (first pass, `based_disambiguation` builds).
    Based(u32, i16, i16),
}

type ObjectKeyRef = ObjectKey;

struct Node {
    latency: u8,
    unit: u8,
    /// A second unit that can also issue this instruction (IU2).
    alternate: Option<u8>,
    occupancy: u8,
    rank: u8,
    serialize: bool,
    is_store: bool,
    height: u32,
    successors: Vec<(usize, u8)>,
    predecessors: usize,
}

fn operand_keys(instruction: &PInstr) -> (Vec<Key>, Vec<Key>) {
    let mut uses = Vec::new();
    let mut defs = Vec::new();
    for class in [Class::General, Class::Float] {
        let wrap = |number: u32| match class {
            Class::General => Key::General(number),
            Class::Float => Key::Float(number),
        };
        for register in instruction.uses(class) {
            // r2/r13 are fixed bases; r0 as a base reads literal zero.
            if class == Class::General && matches!(register, 2 | 13) {
                continue;
            }
            uses.push(wrap(register));
        }
        for register in instruction.defs(class) {
            defs.push(wrap(register));
        }
    }
    let name = format!("{:?}", instruction.instruction);
    let head = name.split([' ', '{', '(']).next().unwrap_or("");
    if head.starts_with("Compare") || head.starts_with("FloatCompare") || head.ends_with("Record")
    {
        defs.push(Key::Condition);
    }
    match &instruction.instruction {
        Instruction::BranchConditionalForward { .. } | Instruction::BranchConditionalToLinkRegister { .. } => {
            uses.push(Key::Condition)
        }
        Instruction::SubtractFromCarrying { .. }
        | Instruction::AddCarrying { .. }
        | Instruction::AddImmediateCarrying { .. }
        | Instruction::AddImmediateCarryingRecord { .. }
        | Instruction::SubtractFromImmediate { .. }
        | Instruction::ShiftRightAlgebraicImmediate { .. }
        | Instruction::ShiftRightAlgebraicImmediateRecord { .. }
        | Instruction::ShiftRightAlgebraicWord { .. } => defs.push(Key::Carry),
        Instruction::AddExtended { .. }
        | Instruction::SubtractFromExtended { .. }
        | Instruction::SubtractFromExtendedRecord { .. }
        | Instruction::AddToZeroExtended { .. }
        | Instruction::SubtractFromZeroExtended { .. } => {
            uses.push(Key::Carry);
            defs.push(Key::Carry);
        }
        // A store with update writes its base register.
        Instruction::StoreWordWithUpdate { a, .. } if !defs.contains(&Key::General(u32::from(*a))) => {
            defs.push(Key::General(u32::from(*a)))
        }
        Instruction::MoveFromLinkRegister { .. } => uses.push(Key::Link),
        Instruction::MoveFromConditionRegister { .. } => uses.push(Key::Condition),
        Instruction::ConditionRegisterOr { .. } => {
            uses.push(Key::Condition);
            defs.push(Key::Condition);
        }
        Instruction::ConditionRegisterClear { .. } | Instruction::ConditionRegisterSet { .. } => defs.push(Key::Condition),
        Instruction::MoveToLinkRegister { .. } => defs.push(Key::Link),
        Instruction::BranchToLinkRegister => uses.push(Key::Link),
        Instruction::BranchAndLink { .. } => defs.push(Key::Link),
        Instruction::MoveToCountRegister { .. } => defs.push(Key::Count),
        Instruction::BranchToCountRegister | Instruction::BranchToCountRegisterAndLink => {
            uses.push(Key::Count)
        }
        _ => {}
    }
    (uses, defs)
}

fn memory_of(instruction: &PInstr) -> Memory {
    let name = format!("{:?}", instruction.instruction);
    let head = name.split([' ', '{', '(']).next().unwrap_or("");
    let is_load = head.starts_with("Load") || head.starts_with("PairedSingleQuantizedLoad");
    let is_store = head.starts_with("Store") || head.starts_with("PairedSingleQuantizedStore");
    // Allocating the frame orders only through r1.
    if matches!(instruction.instruction, Instruction::StoreWordWithUpdate { a: 1, .. })
        && std::env::var_os("MWCC_SCHED_STWU_FREE").is_some()
    {
        return Memory::None;
    }
    if !is_load && !is_store {
        // The address of a frame object reads that object (final pass).
        if let Instruction::AddImmediate { a: 1, immediate, .. } = instruction.instruction {
            if std::env::var_os("MWCC_SCHED_NO_ADDRESS_LOAD").is_none() && FINAL_PASS.with(|flag| flag.get()) {
                let start = FRAME_OBJECTS.with(|objects| {
                    objects.borrow().iter().find(|&&(start, end)| start <= immediate && immediate < end).map(|&(start, _)| start)
                });
                if let Some(start) = start {
                    return Memory::Load(Some(ObjectKey::Frame(start)));
                }
            }
        }
        return Memory::None;
    }
    // Nothing stores to read-only memory (constant images).
    if is_load && instruction.flags.read_only {
        return Memory::None;
    }
    // An access at a computed address inside a known global.
    if let Some(symbol) = &instruction.object {
        let object = Some(ObjectKey::Symbol(symbol.clone()));
        return if is_load { Memory::Load(object) } else { Memory::Store(object) };
    }
    // An access through a section anchor touches its named object.
    if let Some(symbol) = &instruction.displacement_symbol {
        let object = (std::env::var_os("MWCC_SCHED_ANCHOR_WILDCARD").is_none()).then(|| ObjectKey::Symbol(symbol.clone()));
        return if is_load { Memory::Load(object) } else { Memory::Store(object) };
    }
    let object = match (&instruction.relocation, &instruction.instruction) {
        // Nothing stores to the constant pool: its loads are unordered.
        (Some(relocation), _)
            if matches!(relocation.target, RelocationTarget::Constant(_) | RelocationTarget::ConstantWithAddend(..)) && is_load
                && std::env::var_os("MWCC_SCHED_POOL_ORDERED").is_none() =>
        {
            return Memory::None;
        }
        (Some(relocation), _) if matches!(relocation.target, RelocationTarget::Constant(_)) => {
            let RelocationTarget::Constant(index) = relocation.target else { unreachable!() };
            Some(ObjectKey::Symbol(format!("@pool{index}")))
        }
        (Some(relocation), _)
            if matches!(
                relocation.kind,
                RelocationKind::EmbSda21 | RelocationKind::Addr16Lo
            ) =>
        {
            match &relocation.target {
                RelocationTarget::External(symbol) | RelocationTarget::ExternalWithAddend(symbol, _) => {
                    Some(ObjectKey::Symbol(symbol.clone()))
                }
                _ => None,
            }
        }
        (None, _) if BASED.with(|flag| flag.get()) && instruction.flags.nonvolatile_base && frame_offset(&name).is_none() => {
            based_access(&name).map(|(base, offset)| ObjectKey::Based(base, offset, access_bytes(&name)))
        }
        (None, _) => frame_offset(&name).map(|offset| {
            // An access inside a frame object belongs to that object.
            let start = FRAME_OBJECTS.with(|objects| {
                objects.borrow().iter().find(|&&(start, end)| start <= offset && offset < end).map(|&(start, _)| start)
            });
            match start {
                Some(start)
                    if std::env::var_os("MWCC_SCHED_FRAME_OBJECT_WHOLE").is_none()
                        && STRUCT_FRAME_OBJECTS.with(|objects| objects.borrow().contains(&start)) =>
                {
                    ObjectKey::FrameBytes(start, offset, access_bytes(&name))
                }
                _ => ObjectKey::Frame(start.unwrap_or(offset)),
            }
        }),
        _ => None,
    };
    if is_load {
        Memory::Load(object)
    } else {
        Memory::Store(object)
    }
}

/// Whether two accesses may touch the same memory: a named object only
/// itself; a pointer access anything but a private frame object.
fn may_alias(object: &Option<ObjectKey>, other: &Option<ObjectKey>) -> bool {
    // (Byte ranges of one frame object: overlapping ones alias.)
    if let (Some(ObjectKey::FrameBytes(start, offset, bytes)), Some(ObjectKey::FrameBytes(other_start, other_offset, other_bytes))) =
        (object, other)
    {
        if alias_all() {
            return true;
        }
        return start == other_start && offset < &(other_offset + other_bytes) && other_offset < &(offset + bytes);
    }
    if let (Some(ObjectKey::Based(base, offset, bytes)), Some(ObjectKey::Based(other_base, other_offset, other_bytes))) = (object, other) {
        if alias_all() || base != other_base {
            return true;
        }
        return offset < &(other_offset + other_bytes) && other_offset < &(offset + bytes);
    }
    // (A based access is a pointer access to everything else.)
    let (object, other) = (&unbased(object), &unbased(other));
    let object = &object.clone().map(ObjectKey::whole);
    let other = &other.clone().map(ObjectKey::whole);
    if alias_all() || object == other {
        return true;
    }
    let private = |key: &Option<ObjectKey>| match key {
        Some(ObjectKey::Frame(start)) => {
            std::env::var_os("MWCC_SCHED_FRAME_SHARED").is_none()
                && PRIVATE_FRAME_OBJECTS.with(|objects| objects.borrow().contains(start))
        }
        _ => false,
    };
    if private(object) || private(other) {
        return false;
    }
    object.is_none() || other.is_none()
}

/// A based access as a plain pointer access.
fn unbased(key: &Option<ObjectKey>) -> Option<ObjectKey> {
    match key {
        Some(ObjectKey::Based(..)) => None,
        other => other.clone(),
    }
}

impl ObjectKey {
    /// The object an access belongs to.
    fn whole(self) -> ObjectKey {
        match self {
            ObjectKey::FrameBytes(start, ..) => ObjectKey::Frame(start),
            other => other,
        }
    }
}

/// The bytes a load or store touches, from its debug form.
fn access_bytes(debug: &str) -> i16 {
    let head = debug.split([' ', '{', '(']).next().unwrap_or("");
    if head.contains("Multiple") {
        i16::MAX / 2
    } else if head.contains("Byte") {
        1
    } else if head.contains("Halfword") {
        2
    } else if head.contains("Double") || head.contains("PairedSingle") {
        8
    } else {
        4
    }
}

thread_local! {
    /// Accesses through one base register at disjoint offsets are
    /// independent (GC/3.x, Wii).
    pub static BASED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The base register and displacement of a non-`r1` D-form access.
fn based_access(debug: &str) -> Option<(u32, i16)> {
    let field = |name: &str| -> Option<&str> {
        let at = debug.find(&format!(" {name}: "))? + name.len() + 3;
        Some(debug[at..].split([',', ' ', '}']).next().unwrap_or(""))
    };
    let base: u32 = field("a")?.parse().ok()?;
    let offset: i16 = field("offset")?.parse().ok()?;
    (base != 1 && base != 0).then_some((base, offset))
}

/// The displacement of an `r1`-based D-form access, from its debug form.
fn frame_offset(debug: &str) -> Option<i16> {
    if !debug.contains(" a: 1,") {
        return None;
    }
    let rest = &debug[debug.find("offset: ")? + 8..];
    let end = rest.find([',', ' ', '}']).unwrap_or(rest.len());
    rest[..end].parse().ok()
}

/// Reorder `instructions` (one basic block) as MWCC's scheduler would.
/// `virtual_registers` enables the final opcode-rank tie-break.
pub fn schedule_block(instructions: &mut Vec<PInstr>, virtual_registers: bool) {
    FINAL_PASS.with(|flag| flag.set(!virtual_registers));
    let count = instructions.len();
    if count <= 2 {
        return;
    }
    let mut nodes: Vec<Node> = instructions
        .iter()
        .map(|instruction| {
            let info = opcode_info(&instruction.instruction);
            let two_integer_units = (TWO_INTEGER_UNITS.with(|flag| flag.get())
                || std::env::var_os("MWCC_SCHED_TWO_IU").is_some())
                && std::env::var_os("MWCC_SCHED_ONE_IU").is_none();
            let multiply_or_divide = info.mnemonic.starts_with("MUL") || info.mnemonic.starts_with("DIV");
            Node {
                latency: info.latency,
                unit: info.unit,
                alternate: (two_integer_units && info.unit == 1 && !multiply_or_divide).then_some(8),
                occupancy: info.occupancy.max(1),
                rank: info.pick_rank,
                serialize: info.serialize
                    || instruction.flags.serialize
                    || instruction.instruction.is_call()
                    // Frame paired-single restores keep the epilogue's order.
                    || (matches!(instruction.instruction, Instruction::PairedSingleQuantizedLoad { a: 1, .. } | Instruction::PairedSingleQuantizedStore { a: 1, .. })
                        && std::env::var_os("MWCC_SCHED_PSQ_FREE").is_none())
                    // The variadic CR1 marker stays at the call.
                    || matches!(instruction.instruction, Instruction::ConditionRegisterClear { .. } | Instruction::ConditionRegisterSet { .. })
                    || (matches!(instruction.instruction, Instruction::AddImmediate { d: 1, a: 1, .. })
                        && std::env::var_os("MWCC_SCHED_POP_FREE").is_none()),
                is_store: matches!(memory_of(instruction), Memory::Store(_)),
                height: u32::from(info.latency),
                successors: Vec::new(),
                predecessors: 0,
            }
        })
        .collect();

    let mut later_uses: HashMap<Key, Vec<usize>> = HashMap::new();
    let mut later_defs: HashMap<Key, Vec<usize>> = HashMap::new();
    let mut later_memory: Vec<(usize, Memory)> = Vec::new();
    let mut later_all: Vec<usize> = Vec::new();
    let mut later_in_order: Option<usize> = None;
    let mut edges: Vec<(usize, usize, u8)> = Vec::new();
    let mut delayed: Vec<(usize, usize)> = Vec::new();

    for index in (0..count).rev() {
        let instruction = &instructions[index];
        let latency = nodes[index].latency;
        if instruction.flags.in_order {
            if let Some(later) = later_in_order {
                edges.push((index, later, 0));
            }
            later_in_order = Some(index);
        }
        // (Final pass: a copy expanded after scheduling follows all the
        // code before it.)
        if !virtual_registers && instruction.flags.block_copy && std::env::var_os("MWCC_SCHED_EARLY_COPIES").is_none() {
            for earlier in 0..index {
                if !instructions[earlier].flags.block_copy {
                    edges.push((earlier, index, 0));
                }
            }
        }
        // (Prescheduling: the parameters' entry copies precede the frame
        // addresses.)
        if virtual_registers && instruction.flags.entry_copy && std::env::var_os("MWCC_SCHED_FREE_ENTRY_COPIES").is_none() {
            for &later in &later_all {
                if matches!(instructions[later].instruction, Instruction::AddImmediate { a: 1, .. }) {
                    edges.push((index, later, 0));
                }
            }
        }
        let (uses, defs) = operand_keys(instruction);
        for key in &uses {
            for &later in later_defs.get(key).into_iter().flatten() {
                edges.push((index, later, key.ordering_latency(latency)));
            }
        }
        for key in &defs {
            for &later in later_uses.get(key).into_iter().flatten() {
                // (`mtlr` reads a loaded value two cycles late; heights
                // keep the load's latency.)
                if mtlr_delay()
                    && matches!(instructions[later].instruction, Instruction::MoveToLinkRegister { .. })
                    && matches!(memory_of(instruction), Memory::Load(_))
                {
                    delayed.push((index, later));
                }
                edges.push((index, later, latency));
            }
            for &later in later_defs.get(key).into_iter().flatten() {
                edges.push((index, later, key.ordering_latency(latency)));
            }
        }
        for key in uses {
            later_uses.entry(key).or_default().insert(0, index);
        }
        for key in defs {
            later_defs.entry(key).or_default().insert(0, index);
        }
        let memory = memory_of(instruction);
        match &memory {
            Memory::None => {}
            Memory::Load(object) => {
                for (later, later_memory_kind) in &later_memory {
                    if let Memory::Store(other) = later_memory_kind {
                        // (Through one base, only stores are disambiguated.)
                        if may_alias(&unbased(object), &unbased(other)) {
                            edges.push((index, *later, latency));
                        }
                    }
                }
            }
            Memory::Store(object) => {
                for (later, later_memory_kind) in &later_memory {
                    let other = match later_memory_kind {
                        Memory::Load(other) | Memory::Store(other) => other,
                        Memory::None => continue,
                    };
                    // (Stores keep their order when one is at a computed
                    // address inside a known global.)
                    let tagged_stores = matches!(later_memory_kind, Memory::Store(_))
                        && (instructions[index].object.is_some() || instructions[*later].object.is_some());
                    let aliased = if matches!(later_memory_kind, Memory::Store(_)) {
                        may_alias(object, other)
                    } else {
                        may_alias(&unbased(object), &unbased(other))
                    };
                    if aliased || tagged_stores {
                        edges.push((index, *later, latency));
                    }
                }
            }
        }
        if memory != Memory::None {
            later_memory.insert(0, (index, memory));
        }
        if nodes[index].serialize {
            for &later in &later_all {
                edges.push((index, later, latency));
            }
        }
        for &later in &later_all {
            if nodes[later].serialize {
                edges.push((index, later, latency));
            }
        }
        later_all.insert(0, index);
    }

    let _ = std::marker::PhantomData::<ObjectKeyRef>;
    // Deduplicate edges keeping the largest latency, then compute heights in
    // reverse textual order (every edge points forward).
    let mut best: HashMap<(usize, usize), u8> = HashMap::new();
    for (from, to, latency) in edges {
        let entry = best.entry((from, to)).or_insert(latency);
        *entry = (*entry).max(latency);
    }
    let mut ordered: Vec<((usize, usize), u8)> = best.into_iter().collect();
    ordered.sort();
    for &((from, to), latency) in &ordered {
        nodes[from].successors.push((to, latency));
        nodes[to].predecessors += 1;
    }
    for index in (0..count).rev() {
        let height = nodes[index]
            .successors
            .iter()
            .map(|&(to, latency)| u32::from(latency) + nodes[to].height)
            .max()
            .unwrap_or(0)
            .max(u32::from(nodes[index].latency));
        nodes[index].height = height;
    }
    let maximum = nodes.iter().map(|node| node.height).max().unwrap_or(0);
    let deadline: Vec<i64> = nodes.iter().map(|node| i64::from(maximum) - i64::from(node.height)).collect();

    let mut remaining_predecessors: Vec<usize> = nodes.iter().map(|node| node.predecessors).collect();
    let mut ready_at: Vec<i64> = vec![0; count];
    let mut issued = vec![false; count];
    let mut unit_busy_until = [0i64; 9];
    let mut store_stage_until: i64 = -1;
    let mut order = Vec::with_capacity(count);
    let mut cycle: i64 = 0;
    while order.len() < count {
        let mut issued_this_cycle = 0;
        while issued_this_cycle < ISSUE_WIDTH {
            let issuable = |index: usize| {
                !issued[index]
                    && remaining_predecessors[index] == 0
                    && ready_at[index] <= cycle
                    && (unit_busy_until[nodes[index].unit as usize] <= cycle
                        || nodes[index].alternate.is_some_and(|unit| unit_busy_until[unit as usize] <= cycle))
                    && !(nodes[index].is_store && store_stage_until >= cycle)
            };
            let release_count = |index: usize, remaining: &[usize]| {
                nodes[index]
                    .successors
                    .iter()
                    .filter(|&&(to, _)| remaining[to] == 1)
                    .count()
            };
            let mut chosen: Option<usize> = None;
            for candidate in 0..count {
                if !issuable(candidate) {
                    continue;
                }
                let Some(best) = chosen else {
                    chosen = Some(candidate);
                    continue;
                };
                // (A copy expanded after this pass comes after other code.)
                let copies = (instructions[candidate].flags.block_copy, instructions[best].flags.block_copy);
                if copies.0 != copies.1 && std::env::var_os("MWCC_SCHED_EARLY_COPIES").is_none() {
                    if !copies.0 {
                        chosen = Some(candidate);
                    }
                    continue;
                }
                if !(cycle < deadline[best] || deadline[candidate] <= cycle) {
                    continue;
                }
                let candidate_due = deadline[candidate] <= cycle;
                let best_due = deadline[best] <= cycle;
                let wins = if candidate_due != best_due {
                    candidate_due
                } else {
                    let (c_release, b_release) = (
                        release_count(candidate, &remaining_predecessors),
                        release_count(best, &remaining_predecessors),
                    );
                    if c_release != b_release {
                        c_release > b_release
                    } else if nodes[candidate].height != nodes[best].height {
                        nodes[candidate].height > nodes[best].height
                    } else {
                        virtual_registers && nodes[candidate].rank < nodes[best].rank
                    }
                };
                if wins {
                    chosen = Some(candidate);
                }
            }
            let Some(index) = chosen else { break };
            if trace() {
                let ready: Vec<String> = (0..count)
                    .filter(|&c| issuable(c))
                    .map(|c| format!("{}:h{}d{}r{}", c, nodes[c].height, deadline[c], release_count(c, &remaining_predecessors)))
                    .collect();
                eprintln!("  cycle {cycle} pick {index} {:?} from {}", instructions[index].instruction, ready.join(" "));
            }
            issued[index] = true;
            order.push(index);
            issued_this_cycle += 1;
            let node = &nodes[index];
            let unit = if unit_busy_until[node.unit as usize] <= cycle {
                node.unit
            } else {
                node.alternate.expect("issuable on its alternate unit")
            };
            unit_busy_until[unit as usize] = cycle + i64::from(node.occupancy);
            if node.is_store {
                store_stage_until = cycle + 1;
            }
            for &(to, latency) in &node.successors {
                remaining_predecessors[to] -= 1;
                // A successor released this cycle issues next cycle at the
                // earliest, except through a zero-latency edge (see
                // `same_cycle_release`).
                let earliest = if same_cycle_release(&instructions[to]) { latency } else { latency.max(1) };
                let earliest = earliest + if delayed.contains(&(index, to)) { mtlr_extra() } else { 0 };
                ready_at[to] = ready_at[to].max(cycle + i64::from(earliest));
            }
            if node.serialize {
                break;
            }
        }
        cycle += 1;
        if cycle > 100_000 {
            // Safety valve: keep textual order for anything left.
            for index in 0..count {
                if !issued[index] {
                    issued[index] = true;
                    order.push(index);
                }
            }
        }
    }
    let original = std::mem::take(instructions);
    let mut slots: Vec<Option<PInstr>> = original.into_iter().map(Some).collect();
    for index in order {
        instructions.push(slots[index].take().expect("each index issues once"));
    }
}

fn mtlr_extra() -> u8 {
    static EXTRA: std::sync::OnceLock<u8> = std::sync::OnceLock::new();
    *EXTRA.get_or_init(|| std::env::var("MWCC_SCHED_MTLR_EXTRA").ok().and_then(|v| v.parse().ok()).unwrap_or(2))
}

fn mtlr_delay() -> bool {
    static DELAY: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *DELAY.get_or_init(|| std::env::var_os("MWCC_SCHED_NO_MTLR_DELAY").is_none())
}

/// `MWCC_SCHED_TRACE`: print each pick with the ready candidates
/// (index:height, deadline, released successors).
fn trace() -> bool {
    static TRACE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *TRACE.get_or_init(|| std::env::var_os("MWCC_SCHED_TRACE").is_some())
}

/// Whether a successor released by a zero-latency edge may issue in the same
/// cycle: in the first (virtual-register) pass any may; in the final pass
/// any but a load. `MWCC_SCHED_SAME_CYCLE` = `none` | `all` overrides this
/// for study.
fn same_cycle_release(successor: &PInstr) -> bool {
    let final_pass = FINAL_PASS.with(|flag| flag.get());
    static MODE: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    match MODE.get_or_init(|| std::env::var("MWCC_SCHED_SAME_CYCLE").ok()).as_deref() {
        Some("none") => false,
        Some("all") => true,
        _ => !final_pass || !matches!(memory_of(successor), Memory::Load(_)),
    }
}

thread_local! {
    static FINAL_PASS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The machine model has a second integer unit (post-1.2.5 builds).
    pub(crate) static TWO_INTEGER_UNITS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// Frame objects that are structs: their fields are independent.
    pub(crate) static STRUCT_FRAME_OBJECTS: std::cell::RefCell<Vec<i16>> = const { std::cell::RefCell::new(Vec::new()) };
    /// Frame objects no pointer can reach.
    pub(crate) static PRIVATE_FRAME_OBJECTS: std::cell::RefCell<Vec<i16>> = const { std::cell::RefCell::new(Vec::new()) };
    /// The function's frame objects, `[start, end)` from r1.
    pub(crate) static FRAME_OBJECTS: std::cell::RefCell<Vec<(i16, i16)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// In the final (physical) pass every memory access is mutually ordered —
/// frame code generated after coloring carries no object identity. The first
/// pass disambiguates named objects. `MWCC_SCHED_ALIAS=pre|both|none`
/// overrides this for model study.
fn alias_all() -> bool {
    let final_pass = FINAL_PASS.with(|flag| flag.get());
    match std::env::var("MWCC_SCHED_ALIAS").as_deref() {
        Ok("none") => false,
        Ok("pre") => !final_pass,
        Ok("both") => true,
        _ => final_pass,
    }
}
