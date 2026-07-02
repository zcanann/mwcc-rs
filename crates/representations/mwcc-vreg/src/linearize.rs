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

/// One abstract operation in a block's dependence DAG.
#[derive(Debug, Clone)]
pub struct DagNode {
    /// For test assertions and diagnostics.
    pub label: &'static str,
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
        DagNode { label, latency, reads: Vec::new(), writes: Vec::new(), alias_group: None, extra_deps: Vec::new() }
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

/// Linearize the DAG: the returned indices are the emission order.
pub fn linearize(nodes: &[DagNode]) -> Vec<usize> {
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

    let mut order = Vec::with_capacity(count);
    let mut issued_at: Vec<Option<u32>> = vec![None; count];
    let mut time = 0u32;
    while order.len() < count {
        // Ops whose deps have all ISSUED and whose operands are COMPLETE.
        let mut ready: Vec<usize> = (0..count)
            .filter(|&candidate| issued_at[candidate].is_none())
            .filter(|&candidate| {
                deps[candidate].iter().all(|&dependency| {
                    issued_at[dependency].is_some_and(|at| at + nodes[dependency].latency <= time)
                })
            })
            .collect();
        if ready.is_empty() {
            time += 1;
            continue;
        }
        // Highest critical-path weight first; source order breaks ties.
        ready.sort_by_key(|&candidate| (std::cmp::Reverse(weight[candidate]), candidate));
        for &pick in ready.iter().take(2) {
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
