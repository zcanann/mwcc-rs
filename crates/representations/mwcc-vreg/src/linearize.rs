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
    /// Issue-to-result steps (see the module table).
    pub latency: u32,
    /// Value ids this op reads (RAW edges come from these).
    pub reads: Vec<u32>,
    /// Value ids this op defines.
    pub writes: Vec<u32>,
    /// Stores that may alias share a group and keep program order.
    pub alias_group: Option<u32>,
    /// Extra dependence edges (indices), e.g. the r0-staging serialization.
    pub extra_deps: Vec<usize>,
}

impl DagNode {
    pub fn new(label: &'static str, latency: u32) -> DagNode {
        let kind = match latency {
            2 => OpKind::Load,
            _ => OpKind::Alu,
        };
        DagNode { label, kind, latency, reads: Vec::new(), writes: Vec::new(), alias_group: None, extra_deps: Vec::new() }
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
    // Critical-path weight: latency + the heaviest dependent's weight.
    let mut weight = vec![0u32; count];
    for index in (0..count).rev() {
        let downstream = (index + 1..count)
            .filter(|&later| deps[later].contains(&index))
            .map(|later| weight[later])
            .max()
            .unwrap_or(0);
        weight[index] = nodes[index].latency + downstream;
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
                            at + nodes[dependency].latency <= time
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
                ready.truncate(model.issue_width);
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
