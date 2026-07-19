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

mod legacy;

pub use legacy::assign_registers_legacy;

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
    /// A named LOCAL's float home: window-top tier in the float register
    /// machine (measured: k_sin-class locals z/v take f4/f3 descending while
    /// the chains churn the low registers).
    pub local_home: bool,
    /// The FSUB-rooted accumulator shape (the k_sin else-tail): the reverse
    /// machine processes in EMISSION order — the f0 chain flows forward
    /// through the dying-operand doors (fmul -> fmsub1 -> fmsub2 -> fnmsub)
    /// and the loads ascend past the busy params.
    pub emission_ordered: bool,
    /// This STORE's chain is rooted at the r3 parameter: when its analytic
    /// earliest-ready is <= the return final's, the chain holds r3 (the
    /// allocator reuses the dying param) and the final's r3 write must EMIT
    /// after this store (the WAR constraint, allocation-coupled scheduling).
    pub r3_chain_store: bool,
}

/// The XER (carry) hazard class: srawi, subfc, addc.
pub const HAZARD_XER: u8 = 1;
/// The integer-multiplier structural hazard: Gekko has ONE multiplier, so two
/// mulli never dual-issue (measured: the two-mulli return tail threads the
/// return chain into the serialization gap).
pub const HAZARD_MUL: u8 = 2;
/// The FPU structural hazard: Gekko has ONE float pipe — no two float
/// arithmetic ops issue the same cycle (measured: independent fadd chains
/// keep source order with no hoist).
pub const HAZARD_FPU: u8 = 3;

