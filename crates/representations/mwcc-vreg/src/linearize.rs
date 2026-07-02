//! The LINEARIZER model — a reproduction of mwcc's -O4 block-DAG scheduling,
//! fit against `harness/sched_dataset/` (the real compiler's measured orders).
//!
//! The model (v3, the first to survive the dataset): Gekko is dual-issue, and
//! mwcc's order reads as PAIRED issue over the dependence DAG —
//!
//! - a simulated clock; each op issues at a time step and its result is ready
//!   `latency` steps later (alu 1, load 2, mulli 3, divw 19, fdiv 31 — the
//!   RELATIVE magnitudes matter, not the exact cycles);
//! - at each step, up to TWO independent ready ops issue (operands complete),
//!   ordered by CRITICAL PATH (the latency-weighted longest path from the op
//!   to any sink), ties broken by source order;
//! - stores in one ALIAS GROUP keep program order (pointer stores conservatively
//!   share a group; distinct globals do not — measured: three_deep_vs_shallow
//!   stores the shallow statement first, divide_chain delays the div store);
//! - register-staging conflicts (two values through r0) are extra dependence
//!   edges, supplied by the caller — the allocation coupling, not yet derived.
//!
//! This module is UNWIRED: it exists to be A/B'd against the dataset before
//! any emitter consumes it.

/// The operation class, for kind-ranked priority components.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpKind {
    Alu,
    Load,
    Store,
}

/// One abstract operation in a block's dependence DAG.
#[derive(Debug, Clone)]
pub struct DagNode {
    /// For test assertions and diagnostics.
    pub label: &'static str,
    /// The operation class.
    pub kind: OpKind,
    /// Issue-to-result steps (see the module table) — the PRIORITY weight.
    pub latency: u32,
    /// Readiness gating steps (defaults to `latency`); mulli gates consumers
    /// at 2 while weighing 3 (measured: store orders vs intra-expression
    /// priority disagree on one latency).
    pub gate_latency: u32,
    /// Value ids this op reads (RAW edges come from these).
    pub reads: Vec<u32>,
    /// Value ids this op defines.
    pub writes: Vec<u32>,
    /// Stores that may alias share a group and keep program order.
    pub alias_group: Option<u32>,
    /// Extra dependence edges (indices), e.g. the r0-staging serialization.
    pub extra_deps: Vec<usize>,
    /// A shared-resource class (e.g. XER for carry-writing srawi): two ops of
    /// one class cannot issue in the same cycle — measured: srawi+srawi
    /// serialize where rlwinm+rlwinm pair.
    pub hazard: Option<u8>,
    /// This value is consumed as an addi source or a load/store BASE — where
    /// PPC reads r0 as literal zero — so it must never be assigned r0.
    pub forbid_r0: bool,
    /// A narrow-parameter re-extension (extsb/extsh/clrlwi). A SINGLE-consumer
    /// extension may reuse its dying param register even as a first-of-pair
    /// intermediate (measured: void extsb r3,r3); a multi-consumer one may not
    /// (it must outlive the first chain's final — measured: extsb r4,r3).
    pub extension: bool,
}

/// The XER (carry) hazard class: srawi, subfc, addc.
pub const HAZARD_XER: u8 = 1;

impl DagNode {
    pub fn new(label: &'static str, latency: u32) -> DagNode {
        let kind = match latency {
            2 => OpKind::Load,
            _ => OpKind::Alu,
        };
        DagNode { label, kind, latency, gate_latency: latency, reads: Vec::new(), writes: Vec::new(), alias_group: None, extra_deps: Vec::new(), hazard: None, forbid_r0: false, extension: false }
    }
    pub fn extension(mut self) -> DagNode {
        self.extension = true;
        self
    }
    pub fn forbid_r0(mut self) -> DagNode {
        self.forbid_r0 = true;
        self
    }
    pub fn hazard(mut self, class: u8) -> DagNode {
        self.hazard = Some(class);
        self
    }
    pub fn gate(mut self, gate_latency: u32) -> DagNode {
        self.gate_latency = gate_latency;
        self
    }
    pub fn kind(mut self, kind: OpKind) -> DagNode {
        self.kind = kind;
        self
    }
    pub fn reads(mut self, values: &[u32]) -> DagNode {
        self.reads = values.to_vec();
        self
    }
    pub fn writes(mut self, values: &[u32]) -> DagNode {
        self.writes = values.to_vec();
        self
    }
    pub fn alias(mut self, group: u32) -> DagNode {
        self.alias_group = Some(group);
        self
    }
    pub fn after(mut self, index: usize) -> DagNode {
        self.extra_deps.push(index);
        self
    }
}

/// A candidate scheduling model: the fitter enumerates these against the
/// dataset; `linearize` uses the frozen best.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Model {
    /// Ops issued per step (Gekko dual-issue = 2).
    pub issue_width: usize,
    /// Ready requires operand COMPLETION (vs mere issue).
    pub gate_on_complete: bool,
    /// LOAD-GATED demotion: ops whose downstream join also waits on a load
    /// through a different operand sort after un-gated ops.
    pub gated_last: bool,
    /// Kind rank in the priority key (lower issues first), by [alu, load, store].
    pub kind_rank: [u8; 3],
    /// Weight (critical path) position: true = before kind in the key.
    pub weight_before_kind: bool,
    /// The selection strategy (global key vs chain round-robin).
    pub strategy: Strategy,
}

/// How the next ops are chosen: one global priority key, or per-CHAIN
/// round-robin (a chain = the ops feeding one sink), which several captures'
/// alternation patterns suggest.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Strategy {
    GlobalKey,
    ChainRobin { lead: LeadRule, offer_non_load_first: bool },
}

/// Which chain leads a round-robin step.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LeadRule {
    /// The latency-heaviest chain always leads.
    Heaviest,
    /// Load-free chains lead load-bearing ones; weight breaks ties.
    LoadFreeFirst,
    /// The lead alternates each step, starting from the LoadFreeFirst pick.
    Alternating,
}

/// The frozen model (v4, fitter-selected at 9/10): dual-issue, completion-
/// gated, critical-path first, with STORES ranking before alu before loads on
/// weight ties (the fitter's discovery — explains the equal-chain stfd
/// placement). The one open fixture is the frame-context tail_pair anomaly.
pub const FROZEN: Model = Model {
    issue_width: 2,
    gate_on_complete: true,
    gated_last: false,
    kind_rank: [1, 2, 0],
    weight_before_kind: true,
    strategy: Strategy::GlobalKey,
};

/// Linearize the DAG with the frozen model.
pub fn linearize(nodes: &[DagNode]) -> Vec<usize> {
    linearize_with(nodes, FROZEN)
}