impl DagNode {
    pub fn new(label: &'static str, latency: u32) -> DagNode {
        let kind = match latency {
            2 => OpKind::Load,
            _ => OpKind::Alu,
        };
        DagNode { label, kind, latency, gate_latency: latency, reads: Vec::new(), writes: Vec::new(), alias_group: None, extra_deps: Vec::new(), hazard: None, forbid_r0: false, extension: false, local_home: false, emission_ordered: false, r3_chain_store: false }
    }
    pub fn local_home(mut self) -> DagNode {
        self.local_home = true;
        self
    }
    pub fn emission_ordered(mut self) -> DagNode {
        self.emission_ordered = true;
        self
    }
    pub fn r3_chain_store(mut self) -> DagNode {
        self.r3_chain_store = true;
        self
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
    /// Whether issue-width slots must use distinct execution ports. Build 163
    /// models one integer ALU, one multiplier, and one shared load/store unit;
    /// the 2.4.x scheduler's measured ordering does not expose this restriction.
    pub port_aware: bool,
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
    // alu before store at weight ties (fire 308: the return-tail fit; the
    // long-latency store AFFINITY tier covers the mulli-store-first captures
    // that store-first ties used to absorb).
    kind_rank: [0, 2, 1],
    weight_before_kind: true,
    strategy: Strategy::GlobalKey,
    port_aware: false,
};

/// Build 163 uses the same dependence/priority model but will not place two
/// operations targeting the same execution port in one issue window.
pub const LEGACY_PORT_AWARE: Model = Model {
    port_aware: true,
    ..FROZEN
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
        let own = if Some(index) == return_sink {
            // The return FINAL contributes nothing: the chain's pull comes
            // from its intermediates (deep_return's store chain leads 2v2 on
            // the source tie; ret_both_deep still leads 3v1).
            0
        } else {
            // Return-chain intermediates weigh full LATENCY like everything
            // else (ret_deep: the mulli-bearing return leads 4v3 — the only
            // capture that discriminates latency from gate weighting there).
            nodes[index].latency
        };
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

    // The WAR constraint: the RETURN FINAL's r3 write emits after the
    // r3-rooted chain's store — but only when that store's ANALYTIC earliest
    // cycle is <= the final's (else the claim overlapped the chain and the
    // allocator adapted its register; measured: the two-mulli tail escapes,
    // the plain and three-store tails bind).
    let earliest: Vec<u32> = {
        let mut earliest = vec![0u32; count];
        for index in 0..count {
            earliest[index] = deps[index]
                .iter()
                .map(|&dependency| earliest[dependency] + nodes[dependency].gate_latency)
                .max()
                .unwrap_or(0);
        }
        earliest
    };
    // A param-direct final (analytic 0) still cannot beat the crowded first
    // cycle, so the binding floor is 1 (nine shapes; the mulli-gated store at
    // 2 escapes while every plain store at 1 binds). The chain must also
    // actually HOLD r3: the LAST store chain's final prefers r0, so its store
    // binds only when an r0 RESERVATION (a non-forbidden return intermediate)
    // pushes the chain to r3 (ret_mix/D unbind — the return emits first).
    let has_r0_reservation = (0..count).any(|node| {
        on_return_chain[node]
            && Some(node) != return_sink
            && nodes[node].kind != OpKind::Store
            && !nodes[node].writes.is_empty()
            && !nodes[node].forbid_r0
    });
    let last_store_index: Option<usize> = (0..count).rev().find(|&node| nodes[node].kind == OpKind::Store);
    let war_stores: Vec<usize> = return_sink
        .map(|sink| {
            (0..count)
                .filter(|&node| {
                    nodes[node].r3_chain_store
                        && earliest[node] <= earliest[sink].max(1)
                        && (deps[node].is_empty() // a LEAF store reads r3 itself: a true RAW hazard
                            || ((!model.port_aware && Some(node) != last_store_index)
                                || has_r0_reservation))
                })
                .collect()
        })
        .unwrap_or_default();

    let mut order = Vec::with_capacity(count);
    let mut issued_at: Vec<Option<u32>> = vec![None; count];
    // PER-EDGE gating (measured: the k_cos else tail): a DOUBLE-FMUL
    // producer (gate 4) consumed in its reader's B slot — the LAST read of
    // a 3-operand fmadd family, the subtrahend/addend — forwards at its
    // plain latency (3); the factor slots keep the full gate.
    let edge_gate = |dependency: usize, consumer: usize| -> u32 {
        let gate = nodes[dependency].gate_latency;
        if gate > nodes[dependency].latency
            && nodes[consumer].reads.len() >= 2
            && nodes[dependency]
                .writes
                .first()
                .is_some_and(|value| nodes[consumer].reads.last() == Some(value))
        {
            return nodes[dependency].latency;
        }
        gate
    };
    let mut time = 0u32;
    let mut robin_flip = false;
    while order.len() < count {
        let mut ready: Vec<usize> = (0..count)
            .filter(|&candidate| issued_at[candidate].is_none())
            .filter(|&candidate| {
                deps[candidate].iter().all(|&dependency| {
                    issued_at[dependency].is_some_and(|at| {
                        if model.gate_on_complete {
                            at + edge_gate(dependency, candidate) <= time
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
        // Ready EXACTLY this cycle: every dependency completed at `time` (or a
        // dependency-free op at cycle 0).
        let fresh = |candidate: usize| -> bool {
            if deps[candidate].is_empty() {
                return time == 0;
            }
            deps[candidate]
                .iter()
                .filter_map(|&dependency| issued_at[dependency].map(|at| at + edge_gate(dependency, candidate)))
                .max()
                .is_some_and(|completed| completed == time)
        };
        // The return-tail tiers (fire 308) are fitted on RETURN-MODE captures
        // only; a void block keeps the store-first tie-break of the original
        // dataset fit.
        let return_mode = return_sink.is_some();
        let execution_port = |node: usize| match nodes[node].kind {
            OpKind::Load | OpKind::Store => 0,
            OpKind::Alu if nodes[node].hazard == Some(HAZARD_MUL) => 1,
            OpKind::Alu => 2,
        };
        let store_count = (0..count)
            .filter(|&node| nodes[node].kind == OpKind::Store)
            .count();
        let long_store_count = (0..count)
            .filter(|&node| {
                nodes[node].kind == OpKind::Store
                    && deps[node]
                        .iter()
                        .any(|&dependency| nodes[dependency].gate_latency >= 2)
            })
            .count();
        let rank = |candidate: usize| -> (u8, u8, u8, u32, u32, usize) {
            let gate = if model.gated_last && gated[candidate] { 1 } else { 0 };
            // A store released by a LONG-latency producer (gate >= 2) issues
            // the moment the gate opens — ahead of everything (measured: the
            // mulli store beats the fresh return op).
            let legacy_long_store_affinity =
                !return_mode || (store_count > 1 && long_store_count == 1);
            let affinity = if (return_mode || model.port_aware)
                && nodes[candidate].kind == OpKind::Store
                && deps[candidate].iter().any(|&dependency| nodes[dependency].gate_latency >= 2)
                && (fresh(candidate) || model.port_aware)
                && (!model.port_aware || legacy_long_store_affinity)
            {
                0u8
            } else {
                1
            };
            // A FRESH non-store outranks weight (measured: the fresh return op
            // beats the heavier aged mulli; at cycle 0 everything is fresh so
            // weight still decides). A port-deferred LOAD keeps the tier while
            // it ages (measured: s1_s2's fourth coefficient load outranks the
            // fresh first fmadd — weight decides between them).
            let legacy_after_store = model.port_aware
                && (0..count)
                    .any(|node| nodes[node].kind == OpKind::Store && issued_at[node].is_some());
            let fresh_alu = if return_mode
                && (!model.port_aware || legacy_after_store)
                && nodes[candidate].kind != OpKind::Store
                && ((model.port_aware && legacy_after_store)
                    || fresh(candidate)
                    || nodes[candidate].kind == OpKind::Load)
            {
                0u8
            } else {
                1
            };

            let kind_index = match nodes[candidate].kind {
                OpKind::Alu => 0,
                OpKind::Load => 1,
                OpKind::Store => 2,
            };
            let kind = if return_mode {
                model.kind_rank[kind_index] as u32
            } else {
                // The original store-first tie-break ([1, 2, 0]).
                [1u8, 2, 0][kind_index] as u32
            };
            let inverse_weight = u32::MAX - weight[candidate];
            if model.weight_before_kind {
                (gate, affinity, fresh_alu, inverse_weight, kind, candidate)
            } else {
                (gate, affinity, fresh_alu, kind, inverse_weight, candidate)
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
                // A LOAD whose every consumer still waits on another UNISSUED
                // operand STALLS (measured: the deep chain's second constant
                // defers past the first fadd — the allocator reuses the dying
                // operand's register, a WAR the scheduler honors on ISSUE).
                // The EMPTY-CYCLE LIFT: when nothing issued this cycle and no
                // unblocked candidate is ready, the top blocked load issues
                // anyway (measured: horner3/4 hoist every coefficient load
                // that has a fresh register — one per empty cycle).
                let load_blocked = |candidate: usize, issued_at: &Vec<Option<u32>>| -> bool {
                    nodes[candidate].kind == OpKind::Load
                        && !(0..count).any(|consumer| {
                            deps[consumer].contains(&candidate)
                                && deps[consumer].iter().all(|&dependency| {
                                    dependency == candidate
                                        || issued_at[dependency].is_some()
                                        // A pending fellow LOAD does not block
                                        // (loads awaiting loads is fine).
                                        || nodes[dependency].kind == OpKind::Load
                                })
                        })
                        && (0..count).any(|consumer| deps[consumer].contains(&candidate))
                };
                // Rescan after every pick: a WAR-deferred return final becomes
                // eligible the moment the bound store lands in this window.
                // LOCAL-HOME deference to pending blocked-load lifts
                // (measured: z4/zv_deeper/ksin_tail; horner4's and
                // fmsub_deep's m1 — no local — issue straight over pending
                // loads): a local_home NODE (v) yields to EVERY pending lift
                // until none remain; an arith merely READING a local (the
                // chain's first fmadd) yields exactly ONE cycle — it defers
                // while FRESH and issues aged (ksin_tail: m1 issues at c5
                // over two still-pending coefficient lifts).
                let arith_defers = |candidate: usize, issued_at: &Vec<Option<u32>>, picked: &[usize]| -> bool {
                    if nodes[candidate].kind == OpKind::Load {
                        return false;
                    }
                    // A lift already PICKED this cycle no longer defers
                    // anyone (measured: ksin_tail's v dual-issues with the
                    // final coefficient lift; z4's m1 follows its lift in the
                    // same cycle). And a lift whose blockage is SELF-CAUSED —
                    // every consumer's unissued non-load blocker is the
                    // candidate or one of its transitive dependents — does
                    // not hold the candidate back (measured: the s_atan w
                    // issues over loads that only wait on w's own chains,
                    // while k_sin's v still defers to lifts blocked by the
                    // independent chain1).
                    let depends_on_candidate = |node: usize, candidate: usize| -> bool {
                        let mut stack = vec![node];
                        let mut seen = vec![false; count];
                        while let Some(current) = stack.pop() {
                            if current == candidate {
                                return true;
                            }
                            if seen[current] {
                                continue;
                            }
                            seen[current] = true;
                            stack.extend(deps[current].iter().copied());
                        }
                        false
                    };
                    let lift_pending = ready.iter().any(|&other| {
                        other != candidate && !picked.contains(&other) && load_blocked(other, issued_at)
                    });
                    if !lift_pending {
                        return false;
                    }
                    // A local depending only on params/LOADS (z = x*x, or
                    // z over the frame reload) leads the schedule and never
                    // defers; a local depending on ARITH (v = z*x) yields —
                    // but ONLY to lifts blocked by something INDEPENDENT of
                    // the local itself (k_sin's v yields to chain1-blocked
                    // coefficient loads; the s_atan w issues straight over
                    // loads that wait only on w's own chains). The one-cycle
                    // fresh-arith deference below keeps self-caused lifts.
                    if nodes[candidate].local_home {
                        let independent_lift = ready.iter().any(|&other| {
                            if other == candidate || picked.contains(&other) || !load_blocked(other, issued_at) {
                                return false;
                            }
                            (0..count).any(|consumer| {
                                deps[consumer].contains(&other)
                                    && deps[consumer].iter().any(|&dependency| {
                                        issued_at[dependency].is_none()
                                            && nodes[dependency].kind != OpKind::Load
                                            && dependency != candidate
                                            && !depends_on_candidate(dependency, candidate)
                                    })
                            })
                        });
                        return independent_lift
                            && deps[candidate].iter().any(|&dep| nodes[dep].kind != OpKind::Load);
                    }
                    deps[candidate].iter().any(|&dep| nodes[dep].local_home) && fresh(candidate)
                };
                'fill: while picked.len() < model.issue_width {
                    for &candidate in &ready {
                        if picked.contains(&candidate) {
                            continue;
                        }
                        if model.port_aware {
                            if picked
                                .iter()
                                .any(|&taken| execution_port(taken) == execution_port(candidate))
                            {
                                continue;
                            }
                            if nodes[candidate].kind == OpKind::Store {
                                let pending_long_store = (0..count).any(|other| {
                                    other != candidate
                                        && issued_at[other].is_none()
                                        && nodes[other].kind == OpKind::Store
                                        && deps[other].iter().any(|&dependency| {
                                            nodes[dependency].gate_latency >= 2
                                                && issued_at[dependency].is_some()
                                        })
                                });
                                if pending_long_store
                                    && !deps[candidate]
                                        .iter()
                                        .any(|&dependency| nodes[dependency].gate_latency >= 2)
                                {
                                    continue;
                                }
                            }
                        }
                        // The LOAD PORT: one load issue per cycle (Gekko has a
                        // single load/store unit; measured: horner coefficient
                        // loads and the s1_s2 front serialize one per cycle).
                        if nodes[candidate].kind == OpKind::Load
                            && picked.iter().any(|&taken| nodes[taken].kind == OpKind::Load)
                        {
                            continue;
                        }
                        if arith_defers(candidate, &issued_at, &picked) {
                            continue;
                        }
                        if load_blocked(candidate, &issued_at) {
                            let other_unblocked = ready.iter().any(|&other| {
                                other != candidate
                                    && !picked.contains(&other)
                                    && !load_blocked(other, &issued_at)
                                    && !arith_defers(other, &issued_at, &picked)
                            });
                            if !picked.is_empty() || other_unblocked {
                                continue;
                            }
                        }
                        // The WAR constraint: the return final waits for (or
                        // follows, within this window) the bound r3-chain stores.
                        if Some(candidate) == return_sink
                            && war_stores
                                .iter()
                                .any(|&store| issued_at[store].is_none() && !picked.contains(&store))
                        {
                            continue;
                        }
                        // The STORE PORT: one store per issue window (measured:
                        // the second fresh store of a pair waits a cycle while
                        // the return op fills the slot). Return mode only — the
                        // void dataset never discriminated it.
                        if return_mode
                            && nodes[candidate].kind == OpKind::Store
                            && picked.iter().any(|&taken| nodes[taken].kind == OpKind::Store)
                        {
                            continue;
                        }
                        if let Some(class) = nodes[candidate].hazard {
                            if picked_hazards.contains(&class) {
                                continue;
                            }
                            picked_hazards.push(class);
                        }
                        picked.push(candidate);
                        continue 'fill;
                    }
                    break;
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
        // Build 163's bundler holds a lone operation for one cycle when a
        // different execution port will become available immediately.
        if model.port_aware && ready.len() == 1 {
            let sole = ready[0];
            let pair_next = (0..count).any(|candidate| {
                candidate != sole
                    && issued_at[candidate].is_none()
                    && execution_port(candidate) != execution_port(sole)
                    && deps[candidate].iter().all(|&dependency| {
                        issued_at[dependency]
                            .is_some_and(|at| at + edge_gate(dependency, candidate) <= time + 1)
                    })
            });
            if pair_next {
                ready.clear();
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
    let param_end = |value: u32| -> usize {
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
        // A FORBIDDEN return-feeder (it cannot stage through r0) will take the
        // r3 handoff — pre-claim it so earlier chains route around (measured:
        // the srawi feeder claims [its slot, the return], pushing the first
        // store final to r6).
        if let Some(return_node) = return_node {
            for node in 0..count {
                if nodes[node].forbid_r0
                    && result[node].is_none()
                    && consumer_of[node].contains(&return_node)
                    && consumer_of[node].len() == 1
                {
                    result[node] = Some(3);
                    occupied.push((3, position[node], value_end(node)));
                }
            }
        }
    }
    // The LAST store chain's sink: only ITS final gets the r0 preference in
    // return mode (measured: the earlier chain's final never touches r0).
    let last_store_sink: Option<usize> = (0..count)
        .filter(|&node| nodes[node].kind == OpKind::Store)
        .max_by_key(|&node| position[node]);
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
        let relaxed = (chain_count <= 2 || (return_mode && start % 2 == 0))
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
        } else if return_mode
            && is_final
            && !nodes[node].forbid_r0
            && consumer_of[node].first().copied() == last_store_sink
        {
            // Only the LAST store chain's final PREFERS r0 in return mode
            // (measured); it falls through when r0 is held — by another
            // occupant or by an overlapping return-intermediate RESERVATION.
            let mut pool = vec![0u8, 3, 4];
            pool.extend(5..=12);
            pool
        } else if return_mode && is_final && !nodes[node].forbid_r0 {
            // An EARLIER chain's final never touches r0 (measured: r6 over a
            // free r0 under the srawi handoff).
            (3..=12).collect()
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


/// The FLOAT register machines (fires 331-335 captures). Both share the
/// interval/window frame: values are non-store defs, intervals are CLOSED
/// [def slot, last consumer slot] over the EMISSION order, params occupy
/// their fixed FPRs over [0, last read], and a consumerless non-store def is
/// the RETURN value, forced to f1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FloatRegModel {
    /// Process defs in REVERSE emission order with ascending first-fit
    /// (else: FORWARD emission order with the MRU free-stack).
    pub reverse: bool,
    /// Reverse machine: a LOAD may take its direct consumer's register
    /// (joining the accumulator), overlap exactly at the consumption slot.
    pub share_loads: bool,
    /// Reverse machine: an ARITH def may take its consumer's register.
    pub share_arith: bool,
    /// Reverse machine: arith shares only into a TWO-operand consumer
    /// (fadd/fmul accumulate in place; an fmadd factor never takes D).
    pub arith_share_two_op_only: bool,
    /// Reverse machine: shares are allowed only into f0 (the accumulator).
    pub share_f0_only: bool,
    /// Reverse machine: a share is REFUSED when the consumer has another
    /// VALUE operand that is a still-unallocated arith (measured: s1_s2's
    /// fifth load takes fresh f4 while mA is pending; horner4/A/B/C shares
    /// all have load or already-allocated-arith siblings).
    pub share_blocked_by_pending_arith: bool,
    /// Reverse machine: process defs by (death DESC, start DESC) instead of
    /// slot-reverse (measured: corrected s1_s2_shallow — mB at slot 3 must
    /// allocate before L15 at slot 4 because it dies later; horner4's
    /// "anomalous" second load falls out naturally under this order).
    pub order_by_death: bool,
    /// A LOWER BOUND on the register window: the dual composition arm
    /// passes the tails' pressure (escaping values + live params + tail
    /// constants), which the shared DAG alone cannot see.
    pub window_floor: u8,
    /// DUAL-TAIL shapes order the prefix tier by DEFINITION descending
    /// (the LAST-defined local takes the top: w > v > z — measured across
    /// the fire-350/354 dual matrix; single-tail shapes keep death-asc).
    /// Set per-call by the dual composition arm.
    pub tier_position_desc: bool,
    /// The COMPOSED-tail regime (the k_cos else: an x re-reload AND a frame
    /// local together): the emission-ordered sequence takes the whole DAG —
    /// the tier-forward machine and the structural crossing tier stand down
    /// (measured against the plain frame-diamond tails, which keep them).
    pub emission_over_tier: bool,
    /// FITTER KNOB: run the death-DESC load pre-pass on every tier-forward
    /// shape (not only duals).
    pub prepass_always: bool,
    /// FITTER KNOB: the pre-pass equal-death tiebreak uses start ASC.
    pub prepass_start_asc: bool,
    /// FITTER KNOB: chain ariths take the LOWEST reusable register instead
    /// of the minimum dying operand.
    pub chain_lowest: bool,
    /// TIER shapes run a FORWARD machine instead of the reverse one:
    /// non-tier values allocate in EMISSION order — loads descending
    /// first-fit from the window top, arith reusing the MINIMUM dying
    /// operand register (measured: the ksin tail's loads descend f6..f2
    /// while horner5/6 — no tier — stay on the reverse machine).
    pub tier_forward_descending: bool,
    /// Reverse machine: named float LOCALS (local_home) and unnamed values
    /// spanning >= 2 other arith definitions take WINDOW-TOP homes,
    /// descending in definition order, BEFORE the death-order pass
    /// (measured: k_sin-class z/v -> f4/f3; the deep mul-of-mul's
    /// cross-chain product -> the window top).
    pub local_top_tier: bool,
    /// Reverse machine: the boundary share into one's own CONSUMER is
    /// allowed regardless of the arity/class rules when the register is
    /// ALSO vacated by the node's own dying operand — the register flows
    /// operand -> node -> consumer as one accumulation chain (measured:
    /// fmsub_wmul's m1 takes f1 through dying-z although the 3-op fmsub
    /// consumer-share alone refuses).
    pub dying_door_share: bool,
    /// Reverse machine: within the equal-death group of the RETURN node's
    /// own operands, LATER emitted slots allocate first — the B/addend-side
    /// operand takes f0 (measured: fmsub_root's subtrahend load, mul_of_mul's
    /// C-side fmul, and s1_s2's mD all claim f0 while the earlier-slot
    /// sibling falls to the next register).
    pub root_slot_order: bool,
    /// The reverse machine applies only to RETURN bodies; void bodies run
    /// the forward stack (measured: deep_vs_shallow's g-chain inherits the
    /// param home through forward in-place reuse).
    pub void_forward: bool,
    /// Forward machine: which dying-operand register the result reuses.
    pub dying_pick: DyingPick,
    /// Forward machine: the initial window stack pushes ascending (top =
    /// window top, first pop descends).
    pub init_ascending: bool,
}

/// Forward machine: a def whose operands die at its slot reuses one of their
/// registers — the candidate rules the fitter enumerates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DyingPick {
    /// The lowest-numbered dying register.
    MinReg,
    /// The highest-numbered dying register.
    MaxReg,
    /// The register of the OLDEST dying operand (earliest def slot; params
    /// count as older than any value).
    OldestDef,
    /// The register of the NEWEST dying operand.
    NewestDef,
}

/// The FROZEN float register model (fires 335-336: 12/12 captures). The
/// fire-336 probes (h4_wmul, s1_s2_shallow, h3_wmul) cracked the two open
/// rules: a share is refused iff the consumer has a still-pending arith
/// sibling operand (share_blocked_by_pending_arith), and defs process by
/// (death DESC, start DESC) — not slot-reverse (order_by_death).
pub const FROZEN_FLOAT_REG: FloatRegModel = FloatRegModel {
    reverse: true,
    share_loads: true,
    share_arith: true,
    arith_share_two_op_only: true,
    share_f0_only: false,
    share_blocked_by_pending_arith: true,
    local_top_tier: true,
    window_floor: 0,
    tier_position_desc: false,
    emission_over_tier: false,
    prepass_always: false,
    prepass_start_asc: false,
    chain_lowest: false,
    tier_forward_descending: true,
    dying_door_share: true,
    order_by_death: true,
    root_slot_order: true,
    dying_pick: DyingPick::MinReg,
    init_ascending: true,
    void_forward: true,
};

pub fn assign_float_registers(
    nodes: &[DagNode],
    order: &[usize],
    params: &[(u32, u8)],
    model: FloatRegModel,
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
    let is_value = |node: usize| nodes[node].kind != OpKind::Store && !nodes[node].writes.is_empty();
    let value_end = |node: usize| -> usize {
        consumer_of[node].iter().map(|&reader| position[reader]).max().unwrap_or(position[node])
    };
    let param_end = |value: u32| -> usize {
        (0..count)
            .filter(|&reader| nodes[reader].reads.contains(&value))
            .map(|reader| position[reader])
            .max()
            .unwrap_or(0)
    };
    let return_node: Option<usize> = (0..count).find(|&node| {
        consumer_of[node].is_empty() && is_value(node)
    });
    let mut result: Vec<Option<u8>> = vec![None; count];
    if let Some(ret) = return_node {
        result[ret] = Some(1);
    }
    // The WINDOW: the smallest W where every boundary's live set fits —
    // counting a live param ONLY while its register sits INSIDE the window
    // (register < W). A low param (x at f1) consumes a slot as before; a
    // HIGH pseudo (the dual's z at f7, r at f4 feeding a tail) does not
    // shrink the scratch region below it (measured: the k_cos else tail's
    // tier lands f3 under scratch peak 4 while z/r live at f7/f4).
    let window = {
        let slots = order.len();
        let boundary_values = |boundary: usize| -> usize {
            (0..count)
                .filter(|&node| is_value(node))
                .filter(|&node| position[node] < boundary && value_end(node) >= boundary)
                .count()
        };
        let mut window = (0..=slots).map(boundary_values).max().unwrap_or(0).max(params.len());
        loop {
            let fits = (0..=slots).all(|boundary| {
                let inside_params = params
                    .iter()
                    .filter(|&&(value, register)| {
                        (register as usize) < window && param_end(value) >= boundary
                    })
                    .count();
                boundary_values(boundary) + inside_params <= window
            });
            if fits {
                break;
            }
            window += 1;
        }
        window
    } as u8;
    let window = window.max(model.window_floor);
    let param_registers: Vec<u8> = params.iter().map(|&(_, register)| register).collect();
    let use_reverse = model.reverse && (return_node.is_some() || !model.void_forward);
    if use_reverse {
        // Occupancies: (register, start, end, owner) — owner usize::MAX marks
        // a param.
        // A param NO node reads is dead on entry (the dual-tail's x after
        // the shared z consumed it) — no occupancy at all.
        let mut occupied: Vec<(u8, usize, usize, usize)> = params
            .iter()
            .filter(|&&(value, _)| (0..count).any(|reader| nodes[reader].reads.contains(&value)))
            .map(|&(value, register)| (register, 0, param_end(value), usize::MAX))
            .collect();
        if let Some(ret) = return_node {
            occupied.push((1, position[ret], value_end(ret), ret));
        }
        let mut sequence: Vec<usize> = (0..count).filter(|&node| is_value(node)).collect();
        let emission_regime = (0..count).any(|node| nodes[node].emission_ordered);
        if emission_regime {
            // The FSUB-rooted accumulator shape: EMISSION order — the f0
            // chain flows forward through the dying-operand doors.
            sequence.sort_by_key(|&node| position[node]);
        } else if model.order_by_death {
            let return_reads: Vec<u32> = return_node.map(|ret| nodes[ret].reads.clone()).unwrap_or_default();
            sequence.sort_by_key(|&node| {
                let root_slot = if model.root_slot_order {
                    nodes[node]
                        .writes
                        .first()
                        .and_then(|value| return_reads.iter().position(|read| read == value))
                        .map(|slot| slot + 1)
                        .unwrap_or(0)
                } else {
                    0
                };
                std::cmp::Reverse((value_end(node), root_slot, position[node]))
            });
        } else {
            sequence.sort_by_key(|&node| std::cmp::Reverse(position[node]));
        }
        // The WINDOW-TOP tier: named locals and cross-chain values (spanning
        // >= 2 other arith defs) take descending homes from the window top,
        // in definition order, before everything else.
        if model.local_top_tier {
            let consumer_count = |node: usize| -> usize {
                nodes[node]
                    .writes
                    .first()
                    .map(|value| {
                        (0..count)
                            .filter(|&reader| nodes[reader].reads.contains(value))
                            .count()
                    })
                    .unwrap_or(0)
            };
            let mut tier: Vec<usize> = (0..count)
                .filter(|&node| is_value(node) && result[node].is_none())
                .filter(|&node| {
                    if nodes[node].local_home {
                        return true;
                    }
                    if nodes[node].kind == OpKind::Load {
                        // A MULTI-CONSUMER load joins the tier (the frame
                        // reload of x, the duplicated pool literal); the
                        // single-consumer coefficients stay in the load pass
                        // (horner5's canary pinned that exclusion).
                        return consumer_count(node) >= 2;
                    }
                    // The COMPOSED emission regime keeps cascades on the
                    // door chain — the structural crossing rule would hoist
                    // the k_cos else's hz over its 0.5 door (measured: hz f1).
                    if emission_regime && model.emission_over_tier {
                        return false;
                    }
                    let start = position[node];
                    let end = value_end(node);
                    let inside_arith = (0..count)
                        .filter(|&other| other != node && is_value(other) && nodes[other].kind != OpKind::Load)
                        .filter(|&other| position[other] > start && position[other] < end)
                        .count();
                    inside_arith >= 2
                })
                .collect();
            // PREFIX membership: a local scheduled past the first chain
            // arith (the dual v deferred behind the coefficient lifts) is
            // NOT tier — it flows serially (measured: ksin_dual's v = f5,
            // L45's dead register).
            if model.tier_position_desc {
                let first_chain_arith = (0..count)
                    .filter(|&other| {
                        is_value(other) && nodes[other].kind != OpKind::Load && !nodes[other].local_home
                    })
                    .map(|other| position[other])
                    .min();
                if let Some(first_arith) = first_chain_arith {
                    // ADJACENCY (fire 364): only a local landing IMMEDIATELY
                    // after the first chain arith is squeezed out of the tier
                    // (d4's v, slot chain1+1); a load separating them keeps
                    // the local in the tier (d5's v at chain1+2).
                    tier.retain(|&node| !nodes[node].local_home || position[node] != first_arith + 1);
                }
            }
            if model.tier_position_desc {
                // Duals: definition DESC — the last-defined local tops.
                tier.sort_by_key(|&node| std::cmp::Reverse(position[node]));
            } else {
                // Death ASCENDING (position tiebreak): the reload slots BETWEEN
                // the locals (z f7, XR f6, v f5 in the real k_sin).
                // Equal deaths break by CONSUMER COUNT DESC, then position
                // (measured: reload_zv's root tie picks the 3-consumer reload
                // over the 1-consumer v; the composed 5-const chain's root tie
                // picks the 5-consumer z over the 2-consumer reload).
                tier.sort_by_key(|&node| {
                    // ... and equal counts put the LOCAL above the load
                    // (measured: the 2-const return-x tie wants z over the
                    // reload at 2 consumers each).
                    (
                        value_end(node),
                        std::cmp::Reverse(consumer_count(node)),
                        nodes[node].kind == OpKind::Load,
                        position[node],
                    )
                });
            }
            let tier_members: Vec<usize> = tier.clone();
            let mut next_top = window.saturating_sub(1);
            for node in tier {
                let start = position[node];
                let end = value_end(node);
                // A param register is takeable when the param DIES at this
                // node's definition feeding it (z = x*y claims y's f2).
                let register = (0..=next_top)
                    .rev()
                    .find(|&register| {
                        occupied.iter().all(|&(taken, taken_start, taken_end, owner)| {
                            if taken != register || taken_end < start || taken_start > end {
                                return true;
                            }
                            owner == usize::MAX
                                && taken_end == start
                                && params.iter().any(|&(value, param_register)| {
                                    param_register == register && nodes[node].reads.contains(&value)
                                })
                        })
                    })
                    .unwrap_or(next_top);
                result[node] = Some(register);
                occupied.push((register, start, end, node));
                next_top = register.saturating_sub(1);
            }
            // TIER shapes switch to the FORWARD machine: emission order,
            // loads (and any dep-free values) DESCENDING first-fit from the
            // window top, arith reusing the MINIMUM dying operand register
            // (measured: the ksin tail's coefficients descend f6..f2 with
            // the chain riding the dying addends; horner5/6 — no tier —
            // stay on the reverse machine below).
            if model.tier_forward_descending
                && !tier_members.is_empty()
                && !(emission_regime && model.emission_over_tier)
            {
                // LOADS allocate by (death DESC, start DESC) with ASCENDING
                // first-fit — the reverse machine's core rule. Identical to
                // descending-from-the-top on non-interleaved shapes; on the
                // d5 interleave the post-chain C1 load claims f0 for [8,13]
                // early, which blocks the first chain arith's MIN reuse
                // (measured: chain1 lands on its factor's f5).
                let mut pending_loads: Vec<usize> = (0..count)
                    .filter(|_| model.tier_position_desc || model.prepass_always)
                    .filter(|&node| {
                        is_value(node) && result[node].is_none() && nodes[node].kind == OpKind::Load
                    })
                    .filter(|&node| {
                        !nodes[node].writes.first().is_some_and(|value| {
                            tier_members.iter().any(|&member| nodes[member].reads.contains(value))
                        })
                    })
                    .collect();
                if model.prepass_start_asc {
                    pending_loads.sort_by_key(|&node| (std::cmp::Reverse(value_end(node)), position[node]));
                } else {
                    pending_loads.sort_by_key(|&node| std::cmp::Reverse((value_end(node), position[node])));
                }
                for node in pending_loads {
                    let start = position[node];
                    let end = value_end(node);
                    let register = match (0..window).find(|&register| {
                        occupied.iter().all(|&(taken, taken_start, taken_end, _)| {
                            taken != register || taken_end < start || taken_start > end
                        })
                    }) {
                        Some(register) => register,
                        None => return vec![None; count],
                    };
                    result[node] = Some(register);
                    occupied.push((register, start, end, node));
                }
                for &node in order {
                    if !is_value(node) || result[node].is_some() {
                        continue;
                    }
                    let start = position[node];
                    let end = value_end(node);
                    let free = |register: u8, occupied: &Vec<(u8, usize, usize, usize)>| -> bool {
                        occupied.iter().all(|&(taken, taken_start, taken_end, _)| {
                            taken != register || taken_end < start || taken_start > end
                        })
                    };
                    // The MIN register among operands dying exactly at this
                    // definition (in-place reuse).
                    // A dying register is reusable only when nothing ELSE
                    // holds it across this value's span (the d5 interleave's
                    // early-claimed f0 blocks chain1's MIN — measured).
                    let reusable = |register: u8, occupied: &Vec<(u8, usize, usize, usize)>| -> bool {
                        occupied.iter().all(|&(taken, taken_start, taken_end, owner)| {
                            if taken != register || taken_end <= start || taken_start > end {
                                return true;
                            }
                            // The CONSUMER-BOUNDARY door: dying exactly into
                            // the occupant that consumes this value passes
                            // (measured: the shallow table's even chain joins
                            // the return's f1 at its death).
                            taken_start == end
                                && owner != usize::MAX
                                && nodes[node]
                                    .writes
                                    .first()
                                    .is_some_and(|value| nodes[owner].reads.contains(value))
                        })
                    };
                    let min_dying: Option<u8> = nodes[node]
                        .reads
                        .iter()
                        .filter_map(|read| {
                            if let Some(&(value, register)) = params.iter().find(|&&(value, _)| value == *read) {
                                return (param_end(value) == start).then_some(register);
                            }
                            (0..count)
                                .rev()
                                .find(|&writer| nodes[writer].writes.contains(read))
                                .and_then(|writer| {
                                    (value_end(writer) == start).then(|| result[writer]).flatten()
                                })
                        })
                        .filter(|&register| reusable(register, &occupied))
                        .min();
                    // A load consumed by a TIER member (the single-consumer
                    // frame reload feeding z) allocates ASCENDING; the
                    // coefficient loads DESCEND from the window top.
                    let feeds_tier = nodes[node].kind == OpKind::Load
                        && nodes[node].writes.first().is_some_and(|value| {
                            tier_members.iter().any(|&member| nodes[member].reads.contains(value))
                        });
                    // An ESCAPING chain root (its value read by a trailing
                    // STORE sink — it lives across the dual's branch) takes
                    // its C-operand's dying register over the minimum
                    // (measured: ksin_dual's r = m2's f3, not L15's f0).
                    let escapes = nodes[node].writes.first().is_some_and(|value| {
                        (0..count).any(|reader| {
                            nodes[reader].kind == OpKind::Store && nodes[reader].reads.contains(value)
                        })
                    });
                    let c_dying: Option<u8> = if escapes && nodes[node].reads.len() >= 2 {
                        let c_value = nodes[node].reads[1];
                        (0..count)
                            .rev()
                            .find(|&writer| nodes[writer].writes.contains(&c_value))
                            .and_then(|writer| {
                                (value_end(writer) == position[node]).then(|| result[writer]).flatten()
                            })
                            .filter(|&register| reusable(register, &occupied))
                    } else {
                        None
                    };
                    let chain_pick = if model.chain_lowest {
                        (0..window).find(|&register| reusable(register, &occupied))
                    } else {
                        min_dying
                    };
                    let register = if nodes[node].kind != OpKind::Load {
                        match c_dying.or(chain_pick) {
                            Some(register) => register,
                            None => match (0..window).rev().find(|&register| free(register, &occupied)) {
                                Some(register) => register,
                                None => return vec![None; count],
                            },
                        }
                    } else if feeds_tier {
                        match (0..window).find(|&register| free(register, &occupied)) {
                            Some(register) => register,
                            None => return vec![None; count],
                        }
                    } else {
                        match (0..window).rev().find(|&register| free(register, &occupied)) {
                            Some(register) => register,
                            None => return vec![None; count],
                        }
                    };
                    result[node] = Some(register);
                    occupied.push((register, start, end, node));
                }
                return result;
            }
        }
        // Whether one of `node`'s own operands (param or value) holds
        // `register` and dies exactly at `node`'s definition — the register
        // is being vacated TO this node (the dying door).
        let dying_vacates = |node: usize, register: u8, result: &Vec<Option<u8>>| -> bool {
            nodes[node].reads.iter().any(|read| {
                if let Some(&(value, param_register)) = params.iter().find(|&&(value, _)| value == *read) {
                    return param_register == register && param_end(value) == position[node];
                }
                (0..count)
                    .rev()
                    .find(|&writer| nodes[writer].writes.contains(read))
                    .is_some_and(|writer| result[writer] == Some(register) && value_end(writer) == position[node])
            })
        };
        // Within an equal-death group, a member holding an IMMEDIATELY
        // allowed consumer-share (target allocated, class permitted, no
        // pending-arith sibling) allocates FIRST and claims it (measured:
        // the k_sin else-tail's v*r fmul takes its fmsub's f0 ahead of the
        // 0.5 load; groups whose shares are pending-blocked keep start-desc).
        let allowed_share_now = |node: usize, result: &Vec<Option<u8>>| -> bool {
            let Some(&value) = nodes[node].writes.first() else {
                return false;
            };
            (0..count).any(|owner| {
                if !nodes[owner].reads.contains(&value) || result[owner].is_none() {
                    return false;
                }
                if position[owner] != value_end(node) {
                    return false;
                }
                let class_ok = if nodes[node].kind == OpKind::Load {
                    model.share_loads
                } else {
                    let b_slot = nodes[owner].reads.last() == Some(&value);
                    model.share_arith
                        && (!model.arith_share_two_op_only || nodes[owner].reads.len() <= 2 || b_slot)
                };
                if !class_ok {
                    return false;
                }
                if model.share_blocked_by_pending_arith {
                    let blocked = nodes[owner].reads.iter().any(|read| {
                        (0..count)
                            .rev()
                            .find(|&writer| nodes[writer].writes.contains(read))
                            .is_some_and(|writer| {
                                writer != node && nodes[writer].kind != OpKind::Load && result[writer].is_none()
                            })
                    });
                    if blocked {
                        return false;
                    }
                }
                true
            })
        };
        let mut sequence = sequence;
        let mut cursor = 0usize;
        while cursor < sequence.len() {
            // Promote an allowed-share member to the front of ITS equal-death
            // group (the group = the run of equal value_end from cursor).
            let group_end = {
                let head_end = value_end(sequence[cursor]);
                let mut end = cursor;
                while end < sequence.len() && value_end(sequence[end]) == head_end {
                    end += 1;
                }
                end
            };
            if group_end - cursor > 1 {
                if let Some(promoted) = (cursor..group_end)
                    .find(|&index| allowed_share_now(sequence[index], &result))
                {
                    sequence[cursor..=promoted].rotate_right(1);
                }
            }
            let node = sequence[cursor];
            cursor += 1;
            if result[node].is_some() {
                continue;
            }
            let start = position[node];
            let end = value_end(node);
            let share_ok = |owner: usize, result: &Vec<Option<u8>>| -> bool {
                let class_ok = if nodes[node].kind == OpKind::Load {
                    model.share_loads
                } else {
                    // Arith shares into 2-op consumers, and into a 3-op
                    // consumer's B slot (the LAST read — the base/addend the
                    // D naturally accumulates over; measured: the k_sin
                    // else-tail's fmsub2 joins its fnmsub at f0 while every
                    // C-factor join stays refused).
                    let b_slot = nodes[node]
                        .writes
                        .first()
                        .is_some_and(|value| nodes[owner].reads.last() == Some(value));
                    model.share_arith
                        && (!model.arith_share_two_op_only || nodes[owner].reads.len() <= 2 || b_slot)
                };
                if !class_ok {
                    return false;
                }
                if model.share_blocked_by_pending_arith {
                    // The consumer's OTHER value operands: a pending (still
                    // unallocated) arith sibling blocks the join.
                    let blocked = nodes[owner].reads.iter().any(|read| {
                        (0..count).rev().find(|&writer| nodes[writer].writes.contains(read)).is_some_and(
                            |writer| {
                                writer != node
                                    && nodes[writer].kind != OpKind::Load
                                    && result[writer].is_none()
                            },
                        )
                    });
                    if blocked {
                        return false;
                    }
                }
                true
            };
            let pick = (0u8..14).find(|&register| {
                occupied.iter().all(|&(taken, taken_start, taken_end, owner)| {
                    if taken != register || taken_end < start || taken_start > end {
                        return true;
                    }
                    // Operand -> consumer share: the overlap is exactly the
                    // consumption slot and the occupant consumes this node.
                    // The DYING DOOR: when this register is also vacated by
                    // the node's own dying operand, the consumer boundary
                    // passes regardless of the arity/class rules (the
                    // accumulation chain operand -> node -> consumer).
                    if owner != usize::MAX
                        && consumer_of[node].contains(&owner)
                        && taken_start == end
                        && (share_ok(owner, &result)
                            || (model.dying_door_share && dying_vacates(node, register, &result)))
                        && (!model.share_f0_only || register == 0)
                    {
                        return true;
                    }
                    // Def over its own dying operand (a param or earlier
                    // value read by this node, ending exactly at our def).
                    let dying_operand = if owner == usize::MAX {
                        params.iter().any(|&(value, taken_register)| {
                            taken_register == taken && nodes[node].reads.contains(&value)
                        })
                    } else {
                        nodes[node].reads.iter().any(|read| nodes[owner].writes.contains(read))
                    };
                    dying_operand && taken_end == start
                })
            });
            let register = pick.unwrap_or(13);
            result[node] = Some(register);
            occupied.push((register, start, end, node));
        }
    } else {
        // FORWARD: the MRU free-stack. Available = the window minus params.
        // A def whose operands die at its slot reuses one of their registers
        // (per dying_pick), releasing the others; otherwise it pops the stack.
        let mut stack: Vec<u8> = Vec::new();
        let available: Vec<u8> = (0..window).filter(|register| !param_registers.contains(register)).collect();
        if model.init_ascending {
            stack.extend(available.iter());
        } else {
            stack.extend(available.iter().rev());
        }
        let return_slot = return_node.map(|ret| position[ret]);
        // Dying operands of `node`: (register, def slot; params def at -1).
        let dying_of = |node: usize, result: &Vec<Option<u8>>| -> Vec<(u8, isize)> {
            nodes[node]
                .reads
                .iter()
                .filter_map(|read| {
                    let last_read = (0..count)
                        .filter(|&reader| nodes[reader].reads.contains(read))
                        .map(|reader| position[reader])
                        .max()
                        .unwrap_or(0);
                    if last_read != position[node] {
                        return None;
                    }
                    params
                        .iter()
                        .find(|&&(value, _)| value == *read)
                        .map(|&(_, register)| (register, -1isize))
                        .or_else(|| {
                            (0..count)
                                .rev()
                                .find(|&writer| nodes[writer].writes.contains(read))
                                .and_then(|writer| {
                                    result[writer].map(|register| (register, position[writer] as isize))
                                })
                        })
                })
                .collect()
        };
        for &node in order {
            let dying = dying_of(node, &result);
            if !is_value(node) || Some(position[node]) == return_slot {
                // Stores and the forced-f1 return release their dying
                // operand registers to the stack.
                for &(register, _) in &dying {
                    if !stack.contains(&register) {
                        stack.push(register);
                    }
                }
                if is_value(node) {
                    result[node] = Some(1);
                }
                continue;
            }
            let reused: Option<u8> = match model.dying_pick {
                DyingPick::MinReg => dying.iter().map(|&(register, _)| register).min(),
                DyingPick::MaxReg => dying.iter().map(|&(register, _)| register).max(),
                DyingPick::OldestDef => dying.iter().min_by_key(|&&(_, slot)| slot).map(|&(register, _)| register),
                DyingPick::NewestDef => dying.iter().max_by_key(|&&(_, slot)| slot).map(|&(register, _)| register),
            };
            let register = match reused {
                Some(register) => {
                    for &(other, _) in &dying {
                        if other != register && !stack.contains(&other) {
                            stack.push(other);
                        }
                    }
                    register
                }
                None => stack.pop().unwrap_or(13),
            };
            result[node] = Some(register);
        }
    }
    result
}

#[cfg(test)]
mod tests;