/// Linearize the DAG under a candidate model: the returned indices are the
/// emission order.
pub fn linearize_with(nodes: &[DagNode], model: Model) -> Vec<usize> {
    let count = nodes.len();
    // Dependence edges: RAW (a read of a value written earlier in the list),
    // same-alias-group program order, and the explicit extras.
    let mut deps: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (index, node) in nodes.iter().enumerate() {
        for read in &node.reads {
            // The most recent earlier writer of this value.
            if let Some(writer) = (0..index).rev().find(|&w| nodes[w].writes.contains(read)) {
                deps[index].push(writer);
            }
        }
        if let Some(group) = node.alias_group {
            if let Some(previous) = (0..index).rev().find(|&p| nodes[p].alias_group == Some(group)) {
                deps[index].push(previous);
            }
        }
        deps[index].extend(node.extra_deps.iter().copied());
    }
    // Critical-path weight: latency + the heaviest dependent's weight. On the
    // RETURN chain (the chain of a consumerless non-store node) ops weigh
    // their GATE latency instead — measured: the return chain leads only when
    // its gate-weighted path exceeds the store chains' (ret_both_deep's g
    // leads at 3v3 source-tie; ret_deep_chain's return leads at 4v3).
    let return_sink: Option<usize> = (0..count).find(|&node| {
        nodes[node].kind != OpKind::Store
            && !nodes[node].writes.is_empty()
            && (node + 1..count).all(|later| !deps[later].contains(&node))
    });
    let on_return_chain: Vec<bool> = (0..count)
        .map(|node| {
            let mut current = node;
            loop {
                match (current + 1..count).find(|&later| deps[later].contains(&current)) {
                    Some(next) => current = next,
                    None => break Some(current) == return_sink,
                }
            }
        })
        .collect();
    let mut weight = vec![0u32; count];
    for index in (0..count).rev() {
        let downstream = (index + 1..count)
            .filter(|&later| deps[later].contains(&index))
            .map(|later| weight[later])
            .max()
            .unwrap_or(0);
        let own = if on_return_chain[index] { nodes[index].gate_latency } else { nodes[index].latency };
        weight[index] = own + downstream;
    }

    // LOAD-GATED: reachable joins (2+ reads) that also wait on a load through a
    // different operand path. Reachability over the dep edges.
    let mut reaches: Vec<Vec<bool>> = vec![vec![false; count]; count];
    for from in (0..count).rev() {
        for later in from + 1..count {
            if deps[later].contains(&from) {
                reaches[from][later] = true;
                for target in 0..count {
                    if reaches[later][target] {
                        reaches[from][target] = true;
                    }
                }
            }
        }
    }
    let gated: Vec<bool> = (0..count)
        .map(|node| {
            (0..count).any(|join| {
                deps[join].len() >= 2
                    && (node == join || reaches[node][join])
                    && deps[join].iter().any(|&side| {
                        // Another operand path of the join that carries a load
                        // NOT through this node.
                        side != node
                            && !reaches[node][side]
                            && ((nodes[side].kind == OpKind::Load)
                                || (0..count).any(|load| {
                                    nodes[load].kind == OpKind::Load && load != node && reaches[load][side]
                                }))
                    })
            })
        })
        .collect();

    // Chain id = the sink each node ultimately feeds (self for sinks); chain
    // weight = the heaviest path within it; chain has_load per member kinds.
    let chain: Vec<usize> = (0..count)
        .map(|node| (node..count).find(|&sink| (node == sink || reaches[node][sink]) && (sink + 1..count).all(|later| !reaches[sink][later])).unwrap_or(node))
        .collect();
    let chain_weight: Vec<u32> = (0..count).map(|node| {
        let id = chain[node];
        (0..count).filter(|&other| chain[other] == id).map(|other| weight[other]).max().unwrap_or(0)
    }).collect();
    let chain_has_load: Vec<bool> = (0..count).map(|node| {
        let id = chain[node];
        (0..count).any(|other| chain[other] == id && nodes[other].kind == OpKind::Load)
    }).collect();

    let mut order = Vec::with_capacity(count);
    let mut issued_at: Vec<Option<u32>> = vec![None; count];
    let mut time = 0u32;
    let mut robin_flip = false;
    while order.len() < count {
        let mut ready: Vec<usize> = (0..count)
            .filter(|&candidate| issued_at[candidate].is_none())
            .filter(|&candidate| {
                deps[candidate].iter().all(|&dependency| {
                    issued_at[dependency].is_some_and(|at| {
                        if model.gate_on_complete {
                            at + nodes[dependency].gate_latency <= time
                        } else {
                            at < time
                        }
                    })
                })
            })
            .collect();
        if ready.is_empty() {
            time += 1;
            continue;
        }
        let rank = |candidate: usize| -> (u8, u32, u32, usize) {
            let gate = if model.gated_last && gated[candidate] { 1 } else { 0 };
            let kind = model.kind_rank[match nodes[candidate].kind {
                OpKind::Alu => 0,
                OpKind::Load => 1,
                OpKind::Store => 2,
            }] as u32;
            let inverse_weight = u32::MAX - weight[candidate];
            if model.weight_before_kind {
                (gate, inverse_weight, kind, candidate)
            } else {
                (gate, kind, inverse_weight, candidate)
            }
        };
        match model.strategy {
            Strategy::GlobalKey => {
                ready.sort_by_key(|&candidate| rank(candidate));
                // Fill the issue window, skipping shared-resource conflicts
                // (XER: two carry-writing ops cannot pair — measured
                // srawi+srawi serializing where rlwinm+rlwinm pairs).
                let mut picked: Vec<usize> = Vec::new();
                let mut picked_hazards: Vec<u8> = Vec::new();
                for &candidate in &ready {
                    if picked.len() >= model.issue_width {
                        break;
                    }
                    if let Some(class) = nodes[candidate].hazard {
                        if picked_hazards.contains(&class) {
                            continue;
                        }
                        picked_hazards.push(class);
                    }
                    picked.push(candidate);
                }
                ready = picked;
            }
            Strategy::ChainRobin { lead, offer_non_load_first } => {
                // Each chain offers ONE ready op; the lead chain's offer goes first.
                let mut offers: Vec<usize> = Vec::new();
                let mut seen_chains: Vec<usize> = Vec::new();
                let mut by_chain = ready.clone();
                by_chain.sort_by_key(|&candidate| {
                    let load = if offer_non_load_first && nodes[candidate].kind == OpKind::Load { 1u8 } else { 0 };
                    (chain[candidate], load, candidate)
                });
                for &candidate in &by_chain {
                    if !seen_chains.contains(&chain[candidate]) {
                        seen_chains.push(chain[candidate]);
                        offers.push(candidate);
                    }
                }
                let lead_key = |candidate: &usize| -> (u8, std::cmp::Reverse<u32>, usize) {
                    let load_penalty = if chain_has_load[*candidate] { 1u8 } else { 0 };
                    match lead {
                        LeadRule::Heaviest => (0, std::cmp::Reverse(chain_weight[*candidate]), chain[*candidate]),
                        LeadRule::LoadFreeFirst | LeadRule::Alternating => {
                            (load_penalty, std::cmp::Reverse(chain_weight[*candidate]), chain[*candidate])
                        }
                    }
                };
                offers.sort_by_key(lead_key);
                if lead == LeadRule::Alternating && robin_flip {
                    offers.reverse();
                }
                robin_flip = !robin_flip;
                offers.truncate(model.issue_width);
                ready = offers;
            }
        }
        for &pick in &ready {
            issued_at[pick] = Some(time);
            order.push(pick);
        }
        time += 1;
    }
    order
}

/// A candidate REGISTER policy for block values (the allocation contract the
/// emitter needs alongside the order): which values stage through r0, and how
/// the rest pick among free volatiles.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RegisterPolicy {
    pub r0_rule: R0Rule,
    pub reuse: ReuseRule,
}

/// When a value stages through r0.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum R0Rule {
    /// Only the FINAL value (the stw operand) of the LAST source chain — and of
    /// a single-statement block.
    FinalOfLastChain,
    /// The final value, plus the whole last chain when it has TWO ops (the
    /// measured 2-op in-place chains vs the 3-op bounce).
    FinalPlusTwoOpChain,
    /// Every op of the last chain (killed by the 3-op bounce; kept as control).
    WholeLastChain,
}

/// How a non-r0 result picks among free volatile registers (r3..r12).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReuseRule {
    /// The lowest register free at issue (deaths at this instruction count).
    LowestFree,
    /// The lowest free, EXCLUDING registers freed in this same issue cycle.
    LowestFreeStable,
    /// The op's own dying source first, then lowest free.
    OwnSourceFirst,
}

/// Assign a result register per node, walking the linearized order. `params`
/// maps input value ids to their incoming registers; `last_chain` is the chain
/// id (sink index) of the LAST source statement.
pub fn assign_registers(
    nodes: &[DagNode],
    order: &[usize],
    params: &[(u32, u8)],
    policy: RegisterPolicy,
) -> Vec<Option<u8>> {
    let count = nodes.len();
    // Rebuild chains and the final-op set the same way linearize_with does.
    let mut consumer_of: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (index, node) in nodes.iter().enumerate() {
        for read in &node.reads {
            if let Some(writer) = (0..index).rev().find(|&w| nodes[w].writes.contains(read)) {
                consumer_of[writer].push(index);
            }
        }
    }
    let chain_of = |mut node: usize| -> usize {
        loop {
            match consumer_of[node].first() {
                Some(&next) => node = next,
                None => return node,
            }
        }
    };
    let last_sink = (0..count).rev().find(|&node| consumer_of[node].is_empty()).unwrap_or(count - 1);
    // Ops on the last chain, and each op's position: final = feeds the sink store.
    let on_last_chain: Vec<bool> = (0..count).map(|node| chain_of(node) == last_sink).collect();
    let feeds_sink: Vec<bool> = (0..count).map(|node| consumer_of[node].first() == Some(&last_sink)).collect();
    let last_chain_ops = (0..count).filter(|&node| on_last_chain[node] && nodes[node].kind != OpKind::Store).count();

    // The death slot of each VALUE (last read position in the order).
    let position: Vec<usize> = {
        let mut position = vec![0; count];
        for (slot, &node) in order.iter().enumerate() {
            position[node] = slot;
        }
        position
    };
    let mut result: Vec<Option<u8>> = vec![None; count];
    // Live map: register -> death slot (exclusive). Params live until last read.
    let mut live: Vec<(u8, usize)> = params
        .iter()
        .map(|&(value, register)| {
            let death = (0..count)
                .filter(|&reader| nodes[reader].reads.contains(&value))
                .map(|reader| position[reader])
                .max()
                .unwrap_or(0);
            (register, death)
        })
        .collect();
    for (slot, &node) in order.iter().enumerate() {
        if nodes[node].kind == OpKind::Store || nodes[node].writes.is_empty() {
            continue;
        }
        let stages_r0 = match policy.r0_rule {
            R0Rule::FinalOfLastChain => on_last_chain[node] && feeds_sink[node],
            R0Rule::FinalPlusTwoOpChain => {
                on_last_chain[node] && (feeds_sink[node] || last_chain_ops <= 2)
            }
            R0Rule::WholeLastChain => on_last_chain[node],
        };
        if stages_r0 {
            result[node] = Some(0);
            continue;
        }
        let death = consumer_of[node].iter().map(|&reader| position[reader]).max().unwrap_or(slot);
        let own_dying: Option<u8> = nodes[node].reads.iter().find_map(|read| {
            params.iter().find(|&&(value, _)| value == *read).and_then(|&(value, register)| {
                let value_death = (0..count)
                    .filter(|&reader| nodes[reader].reads.contains(&value))
                    .map(|reader| position[reader])
                    .max()
                    .unwrap_or(0);
                (value_death == slot).then_some(register)
            })
        });
        let is_free = |register: u8, live: &[(u8, usize)], include_same_cycle: bool| -> bool {
            live.iter().all(|&(taken, taken_death)| {
                taken != register || taken_death < slot || (include_same_cycle && taken_death == slot)
            })
        };
        let pick = match policy.reuse {
            ReuseRule::OwnSourceFirst => own_dying
                .filter(|&register| is_free(register, &live, true))
                .or_else(|| (3..=12).find(|&register| is_free(register, &live, true))),
            ReuseRule::LowestFree => (3..=12).find(|&register| is_free(register, &live, true)),
            ReuseRule::LowestFreeStable => (3..=12)
                .find(|&register| is_free(register, &live, false))
                .or_else(|| (3..=12).find(|&register| is_free(register, &live, true))),
        };
        let register = pick.unwrap_or(0);
        result[node] = Some(register);
        live.push((register, death));
    }
    result
}

/// RegisterPolicy v2 — the fire-284 synthesis: WHOLE-INTERVAL assignment over
/// the linearized order (the lookahead the paradox demanded):
/// - each value's interval = [def slot, last-read slot], params from entry;
/// - the LAST chain's ops prefer r0 when it is free across their interval;
/// - any op whose source DIES AT ITS DEF may write IN PLACE (an open-interval
///   reuse) — taken when that register stays free across the whole interval;
/// - otherwise the lowest volatile (r3..) free across the whole interval.
/// Assignment processes values in ISSUE order but checks conflicts against
/// EVERY other value's interval, including future ones (interval allocation,
/// not greedy-at-issue).
pub fn assign_registers_v2(nodes: &[DagNode], order: &[usize], params: &[(u32, u8)]) -> Vec<Option<u8>> {
    let count = nodes.len();
    let mut consumer_of: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (index, node) in nodes.iter().enumerate() {
        for read in &node.reads {
            if let Some(writer) = (0..index).rev().find(|&w| nodes[w].writes.contains(read)) {
                consumer_of[writer].push(index);
            }
        }
    }
    let position: Vec<usize> = {
        let mut position = vec![0; count];
        for (slot, &node) in order.iter().enumerate() {
            position[node] = slot;
        }
        position
    };
    let chain_of = |mut node: usize| -> usize {
        loop {
            match consumer_of[node].first() {
                Some(&next) => node = next,
                None => return node,
            }
        }
    };
    let last_sink = (0..count).rev().find(|&node| consumer_of[node].is_empty()).unwrap_or(count - 1);
    // Intervals: params [0, last read]; values [def slot, last read slot].
    struct Interval {
        register: Option<u8>,
        start: usize,
        end: usize,
    }
    let mut intervals: Vec<Interval> = Vec::new();
    let mut param_end = |value: u32| -> usize {
        (0..count)
            .filter(|&reader| nodes[reader].reads.contains(&value))
            .map(|reader| position[reader])
            .max()
            .unwrap_or(0)
    };
    for &(value, register) in params {
        intervals.push(Interval { register: Some(register), start: 0, end: param_end(value) });
    }
    let value_interval = |node: usize| -> (usize, usize) {
        let end = consumer_of[node].iter().map(|&reader| position[reader]).max().unwrap_or(position[node]);
        (position[node], end)
    };
    // Process in issue order; assignment sees all existing intervals AND we
    // re-check against them after each placement (future values conflict via
    // their later placement — the in-place preference is what needs care).
    let mut result: Vec<Option<u8>> = vec![None; count];
    let ordered_values: Vec<usize> = order.iter().copied().filter(|&node| !nodes[node].writes.is_empty() && nodes[node].kind != OpKind::Store).collect();
    for &node in &ordered_values {
        let (start, end) = value_interval(node);
        let free_over = |register: u8, intervals: &[Interval], open_start: bool| -> bool {
            intervals.iter().all(|interval| {
                interval.register != Some(register)
                    || interval.end < start
                    || (open_start && interval.end == start)
                    || interval.start > end
            })
        };
        // The source register dying exactly at this def (the in-place candidate):
        // lowest such source.
        let in_place: Option<u8> = nodes[node]
            .reads
            .iter()
            .filter_map(|read| {
                // A param source:
                params
                    .iter()
                    .find(|&&(value, _)| value == *read)
                    .map(|&(_, register)| {
                        let dies_here = param_end(*read) == start;
                        (register, dies_here)
                    })
                    .or_else(|| {
                        // An internal source: its writer's assigned register.
                        (0..count).rev().find(|&w| nodes[w].writes.contains(read)).and_then(|writer| {
                            result[writer].map(|register| {
                                let (_, writer_end) = value_interval(writer);
                                (register, writer_end == start)
                            })
                        })
                    })
            })
            .filter(|&(_, dies_here)| dies_here)
            .map(|(register, _)| register)
            .min();
        let on_last_chain = chain_of(node) == last_sink;
        let pick = if on_last_chain && free_over(0, &intervals, true) {
            Some(0)
        } else if let Some(register) = in_place.filter(|&register| free_over(register, &intervals, true)) {
            Some(register)
        } else {
            (3..=12).find(|&register| free_over(register, &intervals, false))
        };
        let register = pick.unwrap_or(0);
        result[node] = Some(register);
        intervals.push(Interval { register: Some(register), start, end });
    }
    result
}

/// Register model v3 — the fires-284/286 synthesis in full form:
/// - CLOSED-interval scan in issue order (a register frees the slot AFTER its
///   holder's last read; params occupy from entry);
/// - pool r3..r12 for non-r0 values;
/// - the LAST chain's values prefer r0 when free (closed); other chains may
///   use r0 only when their whole interval PRECEDES the last chain's first def;
/// - a FINAL op (feeding a store) may reuse its own dying source's register
///   (an open-interval exception); last-chain finals still prefer r0.
pub fn assign_registers_v3(nodes: &[DagNode], order: &[usize], params: &[(u32, u8)]) -> Vec<Option<u8>> {
    let count = nodes.len();
    let mut consumer_of: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (index, node) in nodes.iter().enumerate() {
        for read in &node.reads {
            if let Some(writer) = (0..index).rev().find(|&w| nodes[w].writes.contains(read)) {
                consumer_of[writer].push(index);
            }
        }
    }
    let position: Vec<usize> = {
        let mut position = vec![0; count];
        for (slot, &node) in order.iter().enumerate() {
            position[node] = slot;
        }
        position
    };
    let chain_of = |mut node: usize| -> usize {
        loop {
            match consumer_of[node].first() {
                Some(&next) => node = next,
                None => return node,
            }
        }
    };
    let last_sink = (0..count).rev().find(|&node| consumer_of[node].is_empty()).unwrap_or(count - 1);
    // RETURN MODE: a consumerless non-store node is the returned value — its
    // register is FORCED to r3 (pre-claimed so in-place checks see it), store
    // chains become r0-eligible, and the parity gate lifts (measured, the
    // five return captures).
    let return_node: Option<usize> = (0..count).find(|&node| {
        consumer_of[node].is_empty() && nodes[node].kind != OpKind::Store && !nodes[node].writes.is_empty()
    });
    let return_mode = return_node.is_some();
    let last_chain_first_def = (0..count)
        .filter(|&node| chain_of(node) == last_sink && nodes[node].kind != OpKind::Store)
        .map(|node| position[node])
        .min()
        .unwrap_or(usize::MAX);
    let param_end = |value: u32| -> usize {
        (0..count)
            .filter(|&reader| nodes[reader].reads.contains(&value))
            .map(|reader| position[reader])
            .max()
            .unwrap_or(0)
    };
    let value_end = |node: usize| -> usize {
        consumer_of[node].iter().map(|&reader| position[reader]).max().unwrap_or(position[node])
    };
    // Occupancies as (register, start, end) with CLOSED ends.
    let mut occupied: Vec<(u8, usize, usize)> = params
        .iter()
        .map(|&(value, register)| (register, 0, param_end(value)))
        .collect();
    let mut result: Vec<Option<u8>> = vec![None; count];
    // Pre-claim the return value's r3 (its occupancy participates in every
    // in-place conflict check below).
    let return_claim_start = return_node.map(|node| position[node]);
    if let Some(return_node) = return_node {
        result[return_node] = Some(3);
        occupied.push((3, position[return_node], value_end(return_node)));
    }
    // r0 RESERVATION (return mode): each non-forbidden return-chain
    // intermediate pre-claims r0 over its interval — a store final whose
    // interval overlaps finds r0 occupied and falls to its own-dying/pool
    // registers (the arbitration), while a disjoint final shares r0
    // serially (measured across the contention captures). A forbidden
    // intermediate (one feeding the return addi) never reserves.
    if return_mode {
        for node in 0..count {
            if Some(node) == return_node
                || nodes[node].kind == OpKind::Store
                || nodes[node].writes.is_empty()
                || nodes[node].forbid_r0
                || chain_of(node) != last_sink
                || result[node].is_some()
            {
                continue;
            }
            result[node] = Some(0);
            occupied.push((0, position[node], value_end(node)));
        }
    }
    for &node in order {
        if result[node].is_some() {
            continue;
        }
        // The value the return op READS hands its register off at the claim
        // boundary: for that node the r3 claim is OPEN (ret2 -> ret3's r3).
        let feeds_return = return_node.is_some_and(|ret| consumer_of[node].contains(&ret));
        if nodes[node].kind == OpKind::Store || nodes[node].writes.is_empty() {
            continue;
        }
        let start = position[node];
        let end = value_end(node);
        let closed_free = |register: u8, occupied: &[(u8, usize, usize)]| -> bool {
            occupied.iter().all(|&(taken, taken_start, taken_end)| {
                taken != register
                    || taken_end < start
                    || taken_start > end
                    || (feeds_return && register == 3 && Some(taken_start) == return_claim_start && taken_start == end)
            })
        };
        // The own dying source (for the in-place exception), split by origin:
        // an INTERNAL source (another op's result) reuses in place always; a
        // PARAM source only in the relaxed regime — at most two chains, a
        // last chain no deeper than two, and never for a mulli result
        // (measured across the whole dataset).
        let chain_count = {
            let mut sinks: Vec<usize> = (0..count)
                .filter(|&candidate| consumer_of[candidate].is_empty() || nodes[candidate].kind == OpKind::Store)
                .map(|_| 0)
                .collect();
            sinks.clear();
            for candidate in 0..count {
                if nodes[candidate].kind == OpKind::Store {
                    sinks.push(candidate);
                }
            }
            sinks.len().max(1)
        };
        let last_chain_depth = (0..count)
            .filter(|&member| chain_of(member) == last_sink && nodes[member].kind != OpKind::Store)
            .count();
        // Param in-place: FINALS reuse anywhere; INTERMEDIATES only in the
        // second dual-issue slot (odd positions) — a first-of-pair intermediate
        // takes a closed-pool register (equal-pair's r5) while the same op
        // second-of-pair reuses (mult_vs_shift's r3). Loads are exempt.
        let node_is_final = consumer_of[node].len() == 1 && nodes[consumer_of[node][0]].kind == OpKind::Store;
        // The mulli exclusion is per-CHAIN (a chain containing a multiply keeps
        // its params closed — cap3 vs cap2); the parity gate applies only in
        // store-only mode (the return captures reuse at even slots).
        let own_chain_has_multiply = (0..count).any(|member| {
            chain_of(member) == chain_of(node) && nodes[member].kind != OpKind::Store && nodes[member].latency >= 3
        });
        // The multiply exclusion is a STORE-ONLY-mode rule: in return mode a
        // contended store-chain mulli reuses its dying param in place
        // (measured: mulli r4,r4,3 under a mask+or return; the uncontended
        // 1-op-return mulli prefers r0 before reuse is ever consulted).
        let relaxed = chain_count <= 2
            && last_chain_depth <= 2
            && !(own_chain_has_multiply && !return_mode)
            && (return_mode || chain_of(node) != last_sink)
            && (return_mode
                || node_is_final
                || start % 2 == 1
                || nodes[node].kind == OpKind::Load
                || (nodes[node].extension && consumer_of[node].len() == 1));
        let own_dying: Option<u8> = nodes[node]
            .reads
            .iter()
            .filter_map(|read| {
                let from_param = params
                    .iter()
                    .find(|&&(value, _)| value == *read)
                    .map(|&(_, register)| (register, param_end(*read), false));
                let from_internal = (0..count)
                    .rev()
                    .find(|&w| nodes[w].writes.contains(read))
                    .and_then(|writer| result[writer].map(|register| (register, value_end(writer), true)));
                from_param.or(from_internal)
            })
            .filter(|&(_, death, internal)| death == start && (internal || relaxed))
            .map(|(register, _, _)| register)
            .min();
        let open_free = |register: u8, occupied: &[(u8, usize, usize)]| -> bool {
            occupied.iter().all(|&(taken, taken_start, taken_end)| {
                taken != register || taken_end <= start || taken_start > end
            })
        };
        let on_last_chain = chain_of(node) == last_sink;
        let is_final = consumer_of[node].len() == 1 && nodes[consumer_of[node][0]].kind == OpKind::Store;
        // The pool, in mwcc's preference order (measured, 10/10 dataset shapes):
        // the LAST chain's FINAL is forced to r0 (the store staging register);
        // last-chain intermediates and values wholly preceding the last chain
        // slot r0 BETWEEN r4 and r5; everything else never touches r0. A
        // register is candidate when closed-free — or open-free for the op's
        // own dying source (internal sources always; params in the relaxed
        // regime only). First candidate in pool order wins.
        let r0_eligible = (return_mode || on_last_chain || end < last_chain_first_def) && !nodes[node].forbid_r0;
        let pool: Vec<u8> = if !return_mode && on_last_chain && is_final && !nodes[node].forbid_r0 {
            vec![0]
        } else if return_mode && is_final && !nodes[node].forbid_r0 {
            // A return-mode STORE-chain final PREFERS r0 (measured on all five
            // return captures); it falls through when r0 is held — by another
            // occupant or by an overlapping return-intermediate RESERVATION.
            let mut pool = vec![0u8, 3, 4];
            pool.extend(5..=12);
            pool
        } else if r0_eligible {
            let mut pool = vec![3u8, 4, 0];
            pool.extend(5..=12);
            pool
        } else {
            (3..=12).collect()
        };
        let pick = pool
            .iter()
            .copied()
            .find(|&register| {
                if own_dying == Some(register) {
                    open_free(register, &occupied)
                } else {
                    closed_free(register, &occupied)
                }
            });
        let register = pick.unwrap_or(0);
        result[node] = Some(register);
        occupied.push((register, start, end));
    }
    result
}

/// DIAGNOSTIC: assign registers processing chains in an EXPLICIT sequence
/// (values within a chain in issue order), with forced positions pre-claimed
/// (a return value's r3; the store-only last chain's final r0) and in-place
/// ALWAYS eligible (conflicts resolve via the sequence). Used by the fitter to
/// enumerate chain orders per fixture and expose the ordering rule.
pub fn assign_registers_sequenced(
    nodes: &[DagNode],
    order: &[usize],
    params: &[(u32, u8)],
    chain_sequence: &[usize],
) -> Vec<Option<u8>> {
    let count = nodes.len();
    let mut consumer_of: Vec<Vec<usize>> = vec![Vec::new(); count];
    for (index, node) in nodes.iter().enumerate() {
        for read in &node.reads {
            if let Some(writer) = (0..index).rev().find(|&w| nodes[w].writes.contains(read)) {
                consumer_of[writer].push(index);
            }
        }
    }
    let position: Vec<usize> = {
        let mut position = vec![0; count];
        for (slot, &node) in order.iter().enumerate() {
            position[node] = slot;
        }
        position
    };
    let chain_of = |mut node: usize| -> usize {
        loop {
            match consumer_of[node].first() {
                Some(&next) => node = next,
                None => return node,
            }
        }
    };
    let param_end = |value: u32| -> usize {
        (0..count)
            .filter(|&reader| nodes[reader].reads.contains(&value))
            .map(|reader| position[reader])
            .max()
            .unwrap_or(0)
    };
    let value_end = |node: usize| -> usize {
        consumer_of[node].iter().map(|&reader| position[reader]).max().unwrap_or(position[node])
    };
    let return_sink: Option<usize> = chain_sequence
        .iter()
        .copied()
        .find(|&sink| nodes[sink].kind != OpKind::Store && consumer_of[sink].is_empty());
    let return_mode = return_sink.is_some();
    let last_store_sink = chain_sequence
        .iter()
        .copied()
        .filter(|&sink| nodes[sink].kind == OpKind::Store)
        .max();
    let mut occupied: Vec<(u8, usize, usize)> =
        params.iter().map(|&(value, register)| (register, 0, param_end(value))).collect();
    let mut result: Vec<Option<u8>> = vec![None; count];
    // Pre-claim forced positions: the return VALUE's r3; store-only, the last
    // chain's final r0.
    let mut forced: Vec<(usize, u8)> = Vec::new();
    for node in 0..count {
        if return_mode && consumer_of[node].is_empty() && nodes[node].kind != OpKind::Store {
            forced.push((node, 3));
        }
        if !return_mode
            && Some(chain_of(node)) == last_store_sink
            && consumer_of[node].len() == 1
            && nodes[consumer_of[node][0]].kind == OpKind::Store
        {
            forced.push((node, 0));
        }
    }
    for &(node, register) in &forced {
        result[node] = Some(register);
        occupied.push((register, position[node], value_end(node)));
    }
    for &sink in chain_sequence {
        let mut members: Vec<usize> = (0..count)
            .filter(|&node| {
                chain_of(node) == sink && nodes[node].kind != OpKind::Store && result[node].is_none()
            })
            .collect();
        members.sort_by_key(|&node| position[node]);
        for node in members {
            let start = position[node];
            let end = value_end(node);
            let closed_free = |register: u8, occupied: &[(u8, usize, usize)]| -> bool {
                occupied.iter().all(|&(taken, s, e)| taken != register || e < start || s > end)
            };
            let open_free = |register: u8, occupied: &[(u8, usize, usize)]| -> bool {
                occupied.iter().all(|&(taken, s, e)| taken != register || e <= start || s > end)
            };
            let own_dying: Option<u8> = nodes[node]
                .reads
                .iter()
                .filter_map(|read| {
                    params
                        .iter()
                        .find(|&&(value, _)| value == *read)
                        .map(|&(_, register)| (register, param_end(*read), false))
                        .or_else(|| {
                            (0..count)
                                .rev()
                                .find(|&w| nodes[w].writes.contains(read))
                                .and_then(|writer| result[writer].map(|register| (register, value_end(writer), true)))
                        })
                })
                .filter(|&(_, death, internal)| {
                    death == start && (internal || nodes[node].latency < 3)
                })
                .map(|(register, _, _)| register)
                .min();
            let r0_eligible = return_mode || Some(chain_of(node)) == last_store_sink || {
                let last_first_def = last_store_sink
                    .map(|sink| {
                        (0..count)
                            .filter(|&member| chain_of(member) == sink && nodes[member].kind != OpKind::Store)
                            .map(|member| position[member])
                            .min()
                            .unwrap_or(usize::MAX)
                    })
                    .unwrap_or(usize::MAX);
                end < last_first_def
            };
            let pool: Vec<u8> = if r0_eligible {
                let mut pool = vec![3u8, 4, 0];
                pool.extend(5..=12);
                pool
            } else {
                (3..=12).collect()
            };
            let pick = pool.iter().copied().find(|&register| {
                if own_dying == Some(register) {
                    open_free(register, &occupied)
                } else {
                    closed_free(register, &occupied)
                }
            });
            let register = pick.unwrap_or(0);
            result[node] = Some(register);
            occupied.push((register, start, end));
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Latencies used by the fixtures (the module table).
    const ALU: u32 = 1;
    const LOAD: u32 = 2;
    const MUL: u32 = 3;
    const DIV: u32 = 19;
    const STORE: u32 = 1;

    fn labels(nodes: &[DagNode]) -> Vec<&'static str> {
        linearize(nodes).into_iter().map(|index| nodes[index].label).collect()
    }

    /// The dataset as (name, DAG, expected order) — the fitter's ground truth.
    /// Stores carry their kind so kind-ranked keys can act on them.
    fn fixtures() -> Vec<(&'static str, Vec<DagNode>, Vec<&'static str>)> {
        use OpKind::Store as St;
        vec![
            (
                "mult_vs_shift",
                vec![
                    DagNode::new("addi_g", ALU).reads(&[1]).writes(&[10]),
                    DagNode::new("slwi_g", ALU).reads(&[10]).writes(&[11]),
                    DagNode::new("stw_g", STORE).kind(St).reads(&[11]),
                    DagNode::new("addi_h", ALU).reads(&[2]).writes(&[20]),
                    DagNode::new("mulli_h", MUL).reads(&[20]).writes(&[21]),
                    DagNode::new("stw_h", STORE).kind(St).reads(&[21]),
                ],
                vec!["addi_h", "addi_g", "mulli_h", "slwi_g", "stw_g", "stw_h"],
            ),
            (
                "three_deep_vs_shallow",
                vec![
                    DagNode::new("addi_g", ALU).reads(&[1]).writes(&[10]),
                    DagNode::new("slwi_g", ALU).reads(&[10]).writes(&[11]),
                    DagNode::new("addi3_g", ALU).reads(&[11]).writes(&[12]),
                    DagNode::new("stw_g", STORE).kind(St).reads(&[12]),
                    DagNode::new("addi_h", ALU).reads(&[2]).writes(&[20]),
                    DagNode::new("stw_h", STORE).kind(St).reads(&[20]),
                ],
                vec!["addi_g", "addi_h", "slwi_g", "stw_h", "addi3_g", "stw_g"],
            ),
            (
                "divide_chain",
                vec![
                    DagNode::new("divw", DIV).reads(&[1, 2]).writes(&[10]),
                    DagNode::new("stw_g", STORE).kind(St).reads(&[10]),
                    DagNode::new("add", ALU).reads(&[1, 2]).writes(&[20]),
                    DagNode::new("stw_h", STORE).kind(St).reads(&[20]),
                ],
                vec!["divw", "add", "stw_h", "stw_g"],
            ),
            (
                "mult_in_expr",
                vec![
                    DagNode::new("lwz", LOAD).reads(&[1]).writes(&[10]),
                    DagNode::new("mulli", MUL).reads(&[2]).writes(&[20]),
                    DagNode::new("add", ALU).reads(&[10, 20]).writes(&[30]),
                ],
                vec!["mulli", "lwz", "add"],
            ),
            (
                "load_vs_mult",
                vec![
                    DagNode::new("lwz", LOAD).reads(&[1]).writes(&[10]),
                    DagNode::new("addi5_g", ALU).reads(&[10]).writes(&[11]),
                    DagNode::new("stw_g", STORE).kind(St).reads(&[11]),
                    DagNode::new("addi_h", ALU).reads(&[2]).writes(&[20]),
                    DagNode::new("mulli_h", MUL).reads(&[20]).writes(&[21]),
                    DagNode::new("stw_h", STORE).kind(St).reads(&[21]),
                ],
                vec!["addi_h", "lwz", "mulli_h", "addi5_g", "stw_g", "stw_h"],
            ),
            (
                "alu_tie_three",
                vec![
                    DagNode::new("a1", ALU).reads(&[1]).writes(&[10]),
                    DagNode::new("st1", STORE).kind(St).reads(&[10]),
                    DagNode::new("a2", ALU).reads(&[2]).writes(&[20]),
                    DagNode::new("st2", STORE).kind(St).reads(&[20]),
                    DagNode::new("a3", ALU).reads(&[3]).writes(&[30]),
                    DagNode::new("st3", STORE).kind(St).reads(&[30]),
                ],
                vec!["a1", "a2", "a3", "st1", "st2", "st3"],
            ),
            (
                "clean_load_pair",
                vec![
                    DagNode::new("lwz", LOAD).reads(&[1]).writes(&[10]),
                    DagNode::new("srawi", ALU).reads(&[2]).writes(&[11]),
                    DagNode::new("add", ALU).reads(&[10, 11]).writes(&[12]),
                    DagNode::new("stw_g", STORE).kind(St).reads(&[12]),
                    DagNode::new("rlwinm", ALU).reads(&[3]).writes(&[20]),
                    DagNode::new("ori", ALU).reads(&[20]).writes(&[21]),
                    DagNode::new("stw_h", STORE).kind(St).reads(&[21]),
                ],
                vec!["lwz", "srawi", "rlwinm", "add", "ori", "stw_g", "stw_h"],
            ),
            (
                "clean_alu_pair",
                vec![
                    DagNode::new("srawi", ALU).reads(&[2]).writes(&[11]),
                    DagNode::new("add", ALU).reads(&[11, 4]).writes(&[12]),
                    DagNode::new("stw_g", STORE).kind(St).reads(&[12]),
                    DagNode::new("rlwinm", ALU).reads(&[3]).writes(&[20]),
                    DagNode::new("ori", ALU).reads(&[20]).writes(&[21]),
                    DagNode::new("stw_h", STORE).kind(St).reads(&[21]),
                ],
                vec!["srawi", "rlwinm", "add", "ori", "stw_g", "stw_h"],
            ),
            (
                "tail_pair",
                vec![
                    DagNode::new("srawi", ALU).reads(&[1]).writes(&[10]),
                    DagNode::new("lwz_e", LOAD).reads(&[2]).writes(&[11]),
                    DagNode::new("add", ALU).reads(&[10, 11]).writes(&[12]),
                    DagNode::new("addi", ALU).reads(&[12]).writes(&[13]),
                    DagNode::new("stw_eptr", STORE).kind(St).reads(&[13]).alias(1),
                    DagNode::new("rlwinm", ALU).reads(&[3]).writes(&[20]),
                    DagNode::new("oris", ALU).reads(&[20]).writes(&[21]),
                    DagNode::new("stfd_spill", STORE).kind(St).alias(2),
                    DagNode::new("stw_slot", STORE).kind(St).reads(&[21]).alias(2),
                ],
                vec!["rlwinm", "srawi", "lwz_e", "oris", "stfd_spill", "add", "addi", "stw_slot", "stw_eptr"],
            ),
            (
                "tail_pair_equal3",
                vec![
                    DagNode::new("srawi", ALU).reads(&[1]).writes(&[10]),
                    DagNode::new("addi", ALU).reads(&[10]).writes(&[11]),
                    DagNode::new("stw_eptr", STORE).kind(St).reads(&[11]).alias(1),
                    DagNode::new("rlwinm", ALU).reads(&[3]).writes(&[20]),
                    DagNode::new("oris", ALU).reads(&[20]).writes(&[21]),
                    DagNode::new("stfd_spill", STORE).kind(St).alias(2),
                    DagNode::new("stw_slot", STORE).kind(St).reads(&[21]).alias(2),
                ],
                // capture: srawi rlwinm stfd addi oris stw_eptr stw_slot
                vec!["srawi", "rlwinm", "stfd_spill", "addi", "oris", "stw_eptr", "stw_slot"],
            ),
        ]
    }

    /// Register fixtures: (name, DAG, params (value,reg), expected register per
    /// NON-STORE node in NODE order) — from the dataset captures.
    fn register_fixtures() -> Vec<(&'static str, Vec<DagNode>, Vec<(u32, u8)>, Vec<Option<u8>>)> {
        use OpKind::Store as St;
        vec![
            (
                // addi r0(h); addi r3(g in place); mulli r0,r0; slwi r3,r3
                "mult_vs_shift",
                vec![
                    DagNode::new("addi_g", ALU).reads(&[1]).writes(&[10]),
                    DagNode::new("slwi_g", ALU).reads(&[10]).writes(&[11]),
                    DagNode::new("stw_g", STORE).kind(St).reads(&[11]),
                    DagNode::new("addi_h", ALU).reads(&[2]).writes(&[20]),
                    DagNode::new("mulli_h", MUL).reads(&[20]).writes(&[21]),
                    DagNode::new("stw_h", STORE).kind(St).reads(&[21]),
                ],
                vec![(1, 3), (2, 4)],
                vec![Some(3), Some(3), None, Some(0), Some(0), None],
            ),
            (
                // addi r0(h1); addi r4(g!); mulli r3(h2 bounce); addi r0(h3)
                "three_op_last_chain",
                vec![
                    DagNode::new("addi_g", ALU).reads(&[1]).writes(&[10]),
                    DagNode::new("stw_g", STORE).kind(St).reads(&[10]),
                    DagNode::new("addi_h1", ALU).reads(&[2]).writes(&[20]),
                    DagNode::new("mulli_h2", MUL).reads(&[20]).writes(&[21]),
                    DagNode::new("addi_h3", ALU).reads(&[21]).writes(&[22]),
                    DagNode::new("stw_h", STORE).kind(St).reads(&[22]),
                ],
                vec![(1, 3), (2, 4)],
                vec![Some(4), None, Some(0), Some(3), Some(0), None],
            ),
            (
                // a1 r6; a2 r3; a3 r0
                "alu_tie_three",
                vec![
                    DagNode::new("a1", ALU).reads(&[1]).writes(&[10]),
                    DagNode::new("st1", STORE).kind(St).reads(&[10]),
                    DagNode::new("a2", ALU).reads(&[2]).writes(&[20]),
                    DagNode::new("st2", STORE).kind(St).reads(&[20]),
                    DagNode::new("a3", ALU).reads(&[3]).writes(&[30]),
                    DagNode::new("st3", STORE).kind(St).reads(&[30]),
                ],
                vec![(1, 3), (2, 4), (3, 5)],
                vec![Some(6), None, Some(3), None, Some(0), None],
            ),
            (
                // mulli r5(g); srawi r3(h1); addi r0(h2)
                "mulli_srawi_pair",
                vec![
                    DagNode::new("mulli_g", MUL).reads(&[1]).writes(&[10]),
                    DagNode::new("stw_g", STORE).kind(St).reads(&[10]),
                    DagNode::new("srawi_h", ALU).hazard(HAZARD_XER).reads(&[2]).writes(&[20]),
                    DagNode::new("addi_h", ALU).reads(&[20]).writes(&[21]),
                    DagNode::new("stw_h", STORE).kind(St).reads(&[21]),
                ],
                vec![(1, 3), (2, 4)],
                vec![Some(5), None, Some(3), Some(0), None],
            ),
            (
                // lwz r3(reuses base); srawi r4(in place); add r3; rlwinm r0; ori r0
                "clean_load_pair",
                vec![
                    DagNode::new("lwz", LOAD).reads(&[1]).writes(&[10]),
                    DagNode::new("srawi", ALU).reads(&[2]).writes(&[11]),
                    DagNode::new("add", ALU).reads(&[10, 11]).writes(&[12]),
                    DagNode::new("stw_g", STORE).kind(St).reads(&[12]),
                    DagNode::new("rlwinm", ALU).reads(&[3]).writes(&[20]),
                    DagNode::new("ori", ALU).reads(&[20]).writes(&[21]),
                    DagNode::new("stw_h", STORE).kind(St).reads(&[21]),
                ],
                vec![(1, 3), (2, 4), (3, 5)],
                vec![Some(3), Some(4), Some(3), None, Some(0), Some(0), None],
            ),
            (
                // mulli r0 (single statement)
                "single_mulli_store",
                vec![
                    DagNode::new("mulli", MUL).reads(&[1]).writes(&[10]),
                    DagNode::new("stw", STORE).kind(St).reads(&[10]),
                ],
                vec![(1, 3)],
                vec![Some(0), None],
            ),
        ]
    }

    /// The newer capture shapes, register-hardening round (fire 289). MUL ops
    /// gate at 2 while weighing 3 (the split the store orders demand).
    fn register_fixtures_round2() -> Vec<(&'static str, Vec<DagNode>, Vec<(u32, u8)>, Vec<Option<u8>>)> {
        use OpKind::Store as St;
        vec![
            (
                // h1 r0; g1 r4; h2 r3; k1 r0; g2 r4(in place); k2 r0(in place)
                "three_two_op_chains",
                vec![
                    DagNode::new("g1", ALU).reads(&[1]).writes(&[10]),
                    DagNode::new("g2", ALU).reads(&[10]).writes(&[11]),
                    DagNode::new("st_g", STORE).kind(St).reads(&[11]),
                    DagNode::new("h1", ALU).reads(&[2]).writes(&[20]),
                    DagNode::new("h2", MUL).gate(2).reads(&[20]).writes(&[21]),
                    DagNode::new("st_h", STORE).kind(St).reads(&[21]),
                    DagNode::new("k1", ALU).reads(&[3]).writes(&[30]),
                    DagNode::new("k2", ALU).reads(&[30]).writes(&[31]),
                    DagNode::new("st_k", STORE).kind(St).reads(&[31]),
                ],
                vec![(1, 3), (2, 4), (3, 5)],
                vec![Some(4), Some(4), None, Some(0), Some(3), None, Some(0), Some(0), None],
            ),
            (
                // srawi_g r5; srawi_h r3; addi_g r4; addi_h r0
                "equal_two_op_pair",
                vec![
                    DagNode::new("srawi_g", ALU).hazard(HAZARD_XER).reads(&[1]).writes(&[10]),
                    DagNode::new("addi_g", ALU).reads(&[10]).writes(&[11]),
                    DagNode::new("st_g", STORE).kind(St).reads(&[11]),
                    DagNode::new("srawi_h", ALU).hazard(HAZARD_XER).reads(&[2]).writes(&[20]),
                    DagNode::new("addi_h", ALU).reads(&[20]).writes(&[21]),
                    DagNode::new("st_h", STORE).kind(St).reads(&[21]),
                ],
                vec![(1, 3), (2, 4)],
                vec![Some(5), Some(4), None, Some(3), Some(0), None],
            ),
            (
                // g addi r3 (in place, relaxed); h1 addi r0; h2 mulli r0
                "one_op_g_two_op_h",
                vec![
                    DagNode::new("g1", ALU).reads(&[1]).writes(&[10]),
                    DagNode::new("st_g", STORE).kind(St).reads(&[10]),
                    DagNode::new("h1", ALU).reads(&[2]).writes(&[20]),
                    DagNode::new("h2", MUL).gate(2).reads(&[20]).writes(&[21]),
                    DagNode::new("st_h", STORE).kind(St).reads(&[21]),
                ],
                vec![(1, 3), (2, 4)],
                vec![Some(3), None, Some(0), Some(0), None],
            ),
            (
                // h1 r0; g1 r4; h2 r3; g2 r4(in place); h3 r0
                "two_mulli_chains",
                vec![
                    DagNode::new("g1", ALU).reads(&[1]).writes(&[10]),
                    DagNode::new("g2", MUL).gate(2).reads(&[10]).writes(&[11]),
                    DagNode::new("st_g", STORE).kind(St).reads(&[11]),
                    DagNode::new("h1", ALU).reads(&[2]).writes(&[20]),
                    DagNode::new("h2", MUL).gate(2).reads(&[20]).writes(&[21]),
                    DagNode::new("h3", ALU).reads(&[21]).writes(&[22]),
                    DagNode::new("st_h", STORE).kind(St).reads(&[22]),
                ],
                vec![(1, 3), (2, 4)],
                vec![Some(4), Some(4), None, Some(0), Some(3), Some(0), None],
            ),
        ]
    }

    /// Round 3: the RETURN-MODE fixtures (a consumerless non-store node is the
    /// returned value, forced to r3).
    /// Round 4: narrow-parameter EXTENSION nodes in void bodies (fires 302-303).
    /// A single-consumer extension reuses its dying param register in place
    /// (extsb r3,r3); a multi-consumer one takes the next closed-free register
    /// and the first chain's final claims the freed param home.
    fn register_fixtures_round4() -> Vec<(&'static str, Vec<DagNode>, Vec<(u32, u8)>, Vec<Option<u8>>)> {
        use OpKind::Store as St;
        vec![
            (
                // char a; g=a+1; h=a+2;  ->  extsb r4; addi r3; addi r0 (capture)
                "ext_shared_two_chains",
                vec![
                    DagNode::new("extsb", ALU).extension().forbid_r0().reads(&[1]).writes(&[10]),
                    DagNode::new("addi_g", ALU).reads(&[10]).writes(&[11]),
                    DagNode::new("st_g", STORE).kind(St).reads(&[11]),
                    DagNode::new("addi_h", ALU).reads(&[10]).writes(&[12]),
                    DagNode::new("st_h", STORE).kind(St).reads(&[12]),
                ],
                vec![(1, 3)],
                vec![Some(4), Some(3), None, Some(0), None],
            ),
            (
                // char a, int b; g=a+1; h=b+2;  ->  extsb r3,r3 (in place); addi_h r0; addi_g r3
                "ext_single_reuses_in_place",
                vec![
                    DagNode::new("extsb", ALU).extension().forbid_r0().reads(&[1]).writes(&[10]),
                    DagNode::new("addi_g", ALU).reads(&[10]).writes(&[11]),
                    DagNode::new("st_g", STORE).kind(St).reads(&[11]),
                    DagNode::new("addi_h", ALU).reads(&[2]).writes(&[12]),
                    DagNode::new("st_h", STORE).kind(St).reads(&[12]),
                ],
                vec![(1, 3), (2, 4)],
                vec![Some(3), Some(3), None, Some(0), None],
            ),
        ]
    }

    /// Round 5: r0 tenancy arbitration (fire 304). The store-chain multiply
    /// final yields r0 to a shorter-tenancy return intermediate; a forbidden
    /// intermediate (feeding the return addi) never contends.
    fn register_fixtures_round5() -> Vec<(&'static str, Vec<DagNode>, Vec<(u32, u8)>, Vec<Option<u8>>)> {
        use OpKind::Store as St;
        vec![
            (
                // g=a*100; return (b&0x7fff)|1;  ->  mulli r5; mask r0; ori r3
                "contend_mask_or",
                vec![
                    DagNode::new("mulli_g", MUL).gate(2).reads(&[1]).writes(&[10]),
                    DagNode::new("st_g", STORE).kind(St).reads(&[10]),
                    DagNode::new("mask", ALU).reads(&[2]).writes(&[20]),
                    DagNode::new("ori_ret", ALU).reads(&[20]).writes(&[21]),
                ],
                vec![(1, 3), (2, 4)],
                vec![Some(5), None, Some(0), Some(3)],
            ),
            (
                // g=a*100; return (b+1)*3;  ->  mulli r5; addi r0; mulli r3
                "contend_ret_mulli",
                vec![
                    DagNode::new("mulli_g", MUL).gate(2).reads(&[1]).writes(&[10]),
                    DagNode::new("st_g", STORE).kind(St).reads(&[10]),
                    DagNode::new("addi_r", ALU).reads(&[2]).writes(&[20]),
                    DagNode::new("mulli_ret", MUL).gate(2).reads(&[20]).writes(&[21]),
                ],
                vec![(1, 3), (2, 4)],
                vec![Some(5), None, Some(0), Some(3)],
            ),
            (
                // g=a+1; return (b&0x7fff)|1;  ->  the store final yields r0 with
                // EQUAL tenancies too: addi r3 in place; mask r0; ori r3
                "contend_equal_tenancy",
                vec![
                    DagNode::new("addi_g", ALU).reads(&[1]).writes(&[10]),
                    DagNode::new("st_g", STORE).kind(St).reads(&[10]),
                    DagNode::new("mask", ALU).reads(&[2]).writes(&[20]),
                    DagNode::new("ori_ret", ALU).reads(&[20]).writes(&[21]),
                ],
                vec![(1, 3), (2, 4)],
                vec![Some(3), None, Some(0), Some(3)],
            ),
            (
                // g=a*100; return (b>>2)+1;  ->  mulli KEEPS r0 (srawi is forbidden)
                "no_contest_forbidden",
                vec![
                    DagNode::new("mulli_g", MUL).gate(2).reads(&[1]).writes(&[10]),
                    DagNode::new("st_g", STORE).kind(St).reads(&[10]),
                    DagNode::new("srawi", ALU).hazard(HAZARD_XER).forbid_r0().reads(&[2]).writes(&[20]),
                    DagNode::new("addi_ret", ALU).reads(&[20]).writes(&[21]),
                ],
                vec![(1, 3), (2, 4)],
                vec![Some(0), None, Some(3), Some(3)],
            ),
        ]
    }

    fn register_fixtures_round3() -> Vec<(&'static str, Vec<DagNode>, Vec<(u32, u8)>, Vec<Option<u8>>)> {
        use OpKind::Store as St;
        vec![
            (
                // g=a+1; return b+2;  ->  g r0, ret r3
                "ret_after_store",
                vec![
                    DagNode::new("g1", ALU).reads(&[1]).writes(&[10]),
                    DagNode::new("st_g", STORE).kind(St).reads(&[10]),
                    DagNode::new("ret", ALU).reads(&[2]).writes(&[20]),
                ],
                vec![(1, 3), (2, 4)],
                vec![Some(0), None, Some(3)],
            ),
            (
                // g=(a+1)*2; return (b+2)*3;  ->  g1 r3, g2 r4, ret1 r0, ret2 r3
                "ret_both_deep",
                vec![
                    DagNode::new("g1", ALU).reads(&[1]).writes(&[10]),
                    DagNode::new("g2", ALU).reads(&[10]).writes(&[11]),
                    DagNode::new("st_g", STORE).kind(St).reads(&[11]),
                    DagNode::new("ret1", ALU).reads(&[2]).writes(&[20]),
                    DagNode::new("ret2", MUL).gate(2).reads(&[20]).writes(&[21]),
                ],
                vec![(1, 3), (2, 4)],
                vec![Some(3), Some(4), None, Some(0), Some(3)],
            ),
            (
                // g=(b+2)*3; return a+1;  ->  g1 r0, g2 r0, ret r3
                "ret_first_source",
                vec![
                    DagNode::new("g1", ALU).reads(&[2]).writes(&[10]),
                    DagNode::new("g2", MUL).gate(2).reads(&[10]).writes(&[11]),
                    DagNode::new("st_g", STORE).kind(St).reads(&[11]),
                    DagNode::new("ret", ALU).reads(&[1]).writes(&[20]),
                ],
                vec![(1, 3), (2, 4)],
                vec![Some(0), Some(0), None, Some(3)],
            ),
            (
                // g=(a>>1)+5; return (b>>2)+7;  ->  g1 r3, g2 r0, ret1 r3, ret2 r3
                "ret_equal_twin",
                vec![
                    DagNode::new("g1", ALU).hazard(HAZARD_XER).reads(&[1]).writes(&[10]),
                    DagNode::new("g2", ALU).reads(&[10]).writes(&[11]),
                    DagNode::new("st_g", STORE).kind(St).reads(&[11]),
                    DagNode::new("ret1", ALU).hazard(HAZARD_XER).reads(&[2]).writes(&[20]),
                    DagNode::new("ret2", ALU).reads(&[20]).writes(&[21]),
                ],
                vec![(1, 3), (2, 4)],
                vec![Some(3), Some(0), None, Some(3), Some(3)],
            ),
            (
                // g=(a+1)*2; return ((b+2)*3)+4;  ->  g1 r4, g2 r0, ret1 r0, ret2 r3, ret3 r3
                "ret_deep_chain",
                vec![
                    DagNode::new("g1", ALU).reads(&[1]).writes(&[10]),
                    DagNode::new("g2", ALU).reads(&[10]).writes(&[11]),
                    DagNode::new("st_g", STORE).kind(St).reads(&[11]),
                    DagNode::new("ret1", ALU).reads(&[2]).writes(&[20]),
                    DagNode::new("ret2", MUL).gate(2).reads(&[20]).writes(&[21]),
                    DagNode::new("ret3", ALU).reads(&[21]).writes(&[22]),
                ],
                vec![(1, 3), (2, 4)],
                vec![Some(4), Some(0), None, Some(0), Some(3), Some(3)],
            ),
        ]
    }

    /// CHAIN-ORDER DIAGNOSTIC: for each register fixture, try every chain
    /// permutation through assign_registers_sequenced and print the passing
    /// ones — the ordering rule should be visible across fixtures.
    /// Run: `cargo test -p mwcc-vreg chain_order_diagnostic -- --ignored --nocapture`.
    #[test]
    #[ignore = "diagnostic; run with --nocapture"]
    fn chain_order_diagnostic() {
        fn permutations(items: &[usize]) -> Vec<Vec<usize>> {
            if items.len() <= 1 {
                return vec![items.to_vec()];
            }
            let mut all = Vec::new();
            for (index, &head) in items.iter().enumerate() {
                let mut rest = items.to_vec();
                rest.remove(index);
                for mut tail in permutations(&rest) {
                    tail.insert(0, head);
                    all.push(tail);
                }
            }
            all
        }
        let mut shapes = register_fixtures();
        shapes.extend(register_fixtures_round2());
        for (name, nodes, params, expected) in &shapes {
            let order = linearize(nodes);
            let sinks: Vec<usize> = (0..nodes.len())
                .filter(|&node| nodes[node].kind == OpKind::Store)
                .collect();
            let passing: Vec<String> = permutations(&sinks)
                .into_iter()
                .filter(|sequence| assign_registers_sequenced(nodes, &order, params, sequence) == *expected)
                .map(|sequence| format!("{sequence:?}"))
                .collect();
            println!("{name}: sinks {sinks:?} passing {passing:?}");
        }
    }

    /// THE REGISTER FITTER: enumerate policies against the register fixtures.
    /// Run: `cargo test -p mwcc-vreg register_fitter -- --ignored --nocapture`.
    #[test]
    #[ignore = "the register-policy search instrument; run with --nocapture"]
    fn register_fitter() {
        let shapes = register_fixtures();
        let mut best: (usize, Option<RegisterPolicy>) = (0, None);
        for r0_rule in [R0Rule::FinalOfLastChain, R0Rule::FinalPlusTwoOpChain, R0Rule::WholeLastChain] {
            for reuse in [ReuseRule::LowestFree, ReuseRule::LowestFreeStable, ReuseRule::OwnSourceFirst] {
                let policy = RegisterPolicy { r0_rule, reuse };
                let passed = shapes
                    .iter()
                    .filter(|(_, nodes, params, expected)| {
                        let order = linearize(nodes);
                        assign_registers(nodes, &order, params, policy) == *expected
                    })
                    .count();
                println!("{policy:?}: {passed}/{}", shapes.len());
                if passed > best.0 {
                    best = (passed, Some(policy));
                }
            }
        }
        // Score the v2 interval model too.
        let v2_passed = shapes
            .iter()
            .filter(|(_, nodes, params, expected)| {
                let order = linearize(nodes);
                assign_registers_v2(nodes, &order, params) == *expected
            })
            .count();
        println!("v2 interval model: {v2_passed}/{}", shapes.len());
        let v3_passed = shapes
            .iter()
            .filter(|(_, nodes, params, expected)| {
                let order = linearize(nodes);
                assign_registers_v3(nodes, &order, params) == *expected
            })
            .count();
        println!("v3 closed+r0-last model: {v3_passed}/{}", shapes.len());
        let round2 = register_fixtures_round2();
        let round2_passed = round2
            .iter()
            .filter(|(_, nodes, params, expected)| {
                let order = linearize(nodes);
                assign_registers_v3(nodes, &order, params) == *expected
            })
            .count();
        println!("v3 round2: {round2_passed}/{}", round2.len());
        let round3 = register_fixtures_round3();
        let round3_passed = round3
            .iter()
            .filter(|(_, nodes, params, expected)| {
                let order = linearize(nodes);
                assign_registers_v3(nodes, &order, params) == *expected
            })
            .count();
        println!("v3 round3 (returns): {round3_passed}/{}", round3.len());
        let round4 = register_fixtures_round4();
        for (name, nodes, params, expected) in &round4 {
            let order = linearize(nodes);
            let got = assign_registers_v3(nodes, &order, params);
            assert_eq!(got, *expected, "round4 fixture {name}");
        }
        println!("v3 round4 (extensions): {}/{}", round4.len(), round4.len());
        let round5 = register_fixtures_round5();
        for (name, nodes, params, expected) in &round5 {
            let order = linearize(nodes);
            let got = assign_registers_v3(nodes, &order, params);
            assert_eq!(got, *expected, "round5 fixture {name}");
        }
        println!("v3 round5 (r0 arbitration): {}/{}", round5.len(), round5.len());
        for (name, nodes, params, expected) in &round3 {
            let order = linearize(nodes);
            let order_labels: Vec<&str> = order.iter().map(|&index| nodes[index].label).collect();
            let got = assign_registers_v3(nodes, &order, params);
            if got != *expected {
                println!("  r3 FAIL {name}: order {order_labels:?}\n     got {got:?}\n    want {expected:?}");
            }
        }
        for (name, nodes, params, expected) in &round2 {
            let order = linearize(nodes);
            let order_labels: Vec<&str> = order.iter().map(|&index| nodes[index].label).collect();
            let got = assign_registers_v3(nodes, &order, params);
            if got != *expected {
                println!("  r2 FAIL {name}: order {order_labels:?}\n     got {got:?}\n    want {expected:?}");
            }
        }
        for (name, nodes, params, expected) in &shapes {
            let order = linearize(nodes);
            let got = assign_registers_v3(nodes, &order, params);
            if got != *expected {
                println!("  v3 FAIL {name}: got {got:?}\n               want {expected:?}");
            }
        }
        for (name, nodes, params, expected) in &shapes {
            let order = linearize(nodes);
            let got = assign_registers_v2(nodes, &order, params);
            if got != *expected {
                println!("  v2 FAIL {name}: got {got:?}\n               want {expected:?}");
            }
        }
        if let Some(policy) = best.1 {
            println!("best {}/{}: {policy:?}", best.0, shapes.len());
            for (name, nodes, params, expected) in &shapes {
                let order = linearize(nodes);
                let got = assign_registers(nodes, &order, params, policy);
                if got != *expected {
                    println!("  FAIL {name}: got {got:?}\n            want {expected:?}");
                }
            }
        }
    }

    /// THE FITTER: enumerate candidate models, report every one that matches
    /// all fixtures. Run manually: `cargo test -p mwcc-vreg fitter -- --ignored --nocapture`.
    #[test]
    #[ignore = "the model-search instrument; run with --nocapture"]
    fn fitter() {
        let shapes = fixtures();
        let mut winners = Vec::new();
        let mut best: (usize, Option<Model>) = (0, None);
        for issue_width in [1usize, 2] {
            for gate_on_complete in [true, false] {
                for gated_last in [false, true] {
                    for weight_before_kind in [true, false] {
                        for kind_rank in [
                            [0u8, 0, 0],
                            [0, 1, 1],
                            [0, 1, 2],
                            [0, 2, 1],
                            [1, 0, 2],
                            [1, 2, 0],
                            [0, 0, 1],
                            [1, 0, 0],
                        ] {
                        for strategy in [
                            Strategy::GlobalKey,
                            Strategy::ChainRobin { lead: LeadRule::Heaviest, offer_non_load_first: false },
                            Strategy::ChainRobin { lead: LeadRule::Heaviest, offer_non_load_first: true },
                            Strategy::ChainRobin { lead: LeadRule::LoadFreeFirst, offer_non_load_first: false },
                            Strategy::ChainRobin { lead: LeadRule::LoadFreeFirst, offer_non_load_first: true },
                            Strategy::ChainRobin { lead: LeadRule::Alternating, offer_non_load_first: false },
                            Strategy::ChainRobin { lead: LeadRule::Alternating, offer_non_load_first: true },
                        ] {
                            let model = Model { issue_width, gate_on_complete, gated_last, kind_rank, weight_before_kind, strategy };
                            let passed = shapes
                                .iter()
                                .filter(|(_, nodes, expected)| {
                                    let order: Vec<&str> =
                                        linearize_with(nodes, model).into_iter().map(|index| nodes[index].label).collect();
                                    order == *expected
                                })
                                .count();
                            if passed == shapes.len() {
                                winners.push(model);
                            }
                            if passed == shapes.len() - 1 {
                                let failing: Vec<&str> = shapes
                                    .iter()
                                    .filter(|(_, nodes, expected)| {
                                        let order: Vec<&str> = linearize_with(nodes, model)
                                            .into_iter()
                                            .map(|index| nodes[index].label)
                                            .collect();
                                        order != *expected
                                    })
                                    .map(|(name, _, _)| *name)
                                    .collect();
                                println!("near: fails {:?} — {:?}", failing, model.strategy);
                            }
                            if passed > best.0 {
                                best = (passed, Some(model));
                            }
                        }
                        }
                    }
                }
            }
        }
        println!("fixtures: {}", shapes.len());
        println!("winners ({}):", winners.len());
        for model in &winners {
            println!("  {model:?}");
        }
        if winners.is_empty() {
            println!("best: {}/{} with {:?}", best.0, shapes.len(), best.1);
            if let Some(model) = best.1 {
                for (name, nodes, expected) in &shapes {
                    let order: Vec<&str> =
                        linearize_with(nodes, model).into_iter().map(|index| nodes[index].label).collect();
                    if order != *expected {
                        println!("  FAIL {name}: got {order:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn mult_vs_shift_starts_the_mulli_chain() {
        // g = (a+1)*2; h = (b+2)*3;  ->  addi(h) addi(g) mulli slwi stw(g) stw(h)
        let nodes = [
            DagNode::new("addi_g", ALU).reads(&[1]).writes(&[10]),
            DagNode::new("slwi_g", ALU).reads(&[10]).writes(&[11]),
            DagNode::new("stw_g", STORE).reads(&[11]),
            DagNode::new("addi_h", ALU).reads(&[2]).writes(&[20]),
            DagNode::new("mulli_h", MUL).reads(&[20]).writes(&[21]),
            DagNode::new("stw_h", STORE).reads(&[21]),
        ];
        assert_eq!(labels(&nodes), ["addi_h", "addi_g", "mulli_h", "slwi_g", "stw_g", "stw_h"]);
    }

    #[test]
    fn three_deep_vs_shallow_reorders_the_stores() {
        // g = ((a+1)*2)+3; h = b+4;  ->  addi(g) addi(h) slwi stw(h) addi3 stw(g)
        let nodes = [
            DagNode::new("addi_g", ALU).reads(&[1]).writes(&[10]),
            DagNode::new("slwi_g", ALU).reads(&[10]).writes(&[11]),
            DagNode::new("addi3_g", ALU).reads(&[11]).writes(&[12]),
            DagNode::new("stw_g", STORE).reads(&[12]),
            DagNode::new("addi_h", ALU).reads(&[2]).writes(&[20]),
            DagNode::new("stw_h", STORE).reads(&[20]),
        ];
        assert_eq!(labels(&nodes), ["addi_g", "addi_h", "slwi_g", "stw_h", "addi3_g", "stw_g"]);
    }

    #[test]
    fn divide_chain_delays_the_div_store() {
        // g = a/b; h = a+b;  ->  divw add stw(h) stw(g)
        let nodes = [
            DagNode::new("divw", DIV).reads(&[1, 2]).writes(&[10]),
            DagNode::new("stw_g", STORE).reads(&[10]),
            DagNode::new("add", ALU).reads(&[1, 2]).writes(&[20]),
            DagNode::new("stw_h", STORE).reads(&[20]),
        ];
        assert_eq!(labels(&nodes), ["divw", "add", "stw_h", "stw_g"]);
    }

    #[test]
    fn two_mults_keep_source_order() {
        // g = a*5; h = b*7;  ->  mulli mulli stw stw
        let nodes = [
            DagNode::new("mulli_g", MUL).reads(&[1]).writes(&[10]),
            DagNode::new("stw_g", STORE).reads(&[10]),
            DagNode::new("mulli_h", MUL).reads(&[2]).writes(&[20]),
            DagNode::new("stw_h", STORE).reads(&[20]),
        ];
        assert_eq!(labels(&nodes), ["mulli_g", "mulli_h", "stw_g", "stw_h"]);
    }

    #[test]
    fn load_vs_mult_prioritizes_the_deeper_alu_chain() {
        // g = *p + 5; h = (b+2)*3;  ->  addi(h) lwz mulli addi5 stw(g) stw(h)
        let nodes = [
            DagNode::new("lwz", LOAD).reads(&[1]).writes(&[10]),
            DagNode::new("addi5_g", ALU).reads(&[10]).writes(&[11]),
            DagNode::new("stw_g", STORE).reads(&[11]),
            DagNode::new("addi_h", ALU).reads(&[2]).writes(&[20]),
            DagNode::new("mulli_h", MUL).reads(&[20]).writes(&[21]),
            DagNode::new("stw_h", STORE).reads(&[21]),
        ];
        assert_eq!(labels(&nodes), ["addi_h", "lwz", "mulli_h", "addi5_g", "stw_g", "stw_h"]);
    }

    #[test]
    fn alu_ties_keep_source_order() {
        // g = a+1; h = b+2; k = c+3;  ->  a1 a2 a3 st1 st2 st3
        let nodes = [
            DagNode::new("a1", ALU).reads(&[1]).writes(&[10]),
            DagNode::new("st1", STORE).reads(&[10]),
            DagNode::new("a2", ALU).reads(&[2]).writes(&[20]),
            DagNode::new("st2", STORE).reads(&[20]),
            DagNode::new("a3", ALU).reads(&[3]).writes(&[30]),
            DagNode::new("st3", STORE).reads(&[30]),
        ];
        assert_eq!(labels(&nodes), ["a1", "a2", "a3", "st1", "st2", "st3"]);
    }

    #[test]
    fn mult_in_expr_issues_before_the_load()
    {
        // return *p + (a*3);  ->  mulli lwz add
        let nodes = [
            DagNode::new("lwz", LOAD).reads(&[1]).writes(&[10]),
            DagNode::new("mulli", MUL).reads(&[2]).writes(&[20]),
            DagNode::new("add", ALU).reads(&[10, 20]).writes(&[30]),
        ];
        assert_eq!(labels(&nodes), ["mulli", "lwz", "add"]);
    }

    #[test]
    fn aliasing_stores_keep_program_order() {
        // g = *p; *q = a;  ->  lwz stw(g) stw(*q)  (the pointer store may alias)
        let nodes = [
            DagNode::new("lwz", LOAD).reads(&[1]).writes(&[10]),
            DagNode::new("stw_g", STORE).reads(&[10]).alias(0),
            DagNode::new("stw_q", STORE).reads(&[2]).alias(0),
        ];
        assert_eq!(labels(&nodes), ["lwz", "stw_g", "stw_q"]);
    }

    #[test]
    fn float_chains_load_both_constants_first() {
        // gd = a*2.5; hd = b+1.5;  ->  lfd lfd fmul fadd stfd(g) stfd(h)
        const FP: u32 = 3;
        let nodes = [
            DagNode::new("lfd_c1", LOAD).writes(&[10]),
            DagNode::new("fmul", FP).reads(&[1, 10]).writes(&[11]),
            DagNode::new("stfd_g", STORE).reads(&[11]),
            DagNode::new("lfd_c2", LOAD).writes(&[20]),
            DagNode::new("fadd", FP).reads(&[2, 20]).writes(&[21]),
            DagNode::new("stfd_h", STORE).reads(&[21]),
        ];
        assert_eq!(labels(&nodes), ["lfd_c1", "lfd_c2", "fmul", "fadd", "stfd_g", "stfd_h"]);
    }

    /// THE MODEL'S KNOWN BOUNDARY (fire 278): the frexp tail pair. Measured
    /// order (statement-order-INDEPENDENT): rlwinm, srawi, lwz, oris, stfd,
    /// add, addi, stw_slot, stw_eptr — the r0-staging chain (rlwinm->oris->
    /// stw) STARTS FIRST despite the LOWEST critical-path weight, and the lwz
    /// (heaviest path) issues third. Dual-issue critical-path predicts
    /// lwz+srawi first. Hypotheses for v4: staging-resource-first priority
    /// (start the r0 bottleneck early), or register-pressure coupling (ops
    /// reading dying registers first, allocating loads later). Un-ignore when
    /// the model explains it WITHOUT breaking the ten passing fixtures.
    #[test]
    #[ignore = "model v3 boundary: the staging-chain-first priority is unexplained"]
    fn tail_pair_starts_the_staging_chain() {
        let nodes = [
            // s1: *eptr += (ix >> 20) - 1022
            DagNode::new("srawi", ALU).reads(&[1]).writes(&[10]),
            DagNode::new("lwz_e", LOAD).reads(&[2]).writes(&[11]),
            DagNode::new("add", ALU).reads(&[10, 11]).writes(&[12]),
            DagNode::new("addi", ALU).reads(&[12]).writes(&[13]),
            DagNode::new("stw_eptr", STORE).reads(&[13]).alias(1),
            // s2: *(int*)&x = (hx & M) | C   (through the x slot, after the spill)
            DagNode::new("rlwinm", ALU).reads(&[3]).writes(&[20]),
            DagNode::new("oris", ALU).reads(&[20]).writes(&[21]),
            DagNode::new("stfd_spill", STORE).alias(2),
            DagNode::new("stw_slot", STORE).reads(&[21]).alias(2),
        ];
        assert_eq!(
            labels(&nodes),
            ["rlwinm", "srawi", "lwz_e", "oris", "stfd_spill", "add", "addi", "stw_slot", "stw_eptr"]
        );
    }

    #[test]
    fn the_staging_conflict_serializes_load_store_pairs() {
        // g = *p; h = *q; with both values staged through r0 (extra edge):
        // lwz stw lwz stw
        let nodes = [
            DagNode::new("lwz_p", LOAD).reads(&[1]).writes(&[10]),
            DagNode::new("stw_g", STORE).reads(&[10]),
            DagNode::new("lwz_q", LOAD).reads(&[2]).writes(&[20]).after(1),
            DagNode::new("stw_h", STORE).reads(&[20]),
        ];
        assert_eq!(labels(&nodes), ["lwz_p", "stw_g", "lwz_q", "stw_h"]);
    }
}
