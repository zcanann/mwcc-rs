//! Scheduler + register-allocation fixtures for the linearize module.
//! Split from the former inline test module (fire 538).

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
    /// FLOAT ORDER fixtures (fire 331-332 captures): float arithmetic is
    /// latency-3 single-pipe (HAZARD_FPU); loads are the int Load class.
    /// Expected EMISSION order as node indices.
    fn float_order_fixtures() -> Vec<(&'static str, Vec<DagNode>, Vec<usize>, bool)> {
        use OpKind::Store as St;
        const FARITH: u32 = 3;
        // DOUBLE fmul GATES its consumers at 4 cycles while WEIGHING 3 for
        // priority (the mulli precedent: gate and weight split). Measured:
        // z=x*x chains hold the first fmadd one extra cycle so every
        // coefficient lifts first, while z=x+x (a 3-cycle fadd) flows the
        // standard 3-loads-then-fmadd; s1_s2's load order pins the WEIGHT
        // at 3 (weight 4 flips the s1/s2 chains). The fire-341 "local-home
        // stall" was this gate in disguise.
        const FMUL_D: u32 = 4;
        vec![
            (
                // g=a+1.5; h=b+2.5: lfd lfd fadd fadd stfd stfd (source order)
                "float_two_fadd",
                vec![
                    DagNode::new("lfd_c1", LOAD).writes(&[10]),
                    DagNode::new("fadd_g", FARITH).hazard(HAZARD_FPU).reads(&[1, 10]).writes(&[11]),
                    DagNode::new("st_g", STORE).kind(St).reads(&[11]),
                    DagNode::new("lfd_c2", LOAD).writes(&[20]),
                    DagNode::new("fadd_h", FARITH).hazard(HAZARD_FPU).reads(&[2, 20]).writes(&[21]),
                    DagNode::new("st_h", STORE).kind(St).reads(&[21]),
                ],
                vec![0, 3, 1, 4, 2, 5],
                true,
            ),
            (
                // g=(a+1.5)+2.5; h=b+3.5: lfd1.5 lfd3.5 faddg1 lfd2.5 faddh faddg2 sth stg
                "float_deep_vs_shallow",
                vec![
                    DagNode::new("lfd_c1", LOAD).writes(&[10]),
                    DagNode::new("fadd_g1", FARITH).hazard(HAZARD_FPU).reads(&[1, 10]).writes(&[11]),
                    DagNode::new("lfd_c2", LOAD).writes(&[12]),
                    DagNode::new("fadd_g2", FARITH).hazard(HAZARD_FPU).reads(&[11, 12]).writes(&[13]),
                    DagNode::new("st_g", STORE).kind(St).reads(&[13]),
                    DagNode::new("lfd_c3", LOAD).writes(&[20]),
                    DagNode::new("fadd_h", FARITH).hazard(HAZARD_FPU).reads(&[2, 20]).writes(&[21]),
                    DagNode::new("st_h", STORE).kind(St).reads(&[21]),
                ],
                vec![0, 5, 1, 2, 6, 3, 7, 4],
                true,
            ),
            (
                // g=a*b+1.5 (fmadd); h=c+2.5: lfd lfd fmadd fadd stg sth
                "float_fmadd_latency",
                vec![
                    DagNode::new("lfd_c1", LOAD).writes(&[10]),
                    DagNode::new("fmadd_g", FARITH).hazard(HAZARD_FPU).reads(&[1, 2, 10]).writes(&[11]),
                    DagNode::new("st_g", STORE).kind(St).reads(&[11]),
                    DagNode::new("lfd_c2", LOAD).writes(&[20]),
                    DagNode::new("fadd_h", FARITH).hazard(HAZARD_FPU).reads(&[3, 20]).writes(&[21]),
                    DagNode::new("st_h", STORE).kind(St).reads(&[21]),
                ],
                vec![0, 3, 1, 4, 2, 5],
                true,
            ),
            (
                // return z*(1.5+z*2.5): lfd2.5 lfd1.5 fmadd fmul
                "float_horner2",
                vec![
                    DagNode::new("lfd_c25", LOAD).writes(&[10]),
                    DagNode::new("lfd_c15", LOAD).writes(&[11]),
                    DagNode::new("fmadd1", FARITH).hazard(HAZARD_FPU).reads(&[1, 10, 11]).writes(&[12]),
                    DagNode::new("fmul", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[1, 12]).writes(&[13]),
                ],
                vec![0, 1, 2, 3],
                true,
            ),
            (
                // return z*(1.5+z*(2.5+z*3.5)): ALL loads first inner->outer,
                // then the serial fmadd chain (the third load LIFTS on the
                // empty cycle — no unblocked work exists).
                "float_horner3",
                vec![
                    DagNode::new("lfd_c35", LOAD).writes(&[10]),
                    DagNode::new("lfd_c25", LOAD).writes(&[11]),
                    DagNode::new("lfd_c15", LOAD).writes(&[12]),
                    DagNode::new("fmadd1", FARITH).hazard(HAZARD_FPU).reads(&[1, 10, 11]).writes(&[13]),
                    DagNode::new("fmadd2", FARITH).hazard(HAZARD_FPU).reads(&[1, 13, 12]).writes(&[14]),
                    DagNode::new("fmul", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[1, 14]).writes(&[15]),
                ],
                vec![0, 1, 2, 3, 4, 5],
                true,
            ),
            (
                // horner4: the FOURTH coefficient load defers past the first
                // fmadd (blocked by fmadd2 pending; fmadd1 ranks above it at
                // its ready cycle), then lifts on the next empty cycle.
                "float_horner4",
                vec![
                    DagNode::new("lfd_c45", LOAD).writes(&[10]),
                    DagNode::new("lfd_c35", LOAD).writes(&[11]),
                    DagNode::new("lfd_c25", LOAD).writes(&[12]),
                    DagNode::new("lfd_c15", LOAD).writes(&[13]),
                    DagNode::new("fmadd1", FARITH).hazard(HAZARD_FPU).reads(&[1, 10, 11]).writes(&[14]),
                    DagNode::new("fmadd2", FARITH).hazard(HAZARD_FPU).reads(&[1, 14, 12]).writes(&[15]),
                    DagNode::new("fmadd3", FARITH).hazard(HAZARD_FPU).reads(&[1, 15, 13]).writes(&[16]),
                    DagNode::new("fmul", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[1, 16]).writes(&[17]),
                ],
                vec![0, 1, 2, 4, 3, 5, 6, 7],
                true,
            ),
            (
                // T[]-coefficient horner3: lis/addi materialize the base, the
                // three lfd read it (descending offsets), then the chain.
                "float_horner_array",
                vec![
                    DagNode::new("lis", ALU).writes(&[30]),
                    DagNode::new("addi", ALU).reads(&[30]).writes(&[31]),
                    DagNode::new("lfd_t2", LOAD).reads(&[31]).writes(&[10]),
                    DagNode::new("lfd_t1", LOAD).reads(&[31]).writes(&[11]),
                    DagNode::new("lfd_t0", LOAD).reads(&[31]).writes(&[12]),
                    DagNode::new("fmadd1", FARITH).hazard(HAZARD_FPU).reads(&[1, 10, 11]).writes(&[13]),
                    DagNode::new("fmadd2", FARITH).hazard(HAZARD_FPU).reads(&[1, 13, 12]).writes(&[14]),
                    DagNode::new("fmul", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[1, 14]).writes(&[15]),
                ],
                vec![0, 1, 2, 3, 4, 5, 6, 7],
                true,
            ),
            (
                // z*(1.5+w*(2.5+w*3.5)) + w*(4.5+w*5.5): four coefficient
                // loads (one per cycle), fmadd_A slots after the load port,
                // the 1.5 load unblocks the cycle after A issues.
                "float_s1_s2",
                vec![
                    DagNode::new("lfd_c35", LOAD).writes(&[10]),
                    DagNode::new("lfd_c25", LOAD).writes(&[11]),
                    DagNode::new("lfd_c55", LOAD).writes(&[12]),
                    DagNode::new("lfd_c45", LOAD).writes(&[13]),
                    DagNode::new("lfd_c15", LOAD).writes(&[14]),
                    DagNode::new("fmadd_a", FARITH).hazard(HAZARD_FPU).reads(&[2, 10, 11]).writes(&[20]),
                    DagNode::new("fmadd_b", FARITH).hazard(HAZARD_FPU).reads(&[2, 12, 13]).writes(&[21]),
                    DagNode::new("fmadd_c", FARITH).hazard(HAZARD_FPU).reads(&[2, 20, 14]).writes(&[22]),
                    DagNode::new("fmul_d", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[2, 21]).writes(&[23]),
                    DagNode::new("fmadd_f", FARITH).hazard(HAZARD_FPU).reads(&[1, 22, 23]).writes(&[24]),
                ],
                vec![0, 1, 2, 3, 5, 4, 6, 7, 8, 9],
                true,
            ),
            (
                // g=a*1.5; h=b+2.5: the fmul/fadd void pair keeps source
                // order (one load per cycle serializes the front).
                "float_fmul_vs_fadd",
                vec![
                    DagNode::new("lfd_c1", LOAD).writes(&[10]),
                    DagNode::new("fmul_g", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[1, 10]).writes(&[11]),
                    DagNode::new("st_g", STORE).kind(St).reads(&[11]),
                    DagNode::new("lfd_c2", LOAD).writes(&[20]),
                    DagNode::new("fadd_h", FARITH).hazard(HAZARD_FPU).reads(&[2, 20]).writes(&[21]),
                    DagNode::new("st_h", STORE).kind(St).reads(&[21]),
                ],
                vec![0, 3, 1, 4, 2, 5],
                true,
            ),
            (
                // return *p + 1.5: the POOL constant loads before the pointer
                // load (open tie-break — construction order says otherwise);
                // not fit yet, kept as a measured miss.
                "float_load_use",
                vec![
                    DagNode::new("lfd_p", LOAD).reads(&[1]).writes(&[10]),
                    DagNode::new("lfd_c15", LOAD).writes(&[11]),
                    DagNode::new("fadd", FARITH).hazard(HAZARD_FPU).reads(&[10, 11]).writes(&[12]),
                ],
                vec![1, 0, 2],
                false,
            ),
        ]
    }

    /// Diagnostic: score FROZEN against the float order captures.
    #[test]
    fn float_orders() {
        let shapes = float_order_fixtures();
        let mut passed = 0;
        for (name, nodes, expected, must_pass) in &shapes {
            let got = linearize(nodes);
            let ok = got == *expected;
            if ok {
                passed += 1;
            } else {
                let labels: Vec<&str> = got.iter().map(|&index| nodes[index].label).collect();
                let want: Vec<&str> = expected.iter().map(|&index| nodes[index].label).collect();
                println!("float MISS {name}: got {labels:?} want {want:?}");
            }
            if *must_pass {
                assert!(ok, "float fixture {name} regressed");
            }
        }
        println!("float orders: {passed}/{}", shapes.len());
    }

    /// FLOAT REGISTER fixtures (fires 331-335 captures): per-NODE expected
    /// FPR. Params are (value, register). Node construction mirrors
    /// float_order_fixtures so linearize reproduces the real emission.
    fn float_register_fixtures() -> Vec<(&'static str, Vec<DagNode>, Vec<(u32, u8)>, Vec<Option<u8>>)> {
        use OpKind::Store as St;
        const FARITH: u32 = 3;
        const FMUL_D: u32 = 4;
        vec![
            (
                // g=a+1.5; h=b+2.5: lfd f3 / fadd f1,f3,f1 / lfd f0 / fadd f0,f0,f2
                "reg_two_fadd",
                vec![
                    DagNode::new("lfd_c1", LOAD).writes(&[10]),
                    DagNode::new("fadd_g", FARITH).hazard(HAZARD_FPU).reads(&[1, 10]).writes(&[11]),
                    DagNode::new("st_g", STORE).kind(St).reads(&[11]),
                    DagNode::new("lfd_c2", LOAD).writes(&[20]),
                    DagNode::new("fadd_h", FARITH).hazard(HAZARD_FPU).reads(&[2, 20]).writes(&[21]),
                    DagNode::new("st_h", STORE).kind(St).reads(&[21]),
                ],
                vec![(1, 1), (2, 2)],
                vec![Some(3), Some(1), None, Some(0), Some(0), None],
            ),
            (
                // g=(a+1.5)+2.5; h=b+3.5: f3/f1 chain reusing f3; h in f0.
                "reg_deep_vs_shallow",
                vec![
                    DagNode::new("lfd_c1", LOAD).writes(&[10]),
                    DagNode::new("fadd_g1", FARITH).hazard(HAZARD_FPU).reads(&[1, 10]).writes(&[11]),
                    DagNode::new("lfd_c2", LOAD).writes(&[12]),
                    DagNode::new("fadd_g2", FARITH).hazard(HAZARD_FPU).reads(&[11, 12]).writes(&[13]),
                    DagNode::new("st_g", STORE).kind(St).reads(&[13]),
                    DagNode::new("lfd_c3", LOAD).writes(&[20]),
                    DagNode::new("fadd_h", FARITH).hazard(HAZARD_FPU).reads(&[2, 20]).writes(&[21]),
                    DagNode::new("st_h", STORE).kind(St).reads(&[21]),
                ],
                vec![(1, 1), (2, 2)],
                vec![Some(3), Some(1), Some(3), Some(1), None, Some(0), Some(0), None],
            ),
            (
                // g=a*b+1.5; h=c+2.5: lfd f4 / fmadd f1,f1,f2,f4 / lfd f0 / fadd f0,f0,f3
                "reg_fmadd_latency",
                vec![
                    DagNode::new("lfd_c1", LOAD).writes(&[10]),
                    DagNode::new("fmadd_g", FARITH).hazard(HAZARD_FPU).reads(&[1, 2, 10]).writes(&[11]),
                    DagNode::new("st_g", STORE).kind(St).reads(&[11]),
                    DagNode::new("lfd_c2", LOAD).writes(&[20]),
                    DagNode::new("fadd_h", FARITH).hazard(HAZARD_FPU).reads(&[3, 20]).writes(&[21]),
                    DagNode::new("st_h", STORE).kind(St).reads(&[21]),
                ],
                vec![(1, 1), (2, 2), (3, 3)],
                vec![Some(4), Some(1), None, Some(0), Some(0), None],
            ),
            (
                // g=a*1.5; h=b+2.5: fmul f1,f3,f1 / fadd f0,f0,f2
                "reg_fmul_vs_fadd",
                vec![
                    DagNode::new("lfd_c1", LOAD).writes(&[10]),
                    DagNode::new("fmul_g", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[1, 10]).writes(&[11]),
                    DagNode::new("st_g", STORE).kind(St).reads(&[11]),
                    DagNode::new("lfd_c2", LOAD).writes(&[20]),
                    DagNode::new("fadd_h", FARITH).hazard(HAZARD_FPU).reads(&[2, 20]).writes(&[21]),
                    DagNode::new("st_h", STORE).kind(St).reads(&[21]),
                ],
                vec![(1, 1), (2, 2)],
                vec![Some(3), Some(1), None, Some(0), Some(0), None],
            ),
            (
                // return z*(1.5+z*2.5): lfd f2 / lfd f0 / fmadd f0 / fmul f1
                "reg_horner2",
                vec![
                    DagNode::new("lfd_c25", LOAD).writes(&[10]),
                    DagNode::new("lfd_c15", LOAD).writes(&[11]),
                    DagNode::new("fmadd1", FARITH).hazard(HAZARD_FPU).reads(&[1, 10, 11]).writes(&[12]),
                    DagNode::new("fmul", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[1, 12]).writes(&[13]),
                ],
                vec![(1, 1)],
                vec![Some(2), Some(0), Some(0), Some(1)],
            ),
            (
                // horner3: lfd f3 f2 f0 / fmadd f2 / fmadd f0 / fmul f1
                "reg_horner3",
                vec![
                    DagNode::new("lfd_c35", LOAD).writes(&[10]),
                    DagNode::new("lfd_c25", LOAD).writes(&[11]),
                    DagNode::new("lfd_c15", LOAD).writes(&[12]),
                    DagNode::new("fmadd1", FARITH).hazard(HAZARD_FPU).reads(&[1, 10, 11]).writes(&[13]),
                    DagNode::new("fmadd2", FARITH).hazard(HAZARD_FPU).reads(&[1, 13, 12]).writes(&[14]),
                    DagNode::new("fmul", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[1, 14]).writes(&[15]),
                ],
                vec![(1, 1)],
                vec![Some(3), Some(2), Some(0), Some(2), Some(0), Some(1)],
            ),
            (
                // horner4: lfd f3 f0 f2 / fmadd f3 / lfd f0 / fmadd f2 / fmadd f0 / fmul f1
                "reg_horner4",
                vec![
                    DagNode::new("lfd_c45", LOAD).writes(&[10]),
                    DagNode::new("lfd_c35", LOAD).writes(&[11]),
                    DagNode::new("lfd_c25", LOAD).writes(&[12]),
                    DagNode::new("lfd_c15", LOAD).writes(&[13]),
                    DagNode::new("fmadd1", FARITH).hazard(HAZARD_FPU).reads(&[1, 10, 11]).writes(&[14]),
                    DagNode::new("fmadd2", FARITH).hazard(HAZARD_FPU).reads(&[1, 14, 12]).writes(&[15]),
                    DagNode::new("fmadd3", FARITH).hazard(HAZARD_FPU).reads(&[1, 15, 13]).writes(&[16]),
                    DagNode::new("fmul", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[1, 16]).writes(&[17]),
                ],
                vec![(1, 1)],
                vec![Some(3), Some(0), Some(2), Some(0), Some(3), Some(2), Some(0), Some(1)],
            ),
            (
                // s1_s2: lfd f5 f4 f3 f0 / fmadd f5 / lfd f4 / fmadd f0 /
                // fmadd f3 / fmul f0 / fmadd f1
                "reg_s1_s2",
                vec![
                    DagNode::new("lfd_c35", LOAD).writes(&[10]),
                    DagNode::new("lfd_c25", LOAD).writes(&[11]),
                    DagNode::new("lfd_c55", LOAD).writes(&[12]),
                    DagNode::new("lfd_c45", LOAD).writes(&[13]),
                    DagNode::new("lfd_c15", LOAD).writes(&[14]),
                    DagNode::new("fmadd_a", FARITH).hazard(HAZARD_FPU).reads(&[2, 10, 11]).writes(&[20]),
                    DagNode::new("fmadd_b", FARITH).hazard(HAZARD_FPU).reads(&[2, 12, 13]).writes(&[21]),
                    DagNode::new("fmadd_c", FARITH).hazard(HAZARD_FPU).reads(&[2, 20, 14]).writes(&[22]),
                    DagNode::new("fmul_d", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[2, 21]).writes(&[23]),
                    DagNode::new("fmadd_f", FARITH).hazard(HAZARD_FPU).reads(&[1, 22, 23]).writes(&[24]),
                ],
                vec![(1, 1), (2, 2)],
                vec![
                    Some(5),
                    Some(4),
                    Some(3),
                    Some(0),
                    Some(4),
                    Some(5),
                    Some(0),
                    Some(3),
                    Some(0),
                    Some(1),
                ],
            ),
            (
                // return *p + 1.5 (no float params): lfd f1 / lfd f0 / fadd f1
                "reg_load_use",
                vec![
                    DagNode::new("lfd_c15", LOAD).writes(&[11]),
                    DagNode::new("lfd_p", LOAD).reads(&[100]).writes(&[10]),
                    DagNode::new("fadd", FARITH).hazard(HAZARD_FPU).reads(&[11, 10]).writes(&[12]),
                ],
                vec![],
                vec![Some(1), Some(0), Some(1)],
            ),
            (
                // FIRE-336 PROBE A — w*(h4 inner): window 5; L15 takes the
                // m3 addend-share (f0) even with f4 free; L45 factor-shares f4.
                "reg_h4_wmul",
                vec![
                    DagNode::new("lfd_c45", LOAD).writes(&[10]),
                    DagNode::new("lfd_c35", LOAD).writes(&[11]),
                    DagNode::new("lfd_c25", LOAD).writes(&[12]),
                    DagNode::new("lfd_c15", LOAD).writes(&[13]),
                    DagNode::new("fmadd1", FARITH).hazard(HAZARD_FPU).reads(&[1, 10, 11]).writes(&[14]),
                    DagNode::new("fmadd2", FARITH).hazard(HAZARD_FPU).reads(&[1, 14, 12]).writes(&[15]),
                    DagNode::new("fmadd3", FARITH).hazard(HAZARD_FPU).reads(&[1, 15, 13]).writes(&[16]),
                    DagNode::new("fmul", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[2, 16]).writes(&[17]),
                ],
                vec![(1, 1), (2, 2)],
                vec![Some(4), Some(0), Some(3), Some(0), Some(4), Some(3), Some(0), Some(1)],
            ),
            (
                // FIRE-336 PROBE B — z*(1.5+w*2.5) + w*(3.5+w*4.5): mwcc
                // evaluates the s2 subtree FIRST (loads 4.5, 3.5, 2.5, 1.5);
                // mB accumulates in its addend's f0, the s1 chain in f3, and
                // mB (slot 3, dies 6) allocates BEFORE L15 (slot 4, dies 5)
                // — the death-order keystone.
                "reg_s1_s2_shallow",
                vec![
                    DagNode::new("lfd_c45", LOAD).writes(&[10]),
                    DagNode::new("lfd_c35", LOAD).writes(&[11]),
                    DagNode::new("lfd_c25", LOAD).writes(&[12]),
                    DagNode::new("lfd_c15", LOAD).writes(&[13]),
                    DagNode::new("fmadd_b", FARITH).hazard(HAZARD_FPU).reads(&[2, 10, 11]).writes(&[20]),
                    DagNode::new("fmadd_a", FARITH).hazard(HAZARD_FPU).reads(&[2, 12, 13]).writes(&[21]),
                    DagNode::new("fmul_d", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[2, 20]).writes(&[22]),
                    DagNode::new("fmadd_f", FARITH).hazard(HAZARD_FPU).reads(&[1, 21, 22]).writes(&[23]),
                ],
                vec![(1, 1), (2, 2)],
                vec![Some(3), Some(0), Some(4), Some(3), Some(0), Some(3), Some(0), Some(1)],
            ),
            (
                // FIRE-338 — 1.5 - z*2.5: the single fnmsub root (loads under
                // the standard death-tie start-desc order).
                "reg_fnmsub_single",
                vec![
                    DagNode::new("lfd_c25", LOAD).writes(&[10]),
                    DagNode::new("lfd_c15", LOAD).writes(&[11]),
                    DagNode::new("fnmsub", FARITH).hazard(HAZARD_FPU).reads(&[10, 1, 11]).writes(&[12]),
                ],
                vec![(1, 1)],
                vec![Some(2), Some(0), Some(1)],
            ),
            (
                // FIRE-338 — z*(1.5 - z*(2.5 - z*3.5)): the fnmsub chain has
                // horner3's exact geometry (same registers).
                "reg_fnmsub_chain",
                vec![
                    DagNode::new("lfd_c35", LOAD).writes(&[10]),
                    DagNode::new("lfd_c25", LOAD).writes(&[11]),
                    DagNode::new("lfd_c15", LOAD).writes(&[12]),
                    DagNode::new("fnmsub1", FARITH).hazard(HAZARD_FPU).reads(&[1, 10, 11]).writes(&[13]),
                    DagNode::new("fnmsub2", FARITH).hazard(HAZARD_FPU).reads(&[1, 13, 12]).writes(&[14]),
                    DagNode::new("fmul", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[1, 14]).writes(&[15]),
                ],
                vec![(1, 1)],
                vec![Some(3), Some(2), Some(0), Some(2), Some(0), Some(1)],
            ),
            (
                // FIRE-338 — (z*w)*(1.5+z*2.5): the inner fmul schedules into
                // the load window; the fmadd takes f1 over its dying z (the
                // def-over-dying-param case) and root shares are z-blocked.
                "reg_mul_of_mul",
                vec![
                    DagNode::new("lfd_c25", LOAD).writes(&[10]),
                    DagNode::new("lfd_c15", LOAD).writes(&[11]),
                    DagNode::new("fmul_zw", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[1, 2]).writes(&[20]),
                    DagNode::new("fmadd", FARITH).hazard(HAZARD_FPU).reads(&[10, 1, 11]).writes(&[21]),
                    DagNode::new("fmul_root", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[21, 20]).writes(&[22]),
                ],
                vec![(1, 1), (2, 2)],
                vec![Some(3), Some(2), Some(0), Some(1), Some(1)],
            ),
            (
                // FIRE-338 OPEN — z*(1.5+z*2.5) - 3.5: the fmsub-root shape
                // resolves its equal-death load/arith tie OPPOSITE to h3's
                // identical pair (L35 takes f0 FIRST; the fmadd falls to f2).
                // Undistinguished under FROZEN — kept as the measured miss.
                "reg_fmsub_root",
                vec![
                    DagNode::new("lfd_c25", LOAD).writes(&[10]),
                    DagNode::new("lfd_c15", LOAD).writes(&[11]),
                    DagNode::new("lfd_c35", LOAD).writes(&[12]),
                    DagNode::new("fmadd", FARITH).hazard(HAZARD_FPU).reads(&[10, 1, 11]).writes(&[13]),
                    DagNode::new("fmsub", FARITH).hazard(HAZARD_FPU).reads(&[1, 13, 12]).writes(&[14]),
                ],
                vec![(1, 1)],
                vec![Some(3), Some(2), Some(0), Some(2), Some(1)],
            ),
            (
                // FIRE-339 — z*(h3 chain) - 4.5: the deeper fmsub root. The
                // subtrahend load defers (blocked-load stall) to slot 4 and
                // takes f0 via root-slot order; the chain packs f3/f2/f0.
                "reg_fmsub_deep",
                vec![
                    DagNode::new("lfd_c35", LOAD).writes(&[10]),
                    DagNode::new("lfd_c25", LOAD).writes(&[11]),
                    DagNode::new("lfd_c15", LOAD).writes(&[12]),
                    DagNode::new("lfd_c45", LOAD).writes(&[13]),
                    DagNode::new("fmadd1", FARITH).hazard(HAZARD_FPU).reads(&[10, 1, 11]).writes(&[14]),
                    DagNode::new("fmadd2", FARITH).hazard(HAZARD_FPU).reads(&[1, 14, 12]).writes(&[15]),
                    DagNode::new("fmsub", FARITH).hazard(HAZARD_FPU).reads(&[1, 15, 13]).writes(&[16]),
                ],
                vec![(1, 1)],
                vec![Some(3), Some(0), Some(2), Some(0), Some(3), Some(2), Some(1)],
            ),
            (
                // FIRE-339 — w*(1.5+z*2.5) - 3.5: m1 takes f1 through the
                // DYING DOOR (z vacates f1 to it; the 3-op fmsub consumer
                // boundary passes because the register flows z -> m1 -> root).
                "reg_fmsub_wmul",
                vec![
                    DagNode::new("lfd_c25", LOAD).writes(&[10]),
                    DagNode::new("lfd_c15", LOAD).writes(&[11]),
                    DagNode::new("lfd_c35", LOAD).writes(&[12]),
                    DagNode::new("fmadd1", FARITH).hazard(HAZARD_FPU).reads(&[10, 1, 11]).writes(&[13]),
                    DagNode::new("fmsub", FARITH).hazard(HAZARD_FPU).reads(&[2, 13, 12]).writes(&[14]),
                ],
                vec![(1, 1), (2, 2)],
                vec![Some(4), Some(3), Some(0), Some(1), Some(1)],
            ),
            (
                // FIRE-340 — z=x*x; z*(1.5+z*2.5): the named local takes the
                // window top (f2); x's f1 frees at slot 0 for the 2.5 load.
                "reg_local_z2",
                vec![
                    DagNode::new("fmul_z", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).local_home().reads(&[1, 1]).writes(&[20]),
                    DagNode::new("lfd_c25", LOAD).writes(&[10]),
                    DagNode::new("lfd_c15", LOAD).writes(&[11]),
                    DagNode::new("fmadd1", FARITH).hazard(HAZARD_FPU).reads(&[10, 20, 11]).writes(&[21]),
                    DagNode::new("fmul_root", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[20, 21]).writes(&[22]),
                ],
                vec![(1, 1)],
                vec![Some(2), Some(1), Some(0), Some(0), Some(1)],
            ),
            (
                // FIRE-340 — z=x*x; 3-use horner: z -> f3 (window-top).
                "reg_local_z3",
                vec![
                    DagNode::new("fmul_z", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).local_home().reads(&[1, 1]).writes(&[20]),
                    DagNode::new("lfd_c35", LOAD).writes(&[10]),
                    DagNode::new("lfd_c25", LOAD).writes(&[11]),
                    DagNode::new("lfd_c15", LOAD).writes(&[12]),
                    DagNode::new("fmadd1", FARITH).hazard(HAZARD_FPU).reads(&[10, 20, 11]).writes(&[21]),
                    DagNode::new("fmadd2", FARITH).hazard(HAZARD_FPU).reads(&[20, 21, 12]).writes(&[22]),
                    DagNode::new("fmul_root", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[20, 22]).writes(&[23]),
                ],
                vec![(1, 1)],
                vec![Some(3), Some(2), Some(1), Some(0), Some(1), Some(0), Some(1)],
            ),
            (
                // FIRE-340 — z=x*x; v=z*x (k_sin core): locals descend from
                // the top in declaration order (z f4, v f3); x lives to the
                // root fmadd's addend.
                "reg_local_zv",
                vec![
                    DagNode::new("fmul_z", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).local_home().reads(&[1, 1]).writes(&[20]),
                    DagNode::new("lfd_c25", LOAD).writes(&[10]),
                    DagNode::new("lfd_c15", LOAD).writes(&[11]),
                    DagNode::new("fmul_v", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).local_home().reads(&[20, 1]).writes(&[21]),
                    DagNode::new("fmadd1", FARITH).hazard(HAZARD_FPU).reads(&[10, 20, 11]).writes(&[22]),
                    DagNode::new("fmadd_root", FARITH).hazard(HAZARD_FPU).reads(&[21, 22, 1]).writes(&[23]),
                ],
                vec![(1, 1)],
                vec![Some(4), Some(2), Some(0), Some(3), Some(0), Some(1)],
            ),
            (
                // FIRE-340 — k_sin r-shape (r folded): x + z*(1.5+z*(2.5+z*3.5)).
                "reg_local_ksin_r",
                vec![
                    DagNode::new("fmul_z", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).local_home().reads(&[1, 1]).writes(&[20]),
                    DagNode::new("lfd_c35", LOAD).writes(&[10]),
                    DagNode::new("lfd_c25", LOAD).writes(&[11]),
                    DagNode::new("lfd_c15", LOAD).writes(&[12]),
                    DagNode::new("fmadd1", FARITH).hazard(HAZARD_FPU).reads(&[10, 20, 11]).writes(&[21]),
                    DagNode::new("fmadd2", FARITH).hazard(HAZARD_FPU).reads(&[20, 21, 12]).writes(&[22]),
                    DagNode::new("fmadd_root", FARITH).hazard(HAZARD_FPU).reads(&[20, 22, 1]).writes(&[23]),
                ],
                vec![(1, 1)],
                vec![Some(4), Some(3), Some(2), Some(0), Some(2), Some(0), Some(1)],
            ),
            (
                // FIRE-340 — the DEEP mul-of-mul ((z*w)*(2-arith chain)): the
                // cross-chain product spans two arith defs and joins the
                // window-top tier structurally (f4) — the fire-339 open case.
                "reg_mul_of_mul_deep",
                vec![
                    DagNode::new("lfd_c35", LOAD).writes(&[10]),
                    DagNode::new("lfd_c25", LOAD).writes(&[11]),
                    DagNode::new("lfd_c15", LOAD).writes(&[12]),
                    DagNode::new("fmadd1", FARITH).hazard(HAZARD_FPU).reads(&[10, 1, 11]).writes(&[21]),
                    DagNode::new("fmadd2", FARITH).hazard(HAZARD_FPU).reads(&[1, 21, 12]).writes(&[22]),
                    DagNode::new("fmul_zw", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[1, 2]).writes(&[20]),
                    DagNode::new("fmul_root", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[22, 20]).writes(&[23]),
                ],
                vec![(1, 1), (2, 2)],
                vec![Some(3), Some(2), Some(0), Some(2), Some(0), Some(4), Some(1)],
            ),
            (
                // FIRE-341 — zv_deep: v (local, reads z) STALLS like a
                // blocked load — its tier home f3 is held by L35 until m1
                // frees it; v issues on the empty cycle after m1.
                "reg_local_zv_deep",
                vec![
                    DagNode::new("fmul_z", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).local_home().reads(&[1, 1]).writes(&[20]),
                    DagNode::new("fmul_v", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).local_home().reads(&[20, 1]).writes(&[21]),
                    DagNode::new("lfd_c35", LOAD).writes(&[10]),
                    DagNode::new("lfd_c25", LOAD).writes(&[11]),
                    DagNode::new("lfd_c15", LOAD).writes(&[12]),
                    DagNode::new("fmadd1", FARITH).hazard(HAZARD_FPU).reads(&[10, 20, 11]).writes(&[22]),
                    DagNode::new("fmadd2", FARITH).hazard(HAZARD_FPU).reads(&[20, 22, 12]).writes(&[23]),
                    DagNode::new("fmadd_root", FARITH).hazard(HAZARD_FPU).reads(&[21, 23, 1]).writes(&[24]),
                ],
                vec![(1, 1)],
                vec![Some(4), Some(3), Some(3), Some(2), Some(0), Some(2), Some(0), Some(1)],
            ),
            (
                // FIRE-341 — z4: the four-coefficient local chain (x's f1
                // frees at z; the chain reuses it mid-stream).
                "reg_local_z4",
                vec![
                    DagNode::new("fmul_z", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).local_home().reads(&[1, 1]).writes(&[20]),
                    DagNode::new("lfd_c45", LOAD).writes(&[10]),
                    DagNode::new("lfd_c35", LOAD).writes(&[11]),
                    DagNode::new("lfd_c25", LOAD).writes(&[12]),
                    DagNode::new("lfd_c15", LOAD).writes(&[13]),
                    DagNode::new("fmadd1", FARITH).hazard(HAZARD_FPU).reads(&[10, 20, 11]).writes(&[21]),
                    DagNode::new("fmadd2", FARITH).hazard(HAZARD_FPU).reads(&[20, 21, 12]).writes(&[22]),
                    DagNode::new("fmadd3", FARITH).hazard(HAZARD_FPU).reads(&[20, 22, 13]).writes(&[23]),
                    DagNode::new("fmul_root", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[20, 23]).writes(&[24]),
                ],
                vec![(1, 1)],
                vec![Some(4), Some(3), Some(2), Some(1), Some(0), Some(2), Some(1), Some(0), Some(1)],
            ),
            (
                // FIRE-341 — zv_deeper: the same stall over a 4-coefficient
                // chain (v waits two lift cycles).
                "reg_local_zv_deeper",
                vec![
                    DagNode::new("fmul_z", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).local_home().reads(&[1, 1]).writes(&[20]),
                    DagNode::new("fmul_v", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).local_home().reads(&[20, 1]).writes(&[21]),
                    DagNode::new("lfd_c45", LOAD).writes(&[10]),
                    DagNode::new("lfd_c35", LOAD).writes(&[11]),
                    DagNode::new("lfd_c25", LOAD).writes(&[12]),
                    DagNode::new("lfd_c15", LOAD).writes(&[13]),
                    DagNode::new("fmadd1", FARITH).hazard(HAZARD_FPU).reads(&[10, 20, 11]).writes(&[22]),
                    DagNode::new("fmadd2", FARITH).hazard(HAZARD_FPU).reads(&[20, 22, 12]).writes(&[23]),
                    DagNode::new("fmadd3", FARITH).hazard(HAZARD_FPU).reads(&[20, 23, 13]).writes(&[24]),
                    DagNode::new("fmadd_root", FARITH).hazard(HAZARD_FPU).reads(&[21, 24, 1]).writes(&[25]),
                ],
                vec![(1, 1)],
                vec![Some(5), Some(4), Some(4), Some(3), Some(2), Some(0), Some(3), Some(2), Some(0), Some(1)],
            ),
            (
                // FIRE-345 — the FULL k_sin tail: z/v tier (f7/f6), the six
                // coefficients DESCENDING f6..f2,f0 under the tier-forward
                // machine, the chain riding the dying addends, v sharing
                // L55's f6 serially.
                "reg_ksin_tail",
                vec![
                    DagNode::new("fmul_z", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).local_home().reads(&[1, 1]).writes(&[20]),
                    DagNode::new("fmul_v", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).local_home().reads(&[20, 1]).writes(&[21]),
                    DagNode::new("lfd_c55", LOAD).writes(&[10]),
                    DagNode::new("lfd_c45", LOAD).writes(&[11]),
                    DagNode::new("lfd_c35", LOAD).writes(&[12]),
                    DagNode::new("lfd_c25", LOAD).writes(&[13]),
                    DagNode::new("lfd_c15", LOAD).writes(&[14]),
                    DagNode::new("lfd_c65", LOAD).writes(&[15]),
                    DagNode::new("fmadd1", FARITH).hazard(HAZARD_FPU).reads(&[10, 20, 11]).writes(&[22]),
                    DagNode::new("fmadd2", FARITH).hazard(HAZARD_FPU).reads(&[20, 22, 12]).writes(&[23]),
                    DagNode::new("fmadd3", FARITH).hazard(HAZARD_FPU).reads(&[20, 23, 13]).writes(&[24]),
                    DagNode::new("fmadd4", FARITH).hazard(HAZARD_FPU).reads(&[20, 24, 14]).writes(&[25]),
                    DagNode::new("fmadd5", FARITH).hazard(HAZARD_FPU).reads(&[20, 25, 15]).writes(&[26]),
                    DagNode::new("fmadd_root", FARITH).hazard(HAZARD_FPU).reads(&[21, 26, 1]).writes(&[27]),
                ],
                vec![(1, 1)],
                vec![
                    Some(7),
                    Some(6),
                    Some(6),
                    Some(5),
                    Some(4),
                    Some(3),
                    Some(2),
                    Some(0),
                    Some(5),
                    Some(4),
                    Some(3),
                    Some(2),
                    Some(0),
                    Some(1),
                ],
            ),
            (
                // FIRE-348 — the fctiwz frame-reload, flat tail: the
                // MULTI-CONSUMER reload joins the tier (top f2); f1 is free
                // (x lives in the frame).
                "reg_reload_flat",
                vec![
                    DagNode::new("lfd_x", LOAD).writes(&[30]),
                    DagNode::new("lfd_c25", LOAD).writes(&[10]),
                    DagNode::new("lfd_c15", LOAD).writes(&[11]),
                    DagNode::new("fmadd1", FARITH).hazard(HAZARD_FPU).reads(&[10, 30, 11]).writes(&[20]),
                    DagNode::new("fmul_root", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[30, 20]).writes(&[21]),
                ],
                vec![],
                vec![Some(2), Some(1), Some(0), Some(0), Some(1)],
            ),
            (
                // FIRE-348 — reload + z/v: the tier orders by DEATH ASC
                // (z f4, XR f3, v f2 — the real k_sin's f7/f6/f5 pattern).
                "reg_reload_zv",
                vec![
                    DagNode::new("lfd_x", LOAD).writes(&[30]),
                    DagNode::new("lfd_c25", LOAD).writes(&[10]),
                    DagNode::new("fmul_z", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).local_home().reads(&[30, 30]).writes(&[20]),
                    DagNode::new("lfd_c15", LOAD).writes(&[11]),
                    DagNode::new("fmul_v", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).local_home().reads(&[20, 30]).writes(&[21]),
                    DagNode::new("fmadd1", FARITH).hazard(HAZARD_FPU).reads(&[10, 20, 11]).writes(&[22]),
                    DagNode::new("fmadd_root", FARITH).hazard(HAZARD_FPU).reads(&[21, 22, 30]).writes(&[23]),
                ],
                vec![],
                vec![Some(3), Some(1), Some(4), Some(0), Some(2), Some(0), Some(1)],
            ),
            (
                // FIRE-348 — reload + z3: the SINGLE-consumer reload feeds
                // the tier and allocates ASCENDING (f0) while the
                // coefficients descend f2, f1, f0.
                "reg_reload_z3",
                vec![
                    DagNode::new("lfd_x", LOAD).writes(&[30]),
                    DagNode::new("lfd_c35", LOAD).writes(&[10]),
                    DagNode::new("fmul_z", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).local_home().reads(&[30, 30]).writes(&[20]),
                    DagNode::new("lfd_c25", LOAD).writes(&[11]),
                    DagNode::new("lfd_c15", LOAD).writes(&[12]),
                    DagNode::new("fmadd1", FARITH).hazard(HAZARD_FPU).reads(&[10, 20, 11]).writes(&[21]),
                    DagNode::new("fmadd2", FARITH).hazard(HAZARD_FPU).reads(&[20, 21, 12]).writes(&[22]),
                    DagNode::new("fmul_root", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[20, 22]).writes(&[23]),
                ],
                vec![],
                vec![Some(0), Some(2), Some(3), Some(1), Some(0), Some(1), Some(0), Some(1)],
            ),
            (
                // FIRE-351 — the k_sin ELSE tail: x-(z*(0.5y-v*r)-y)-v*1.5
                // over five params. Everything chains through f0 (ascending
                // + boundary shares); the 0.5 load takes dead r's f4; 1.5
                // ascends past the busy params to f6.
                "reg_ksin_else",
                vec![
                    DagNode::new("fmul_vr", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[3, 4]).writes(&[20]),
                    DagNode::new("lfd_c05", LOAD).writes(&[10]),
                    DagNode::new("lfd_c15", LOAD).writes(&[11]),
                    DagNode::new("fmsub1", FARITH).hazard(HAZARD_FPU).reads(&[10, 2, 20]).writes(&[21]),
                    DagNode::new("fmsub2", FARITH).hazard(HAZARD_FPU).reads(&[5, 21, 2]).writes(&[22]),
                    DagNode::new("fnmsub", FARITH).hazard(HAZARD_FPU).reads(&[11, 3, 22]).writes(&[23]),
                    DagNode::new("fsub", FARITH).emission_ordered().hazard(HAZARD_FPU).reads(&[1, 23]).writes(&[24]),
                ],
                vec![(1, 1), (2, 2), (3, 3), (4, 4), (5, 5)],
                vec![Some(0), Some(4), Some(6), Some(0), Some(0), Some(0), Some(1)],
            ),
            (
                // FIRE-336 PROBE C — w*(h3 inner): window 5; loads f4 f3 f0.
                "reg_h3_wmul",
                vec![
                    DagNode::new("lfd_c35", LOAD).writes(&[10]),
                    DagNode::new("lfd_c25", LOAD).writes(&[11]),
                    DagNode::new("lfd_c15", LOAD).writes(&[12]),
                    DagNode::new("fmadd1", FARITH).hazard(HAZARD_FPU).reads(&[1, 10, 11]).writes(&[13]),
                    DagNode::new("fmadd2", FARITH).hazard(HAZARD_FPU).reads(&[1, 13, 12]).writes(&[14]),
                    DagNode::new("fmul", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[2, 14]).writes(&[15]),
                ],
                vec![(1, 1), (2, 2)],
                vec![Some(4), Some(3), Some(0), Some(3), Some(0), Some(1)],
            ),
        ]
    }

    /// FRONTIER (fire 364, not yet fitted): the in-frame depth-5 dual — the
    /// k_cos chain length. The frozen ORDER model reproduces the real
    /// schedule exactly (C1's load lands after chain1, v after that); the
    /// REGISTERS do not fit the frozen machine: v joins the tier (def-DESC
    /// top f8; the adjacency hypothesis — v is filtered only when it lands
    /// IMMEDIATELY after the first chain arith — fits d4-filtered vs
    /// d5-kept), and the load pattern f5,f0,f4,f3,f1 breaks descending
    /// first-fit (3.5, chain1's addend, takes f0 early; C1 reuses it).
    /// FITTED (fire 364): loads allocate death-DESC/start-DESC ascending
    /// first-fit (dual shapes), dying reuse is availability-checked, and
    /// the adjacency rule keeps v in the tier when a load separates it
    /// from the first chain arith.
    #[test]
    fn float_registers_frontier_d5() {
        const FARITH: u32 = 3;
        const FMUL_D: u32 = 4;
        let nodes = vec![
            DagNode::new("lfd_x", LOAD).writes(&[9]),
            DagNode::new("fmul_z", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).local_home().reads(&[9, 9]).writes(&[40]),
            DagNode::new("fmul_v", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).local_home().reads(&[40, 9]).writes(&[41]),
            DagNode::new("lfd_c45", LOAD).writes(&[42]),
            DagNode::new("lfd_c35", LOAD).writes(&[43]),
            DagNode::new("lfd_c25", LOAD).writes(&[44]),
            DagNode::new("lfd_c15", LOAD).writes(&[45]),
            DagNode::new("lfd_cC2", LOAD).writes(&[46]),
            DagNode::new("lfd_cC1", LOAD).writes(&[47]),
            DagNode::new("chain1", FARITH).hazard(HAZARD_FPU).reads(&[42, 40, 43]).writes(&[48]),
            DagNode::new("chain2", FARITH).hazard(HAZARD_FPU).reads(&[40, 48, 44]).writes(&[49]),
            DagNode::new("chain3", FARITH).hazard(HAZARD_FPU).reads(&[40, 49, 45]).writes(&[50]),
            DagNode::new("chain4", FARITH).hazard(HAZARD_FPU).reads(&[40, 50, 46]).writes(&[51]),
            DagNode::new("chain5", FARITH).hazard(HAZARD_FPU).reads(&[40, 51, 47]).writes(&[52]),
            DagNode::new("sink", 1).kind(OpKind::Store).reads(&[40, 41, 52, 9, 2]),
        ];
        let expected: Vec<Option<u8>> = vec![
            Some(6),
            Some(7),
            Some(8),
            Some(5),
            Some(0),
            Some(4),
            Some(3),
            Some(1),
            Some(0),
            Some(5),
            Some(4),
            Some(3),
            Some(1),
            Some(1),
            None,
        ];
        let order = linearize(&nodes);
        let mut model = FROZEN_FLOAT_REG;
        model.tier_position_desc = true;
        model.window_floor = 7;
        model.void_forward = false;
        let registers = assign_float_registers(&nodes, &order, &[(2, 2)], model);
        let hits = registers.iter().zip(&expected).filter(|(a, b)| a == b).count();
        eprintln!("frontier d5: {hits}/{} against FROZEN+dual", expected.len());
        for (index, (got, want)) in registers.iter().zip(&expected).enumerate() {
            if got != want {
                eprintln!("  node {index}: got {got:?} want {want:?}");
            }
        }
        assert_eq!(registers, expected, "the d5 register frontier is not yet fitted");
    }

    /// FRONTIER (fire 368): the k_cos ELSE TAIL — x re-reload + the diamond
    /// frame local + folded hz/a, an Fsub-rooted tail whose REAL registers
    /// fit the REVERSE machine (death DESC with a consumer-count tiebreak
    /// before start DESC, ascending first-fit, boundary doors, the
    /// pending-sibling block) — NOT the emission-ordered regime the
    /// Fsub-root flag currently forces. Real order also differs from the
    /// frozen scheduler by one swap (inner before a at slot 6).
    /// FITTED (fire 370): the inside-params window (a param consumes a
    /// window slot only while its register sits inside it), the emission
    /// regime overriding the tier-forward dispatch, and the structural
    /// crossing tier gated out of emission-ordered cascades.
    #[test]
    fn float_registers_frontier_kcos_else() {
        const FARITH: u32 = 3;
        let nodes = vec![
            DagNode::new("lfd_x", LOAD).writes(&[9]),
            DagNode::new("lfd_qx", LOAD).writes(&[7]),
            DagNode::new("lfd_05", LOAD).writes(&[12]),
            DagNode::new("lfd_one", LOAD).writes(&[13]),
            DagNode::new("fmul_xy", FARITH).gate(4).hazard(HAZARD_FPU).reads(&[9, 3]).writes(&[14]),
            DagNode::new("fmsub_hz", FARITH).hazard(HAZARD_FPU).reads(&[12, 1, 7]).writes(&[15]),
            DagNode::new("fmsub_in", FARITH).hazard(HAZARD_FPU).reads(&[1, 2, 14]).writes(&[16]),
            DagNode::new("fsub_a", FARITH).hazard(HAZARD_FPU).reads(&[13, 7]).writes(&[17]),
            DagNode::new("fsub_s2", FARITH).hazard(HAZARD_FPU).reads(&[15, 16]).writes(&[18]),
            DagNode::new("fsub_rt", FARITH).hazard(HAZARD_FPU).reads(&[17, 18]).writes(&[19]).emission_ordered(),
        ];
        let order = linearize(&nodes);
        // REAL schedule: x, qx, xy, 0.5, one, hz, INNER, a, s2, root.
        let expected_order: Vec<usize> = vec![0, 1, 4, 2, 3, 5, 6, 7, 8, 9];
        let order_hits = order == expected_order;
        eprintln!("kcos_else order: got {order:?} want {expected_order:?} ({order_hits})");
        let mut model = FROZEN_FLOAT_REG;
        model.emission_over_tier = true;
        let registers = assign_float_registers(&nodes, &order, &[(1, 7), (2, 4), (3, 2)], model);
        let expected: Vec<Option<u8>> = vec![
            Some(0),
            Some(3),
            Some(1),
            Some(2),
            Some(0),
            Some(1),
            Some(0),
            Some(2),
            Some(0),
            Some(1),
        ];
        for (index, (got, want)) in registers.iter().zip(&expected).enumerate() {
            if got != want {
                eprintln!("  reg node {index}: got {got:?} want {want:?}");
            }
        }
        assert!(order_hits, "kcos else ORDER unfitted");
        assert_eq!(registers, expected, "kcos else REGISTERS unfitted");
    }

    /// FRONTIER (fire 373, unfitted): the MULTI-LOCAL coefficient-table
    /// tail — s_atan's even/odd split (`z*(aT0+w*(aT2+w*aT4)) +
    /// w*(aT1+w*aT3)` with z=x*x, w=z*z). Two gaps against the frozen
    /// models: (1) ORDER — the local-dep defer pushes w past ALL five
    /// loads (slot 6); the real schedule interleaves w after TWO (slot 3):
    /// the v-defers-fully rule from k_sin over-fires here; (2) REGISTERS —
    /// the loads land f4,f3,f1,f0,f2 (aT0 takes the freed x slot LAST)
    /// against plain descending. Real: z=f5 w=f6 (tier), chains
    /// f3,f0,f1,f0, root f1. Run with --ignored to score candidates.
    #[test]
    #[ignore]
    fn float_registers_frontier_t5_table() {
        const FARITH: u32 = 3;
        const FMUL_D: u32 = 4;
        let nodes = vec![
            DagNode::new("fmul_z", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).local_home().reads(&[1, 1]).writes(&[10]),
            DagNode::new("fmul_w", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).local_home().reads(&[10, 10]).writes(&[11]),
            DagNode::new("lfd_t4", LOAD).writes(&[12]),
            DagNode::new("lfd_t2", LOAD).writes(&[13]),
            DagNode::new("lfd_t3", LOAD).writes(&[14]),
            DagNode::new("lfd_t1", LOAD).writes(&[15]),
            DagNode::new("lfd_t0", LOAD).writes(&[16]),
            DagNode::new("even1", FARITH).hazard(HAZARD_FPU).reads(&[11, 12, 13]).writes(&[17]),
            DagNode::new("odd1", FARITH).hazard(HAZARD_FPU).reads(&[11, 14, 15]).writes(&[18]),
            DagNode::new("even2", FARITH).hazard(HAZARD_FPU).reads(&[11, 17, 16]).writes(&[19]),
            DagNode::new("mul_odd", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[11, 18]).writes(&[20]),
            DagNode::new("root", FARITH).hazard(HAZARD_FPU).reads(&[10, 19, 20]).writes(&[21]),
        ];
        let order = linearize(&nodes);
        let expected_order: Vec<usize> = vec![0, 2, 3, 1, 4, 5, 6, 7, 8, 9, 10, 11];
        eprintln!("t5 order: got {order:?} want {expected_order:?}");
        let registers = assign_float_registers(&nodes, &order, &[(1, 1)], FROZEN_FLOAT_REG);
        let expected: Vec<Option<u8>> = vec![
            Some(5),
            Some(6),
            Some(4),
            Some(3),
            Some(1),
            Some(0),
            Some(2),
            Some(3),
            Some(0),
            Some(1),
            Some(0),
            Some(1),
        ];
        for (index, (got, want)) in registers.iter().zip(&expected).enumerate() {
            if got != want {
                eprintln!("  t5 reg node {index}: got {got:?} want {want:?}");
            }
        }
        assert_eq!(order, expected_order, "t5 ORDER unfitted");
        assert_eq!(registers, expected, "t5 REGISTERS unfitted");
    }

    /// The SHALLOW multi-local table shape (fire 374): z,w + three
    /// coefficients — plain descending loads, no interleave. Checks
    /// whether the frozen machine already covers the V2 sub-class.
    #[test]
    #[ignore]
    fn float_registers_frontier_t5_shallow() {
        const FARITH: u32 = 3;
        const FMUL_D: u32 = 4;
        let nodes = vec![
            DagNode::new("fmul_z", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).local_home().reads(&[1, 1]).writes(&[10]),
            DagNode::new("fmul_w", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).local_home().reads(&[10, 10]).writes(&[11]),
            DagNode::new("lfd_t2", LOAD).writes(&[12]),
            DagNode::new("lfd_t0", LOAD).writes(&[13]),
            DagNode::new("lfd_t1", LOAD).writes(&[14]),
            DagNode::new("even1", FARITH).hazard(HAZARD_FPU).reads(&[11, 12, 13]).writes(&[15]),
            DagNode::new("mul_odd", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[11, 14]).writes(&[16]),
            DagNode::new("root", FARITH).hazard(HAZARD_FPU).reads(&[10, 15, 16]).writes(&[17]),
        ];
        let order = linearize(&nodes);
        let expected_order: Vec<usize> = vec![0, 2, 3, 1, 4, 5, 6, 7];
        eprintln!("v2 order: got {order:?} want {expected_order:?}");
        let registers = assign_float_registers(&nodes, &order, &[(1, 1)], FROZEN_FLOAT_REG);
        let expected: Vec<Option<u8>> = vec![
            Some(3),
            Some(4),
            Some(2),
            Some(1),
            Some(0),
            Some(1),
            Some(0),
            Some(1),
        ];
        for (index, (got, want)) in registers.iter().zip(&expected).enumerate() {
            if got != want {
                eprintln!("  v2 reg node {index}: got {got:?} want {want:?}");
            }
        }
        assert_eq!(order, expected_order, "v2 ORDER unfitted");
        assert_eq!(registers, expected, "v2 REGISTERS unfitted");
    }

    /// THE DEEP-FIT ENUMERATOR (fire 376): score every knob combo against
    /// the pinned fixtures AND the frontier captures simultaneously —
    /// replaces sequential hand-fitting (four hand-fits each broke on the
    /// fifth shape). Run with --ignored --nocapture.
    #[test]
    #[ignore]
    fn float_registers_deep_fit() {
        const FARITH: u32 = 3;
        const FMUL_D: u32 = 4;
        let mut shapes = float_register_fixtures();
        // T5 (the 5-coefficient s_atan split).
        shapes.push((
            "frontier_t5",
            vec![
                DagNode::new("fmul_z", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).local_home().reads(&[1, 1]).writes(&[10]),
                DagNode::new("fmul_w", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).local_home().reads(&[10, 10]).writes(&[11]),
                DagNode::new("lfd_t4", LOAD).writes(&[12]),
                DagNode::new("lfd_t2", LOAD).writes(&[13]),
                DagNode::new("lfd_t3", LOAD).writes(&[14]),
                DagNode::new("lfd_t1", LOAD).writes(&[15]),
                DagNode::new("lfd_t0", LOAD).writes(&[16]),
                DagNode::new("even1", FARITH).hazard(HAZARD_FPU).reads(&[11, 12, 13]).writes(&[17]),
                DagNode::new("odd1", FARITH).hazard(HAZARD_FPU).reads(&[11, 14, 15]).writes(&[18]),
                DagNode::new("even2", FARITH).hazard(HAZARD_FPU).reads(&[11, 17, 16]).writes(&[19]),
                DagNode::new("mul_odd", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[11, 18]).writes(&[20]),
                DagNode::new("root", FARITH).hazard(HAZARD_FPU).reads(&[10, 19, 20]).writes(&[21]),
            ],
            vec![(1, 1)],
            vec![
                Some(5), Some(6), Some(4), Some(3), Some(1), Some(0), Some(2), Some(3), Some(0), Some(1), Some(0), Some(1),
            ],
        ));
        // V3 (the 7-coefficient split).
        shapes.push((
            "frontier_v3",
            vec![
                DagNode::new("fmul_z", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).local_home().reads(&[1, 1]).writes(&[10]),
                DagNode::new("fmul_w", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).local_home().reads(&[10, 10]).writes(&[11]),
                DagNode::new("lfd_t6", LOAD).writes(&[12]),
                DagNode::new("lfd_t4", LOAD).writes(&[13]),
                DagNode::new("lfd_t5", LOAD).writes(&[14]),
                DagNode::new("lfd_t3", LOAD).writes(&[15]),
                DagNode::new("lfd_t2", LOAD).writes(&[16]),
                DagNode::new("even1", FARITH).hazard(HAZARD_FPU).reads(&[11, 12, 13]).writes(&[17]),
                DagNode::new("lfd_t1", LOAD).writes(&[18]),
                DagNode::new("lfd_t0", LOAD).writes(&[19]),
                DagNode::new("odd1", FARITH).hazard(HAZARD_FPU).reads(&[11, 14, 15]).writes(&[20]),
                DagNode::new("even2", FARITH).hazard(HAZARD_FPU).reads(&[11, 17, 16]).writes(&[21]),
                DagNode::new("odd2", FARITH).hazard(HAZARD_FPU).reads(&[11, 20, 18]).writes(&[22]),
                DagNode::new("even3", FARITH).hazard(HAZARD_FPU).reads(&[11, 21, 19]).writes(&[23]),
                DagNode::new("mul_odd", FARITH).gate(FMUL_D).hazard(HAZARD_FPU).reads(&[11, 22]).writes(&[24]),
                DagNode::new("root", FARITH).hazard(HAZARD_FPU).reads(&[10, 23, 24]).writes(&[25]),
            ],
            vec![(1, 1)],
            vec![
                Some(6), Some(7), Some(3), Some(0), Some(2), Some(1), Some(4), Some(5), Some(0), Some(3), Some(1), Some(2), Some(0), Some(1), Some(0), Some(1),
            ],
        ));
        for prepass_always in [false, true] {
            for prepass_start_asc in [false, true] {
                for chain_lowest in [false, true] {
                    let mut model = FROZEN_FLOAT_REG;
                    model.prepass_always = prepass_always;
                    model.prepass_start_asc = prepass_start_asc;
                    model.chain_lowest = chain_lowest;
                    let mut full = 0usize;
                    let mut names: Vec<&str> = Vec::new();
                    for (name, nodes, params, expected) in &shapes {
                        let order = linearize(nodes);
                        let registers = assign_float_registers(nodes, &order, params, model);
                        if &registers == expected {
                            full += 1;
                        } else {
                            names.push(name);
                        }
                    }
                    eprintln!(
                        "prepass_always={} start_asc={} chain_lowest={} => {}/{} (miss: {:?})",
                        prepass_always,
                        prepass_start_asc,
                        chain_lowest,
                        full,
                        shapes.len(),
                        &names[..names.len().min(6)]
                    );
                }
            }
        }
    }

    /// Diagnostic: score both float register machines across the model space.
    #[test]
    fn float_registers() {
        let shapes = float_register_fixtures();
        let mut models: Vec<FloatRegModel> = Vec::new();
        for share_loads in [false, true] {
            for share_arith in [false, true] {
                for arith_share_two_op_only in [false, true] {
                    for share_f0_only in [false, true] {
                        for share_blocked_by_pending_arith in [false, true] {
                            for order_by_death in [false, true] {
                                for void_forward in [false, true] {
                                    for dying_pick in
                                        [DyingPick::MinReg, DyingPick::MaxReg, DyingPick::OldestDef, DyingPick::NewestDef]
                                    {
                                        for root_slot_order in [false, true] {
                                            for dying_door_share in [false, true] {
                                                for local_top_tier in [false, true] {
                                                    for tier_forward_descending in [false, true] {
                                                        models.push(FloatRegModel {
                                                            reverse: true,
                                                            share_loads,
                                                            share_arith,
                                                            arith_share_two_op_only,
                                                            share_f0_only,
                                                            share_blocked_by_pending_arith,
                                                            local_top_tier,
                                                            window_floor: 0,
                                                            tier_position_desc: false,
                                                            legacy_three_tier_rotation: false,
                                                            legacy_inframe_full_tier: false,
                                                            emission_over_tier: false,
                                                            prepass_always: false,
                                                            prepass_start_asc: false,
                                                            chain_lowest: false,
                                                            tier_forward_descending,
                                                            dying_door_share,
                                                            order_by_death,
                                                            root_slot_order,
                                                            dying_pick,
                                                            init_ascending: true,
                                                            void_forward,
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        for dying_pick in [DyingPick::MinReg, DyingPick::MaxReg, DyingPick::OldestDef, DyingPick::NewestDef] {
            for init_ascending in [true, false] {
                models.push(FloatRegModel {
                    reverse: false,
                    share_loads: false,
                    share_arith: false,
                    arith_share_two_op_only: false,
                    share_f0_only: false,
                    share_blocked_by_pending_arith: false,
                    local_top_tier: false,
                    window_floor: 0,
                    tier_position_desc: false,
                    legacy_three_tier_rotation: false,
                    legacy_inframe_full_tier: false,
                    emission_over_tier: false,
                    prepass_always: false,
                    prepass_start_asc: false,
                    chain_lowest: false,
                    tier_forward_descending: false,
                    dying_door_share: false,
                    order_by_death: false,
                    root_slot_order: false,
                    dying_pick,
                    init_ascending,
                    void_forward: false,
                });
            }
        }
        let mut best: (usize, Option<FloatRegModel>) = (0, None);
        for model in &models {
            let passed = shapes
                .iter()
                .filter(|(_, nodes, params, expected)| {
                    let order = linearize(nodes);
                    assign_float_registers(nodes, &order, params, *model) == *expected
                })
                .count();
            if passed > best.0 {
                best = (passed, Some(*model));
            }
        }
        let (best_score, best_model) = (best.0, best.1.expect("nonempty model space"));
        println!("float registers BEST {best_model:?}: {best_score}/{}", shapes.len());
        // Score FROZEN and pin everything but the documented open case.
        let mut frozen_passed = 0;
        for (name, nodes, params, expected) in &shapes {
            let order = linearize(nodes);
            let got = assign_float_registers(nodes, &order, params, FROZEN_FLOAT_REG);
            if got == *expected {
                frozen_passed += 1;
            } else {
                println!("  frozen reg MISS {name}: got {got:?} want {expected:?}");
            }
        }
        println!("float registers FROZEN: {frozen_passed}/{}", shapes.len());
        assert_eq!(frozen_passed, shapes.len(), "FROZEN_FLOAT_REG regressed");
    }

    /// RETURN-TAIL ORDER fixtures (fire 306 captures): expected EMISSION order
    /// as node indices. The tail rank (store vs return final) is the open
    /// sub-model; shapes marked false do not pass FROZEN yet.
    fn return_tail_order_fixtures() -> Vec<(&'static str, Vec<DagNode>, Vec<usize>, bool)> {
        use OpKind::Store as St;
        vec![
            (
                // g=a+1; h=b-3; ret (c&15)|1: addi addi mask stw_g ori stw_h
                "tail_plain2",
                vec![
                    DagNode::new("addi_g", ALU).reads(&[1]).writes(&[10]),
                    DagNode::new("st_g", STORE).kind(St).r3_chain_store().reads(&[10]),
                    DagNode::new("addi_h", ALU).reads(&[2]).writes(&[20]),
                    DagNode::new("st_h", STORE).kind(St).reads(&[20]),
                    DagNode::new("mask", ALU).reads(&[3]).writes(&[30]),
                    DagNode::new("ori", ALU).reads(&[30]).writes(&[31]),
                ],
                vec![0, 2, 4, 1, 5, 3],
                false,
            ),
            (
                // g=a+1; h=b*3; ret (c&15)|1: mulli addi mask stw_g stw_h ori
                "tail_one_mulli",
                vec![
                    DagNode::new("addi_g", ALU).reads(&[1]).writes(&[10]),
                    DagNode::new("st_g", STORE).kind(St).r3_chain_store().reads(&[10]),
                    DagNode::new("mulli_h", MUL).gate(2).hazard(HAZARD_MUL).reads(&[2]).writes(&[20]),
                    DagNode::new("st_h", STORE).kind(St).reads(&[20]),
                    DagNode::new("mask", ALU).reads(&[3]).writes(&[30]),
                    DagNode::new("ori", ALU).reads(&[30]).writes(&[31]),
                ],
                vec![2, 0, 4, 1, 3, 5],
                true,
            ),
            (
                // g=a*5; h=b*3; ret (c&15)|1: mulli_g mask ori mulli_h stw_g stw_h
                "tail_two_mulli",
                vec![
                    DagNode::new("mulli_g", MUL).gate(2).hazard(HAZARD_MUL).reads(&[1]).writes(&[10]),
                    DagNode::new("st_g", STORE).kind(St).r3_chain_store().reads(&[10]),
                    DagNode::new("mulli_h", MUL).gate(2).hazard(HAZARD_MUL).reads(&[2]).writes(&[20]),
                    DagNode::new("st_h", STORE).kind(St).reads(&[20]),
                    DagNode::new("mask", ALU).reads(&[3]).writes(&[30]),
                    DagNode::new("ori", ALU).reads(&[30]).writes(&[31]),
                ],
                vec![0, 4, 5, 2, 1, 3],
                false,
            ),
            (
                // 3 stores + 2-op ret: g h k mask stw_g ori stw_h stw_k
                "tail_three_stores",
                vec![
                    DagNode::new("addi_g", ALU).reads(&[1]).writes(&[10]),
                    DagNode::new("st_g", STORE).kind(St).r3_chain_store().reads(&[10]),
                    DagNode::new("addi_h", ALU).reads(&[2]).writes(&[20]),
                    DagNode::new("st_h", STORE).kind(St).reads(&[20]),
                    DagNode::new("addi_k", ALU).reads(&[3]).writes(&[30]),
                    DagNode::new("st_k", STORE).kind(St).reads(&[30]),
                    DagNode::new("mask", ALU).reads(&[4]).writes(&[40]),
                    DagNode::new("ori", ALU).reads(&[40]).writes(&[41]),
                ],
                vec![0, 2, 4, 6, 1, 7, 3, 5],
                false,
            ),
            (
                // g=a+1; ret ((b&15)|1)+2: addi mask ori stw_g addf
                "tail_deep_return",
                vec![
                    DagNode::new("addi_g", ALU).reads(&[1]).writes(&[10]),
                    DagNode::new("st_g", STORE).kind(St).r3_chain_store().reads(&[10]),
                    DagNode::new("mask", ALU).reads(&[2]).writes(&[20]),
                    DagNode::new("ori", ALU).reads(&[20]).writes(&[21]),
                    DagNode::new("addf", ALU).reads(&[21]).writes(&[22]),
                ],
                vec![0, 2, 3, 1, 4],
                false,
            ),
            (
                // g=a+1; ret (b&15)|1: addi mask stw_g ori
                "tail_single_store",
                vec![
                    DagNode::new("addi_g", ALU).reads(&[1]).writes(&[10]),
                    DagNode::new("st_g", STORE).kind(St).r3_chain_store().reads(&[10]),
                    DagNode::new("mask", ALU).reads(&[2]).writes(&[20]),
                    DagNode::new("ori", ALU).reads(&[20]).writes(&[21]),
                ],
                vec![0, 2, 1, 3],
                true,
            ),
            (
                // g=a+1; h=b-3; ret (c>>2)+1: addi addi srawi stw_g ret stw_h
                "tail_forbidden_feeder",
                vec![
                    DagNode::new("addi_g", ALU).reads(&[1]).writes(&[10]),
                    DagNode::new("st_g", STORE).kind(St).r3_chain_store().reads(&[10]),
                    DagNode::new("addi_h", ALU).reads(&[2]).writes(&[20]),
                    DagNode::new("st_h", STORE).kind(St).reads(&[20]),
                    DagNode::new("srawi", ALU).hazard(HAZARD_XER).forbid_r0().reads(&[3]).writes(&[30]),
                    DagNode::new("ret", ALU).reads(&[30]).writes(&[31]),
                ],
                vec![0, 2, 4, 1, 5, 3],
                false,
            ),
            (
                // g=a+1; h=b*3; ret c-2: mulli addi stw_g ret stw_h (BYTE today)
                "tail_ret_three",
                vec![
                    DagNode::new("addi_g", ALU).reads(&[1]).writes(&[10]),
                    DagNode::new("st_g", STORE).kind(St).r3_chain_store().reads(&[10]),
                    DagNode::new("mulli_h", MUL).gate(2).hazard(HAZARD_MUL).reads(&[2]).writes(&[20]),
                    DagNode::new("st_h", STORE).kind(St).reads(&[20]),
                    DagNode::new("ret", ALU).reads(&[3]).writes(&[30]),
                ],
                vec![2, 0, 1, 4, 3],
                true,
            ),
        ]
    }

    /// Diagnostic: score FROZEN against the return-tail order captures.
    #[test]
    fn return_tail_orders() {
        let shapes = return_tail_order_fixtures();
        let mut passed = 0;
        for (name, nodes, expected, must_pass) in &shapes {
            let got = linearize(nodes);
            let ok = got == *expected;
            if ok {
                passed += 1;
            } else {
                let labels: Vec<&str> = got.iter().map(|&index| nodes[index].label).collect();
                let want: Vec<&str> = expected.iter().map(|&index| nodes[index].label).collect();
                println!("tail MISS {name}: got {labels:?} want {want:?}");
            }
            if *must_pass {
                assert!(ok, "return-tail fixture {name} regressed");
            }
        }
        println!("return-tail orders: {passed}/{}", shapes.len());
    }

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
                            let model = Model { issue_width, gate_on_complete, gated_last, kind_rank, weight_before_kind, strategy, port_aware: false };
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
            DagNode::new("stw_g", STORE).kind(OpKind::Store).reads(&[11]),
            DagNode::new("addi_h", ALU).reads(&[2]).writes(&[20]),
            DagNode::new("mulli_h", MUL).reads(&[20]).writes(&[21]),
            DagNode::new("stw_h", STORE).kind(OpKind::Store).reads(&[21]),
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
            DagNode::new("stw_g", STORE).kind(OpKind::Store).reads(&[12]),
            DagNode::new("addi_h", ALU).reads(&[2]).writes(&[20]),
            DagNode::new("stw_h", STORE).kind(OpKind::Store).reads(&[20]),
        ];
        assert_eq!(labels(&nodes), ["addi_g", "addi_h", "slwi_g", "stw_h", "addi3_g", "stw_g"]);
    }

    #[test]
    fn divide_chain_delays_the_div_store() {
        // g = a/b; h = a+b;  ->  divw add stw(h) stw(g)
        let nodes = [
            DagNode::new("divw", DIV).reads(&[1, 2]).writes(&[10]),
            DagNode::new("stw_g", STORE).kind(OpKind::Store).reads(&[10]),
            DagNode::new("add", ALU).reads(&[1, 2]).writes(&[20]),
            DagNode::new("stw_h", STORE).kind(OpKind::Store).reads(&[20]),
        ];
        assert_eq!(labels(&nodes), ["divw", "add", "stw_h", "stw_g"]);
    }

    #[test]
    fn two_mults_keep_source_order() {
        // g = a*5; h = b*7;  ->  mulli mulli stw stw
        let nodes = [
            DagNode::new("mulli_g", MUL).reads(&[1]).writes(&[10]),
            DagNode::new("stw_g", STORE).kind(OpKind::Store).reads(&[10]),
            DagNode::new("mulli_h", MUL).reads(&[2]).writes(&[20]),
            DagNode::new("stw_h", STORE).kind(OpKind::Store).reads(&[20]),
        ];
        assert_eq!(labels(&nodes), ["mulli_g", "mulli_h", "stw_g", "stw_h"]);
    }

    #[test]
    fn load_vs_mult_prioritizes_the_deeper_alu_chain() {
        // g = *p + 5; h = (b+2)*3;  ->  addi(h) lwz mulli addi5 stw(g) stw(h)
        let nodes = [
            DagNode::new("lwz", LOAD).reads(&[1]).writes(&[10]),
            DagNode::new("addi5_g", ALU).reads(&[10]).writes(&[11]),
            DagNode::new("stw_g", STORE).kind(OpKind::Store).reads(&[11]),
            DagNode::new("addi_h", ALU).reads(&[2]).writes(&[20]),
            DagNode::new("mulli_h", MUL).reads(&[20]).writes(&[21]),
            DagNode::new("stw_h", STORE).kind(OpKind::Store).reads(&[21]),
        ];
        assert_eq!(labels(&nodes), ["addi_h", "lwz", "mulli_h", "addi5_g", "stw_g", "stw_h"]);
    }

    #[test]
    fn alu_ties_keep_source_order() {
        // g = a+1; h = b+2; k = c+3;  ->  a1 a2 a3 st1 st2 st3
        let nodes = [
            DagNode::new("a1", ALU).reads(&[1]).writes(&[10]),
            DagNode::new("st1", STORE).kind(OpKind::Store).reads(&[10]),
            DagNode::new("a2", ALU).reads(&[2]).writes(&[20]),
            DagNode::new("st2", STORE).kind(OpKind::Store).reads(&[20]),
            DagNode::new("a3", ALU).reads(&[3]).writes(&[30]),
            DagNode::new("st3", STORE).kind(OpKind::Store).reads(&[30]),
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
            DagNode::new("stw_g", STORE).kind(OpKind::Store).reads(&[10]).alias(0),
            DagNode::new("stw_q", STORE).kind(OpKind::Store).reads(&[2]).alias(0),
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
            DagNode::new("stfd_g", STORE).kind(OpKind::Store).reads(&[11]),
            DagNode::new("lfd_c2", LOAD).writes(&[20]),
            DagNode::new("fadd", FP).reads(&[2, 20]).writes(&[21]),
            DagNode::new("stfd_h", STORE).kind(OpKind::Store).reads(&[21]),
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
            DagNode::new("stw_eptr", STORE).kind(OpKind::Store).reads(&[13]).alias(1),
            // s2: *(int*)&x = (hx & M) | C   (through the x slot, after the spill)
            DagNode::new("rlwinm", ALU).reads(&[3]).writes(&[20]),
            DagNode::new("oris", ALU).reads(&[20]).writes(&[21]),
            DagNode::new("stfd_spill", STORE).kind(OpKind::Store).alias(2),
            DagNode::new("stw_slot", STORE).kind(OpKind::Store).reads(&[21]).alias(2),
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
            DagNode::new("stw_g", STORE).kind(OpKind::Store).reads(&[10]),
            DagNode::new("lwz_q", LOAD).reads(&[2]).writes(&[20]).after(1),
            DagNode::new("stw_h", STORE).kind(OpKind::Store).reads(&[20]),
        ];
        assert_eq!(labels(&nodes), ["lwz_p", "stw_g", "lwz_q", "stw_h"]);
    }

    /// The INT LOCAL-ALLOCATOR fitter (fires 391-397): twelve captured
    /// register maps from the punned/shift-local family (see
    /// docs/int-allocator-frontier.md; regenerate via tools/probe.sh).
    /// Values are (class, def, last-read, expected register); r0-assigned
    /// values (branch-free folds/rewrites/single-use masks) are excluded —
    /// the r0 policy is fitted separately and holds on all captures.
    mod int_alloc_fit {
        /// Value classes: the constant-synthesis temp, the mask constant,
        /// the computed guard local (j0), a loaded home dead before the
        /// first branch (discarded), a loaded home that survives it, and
        /// the multi-use shifted mask.
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum Class {
            Temp,
            Mask,
            Computed,
            LoadDiscarded,
            LoadSurviving,
            Shift,
            ArmShift,
            /// The ladder scrutinee: read by multiple top-level branch
            /// conditions (the composed s_floor's j0).
            Scrutinee,
        }
        pub struct Value {
            pub class: Class,
            pub def: u32,
            pub last: u32,
            pub expected: u8,
        }
        pub struct Fixture {
            pub name: &'static str,
            pub values: &'static [Value],
        }
        const fn v(class: Class, def: u32, last: u32, expected: u8) -> Value {
            Value { class, def, last, expected }
        }
        use Class::*;
        pub const FIXTURES: &[Fixture] = &[
            Fixture { name: "V1", values: &[
                v(Temp, 1, 2, 3), v(Mask, 2, 8, 4), v(Computed, 6, 7, 3),
                v(LoadSurviving, 4, 13, 5), v(LoadSurviving, 5, 14, 6),
            ]},
            Fixture { name: "V1b", values: &[
                v(Temp, 1, 2, 3), v(Mask, 2, 8, 5), v(Computed, 6, 7, 4),
                v(LoadSurviving, 4, 15, 6), v(LoadDiscarded, 5, 10, 3),
                v(Shift, 8, 13, 4),
            ]},
            Fixture { name: "V1c", values: &[
                v(Temp, 1, 2, 3), v(Mask, 2, 7, 4), v(Computed, 5, 6, 3),
                v(LoadSurviving, 4, 12, 5), v(Shift, 7, 11, 3),
            ]},
            Fixture { name: "V1d", values: &[
                v(Temp, 1, 4, 3), v(Mask, 4, 9, 4), v(Computed, 7, 8, 3),
                v(LoadSurviving, 5, 11, 5),
            ]},
            Fixture { name: "W4", values: &[
                v(Temp, 1, 2, 3), v(Mask, 2, 7, 4), v(Computed, 5, 6, 3),
                v(LoadSurviving, 4, 14, 5), v(Shift, 7, 11, 3),
            ]},
            Fixture { name: "W7", values: &[
                v(Mask, 1, 6, 4), v(Computed, 4, 5, 3),
                v(LoadSurviving, 3, 11, 5), v(Shift, 6, 10, 3),
            ]},
            Fixture { name: "W8", values: &[
                v(Computed, 3, 4, 3), v(LoadSurviving, 2, 10, 4),
                v(Shift, 5, 9, 3),
            ]},
            Fixture { name: "W10", values: &[
                v(Temp, 1, 2, 3), v(Mask, 2, 8, 4), v(Computed, 6, 7, 3),
                v(LoadSurviving, 4, 16, 5), v(LoadSurviving, 5, 17, 6),
                v(Shift, 8, 13, 3),
            ]},
            Fixture { name: "W11", values: &[
                v(Temp, 1, 2, 3), v(Mask, 2, 7, 4), v(Computed, 5, 6, 3),
                v(LoadSurviving, 4, 13, 5), v(Shift, 7, 11, 3),
            ]},
            Fixture { name: "D1", values: &[
                v(Temp, 1, 2, 3), v(Mask, 2, 8, 4), v(Computed, 6, 7, 3),
                v(LoadSurviving, 4, 15, 5), v(LoadSurviving, 5, 14, 6),
                v(Shift, 8, 13, 3),
            ]},
            // D2/D3 reproduce V1b exactly (value and mutation order are
            // irrelevant) — encoded once as V1b.
            Fixture { name: "D4", values: &[
                v(Temp, 1, 2, 3), v(Mask, 2, 8, 4), v(Computed, 6, 7, 3),
                v(LoadDiscarded, 4, 9, 6), v(LoadDiscarded, 5, 10, 5),
            ]},
            Fixture { name: "D5", values: &[
                v(Temp, 1, 2, 3), v(Mask, 2, 8, 4), v(Computed, 6, 7, 3),
                v(LoadSurviving, 4, 17, 5), v(LoadSurviving, 5, 12, 6),
                v(Shift, 8, 15, 3),
            ]},
            // D4 with the test operands swapped — the discarded pair's
            // DEATHS flip but the registers do not: the crosserless
            // intra-key is def-desc (structural), not death-based.
            Fixture { name: "D4c", values: &[
                v(Temp, 1, 2, 3), v(Mask, 2, 8, 4), v(Computed, 6, 7, 3),
                v(LoadDiscarded, 4, 10, 6), v(LoadDiscarded, 5, 9, 5),
            ]},
            // Standalone s_floor arm3 (fire 400): j0=r4, i=r7.
            Fixture { name: "ARM3", values: &[
                v(Mask, 1, 8, 3), v(Mask, 19, 20, 3), v(Computed, 5, 18, 4),
                v(LoadSurviving, 3, 27, 5), v(LoadSurviving, 4, 28, 6),
                v(Shift, 8, 26, 7),
            ]},
            // The COMPOSED full s_floor (fire 401): the same arm's j0/i
            // SWAP (i=r4, j0=r7) — j0 is the ladder SCRUTINEE and the
            // shifts are ARM-DEFINED (they join the death-asc pool).
            Fixture { name: "SFLOOR", values: &[
                v(Temp, 4, 5, 3), v(Temp, 26, 40, 3), v(Mask, 52, 53, 3),
                v(Mask, 69, 70, 3), v(Scrutinee, 5, 68, 7),
                v(LoadSurviving, 2, 77, 5), v(LoadSurviving, 3, 78, 6),
                v(ArmShift, 28, 42, 4), v(ArmShift, 53, 76, 4),
            ]},
        ];

        /// THE FITTED MODEL (v2, 13/13) — the public crate module; the
        /// pinned test drives it through this adapter.
        pub fn model_order(fixture: &Fixture) -> Vec<usize> {
            let values: Vec<crate::int_alloc::Value> = fixture
                .values
                .iter()
                .map(|value| crate::int_alloc::Value {
                    class: to_public(value.class),
                    def: value.def,
                    last: value.last,
                })
                .collect();
            crate::int_alloc::model_order(&values)
        }

        fn to_public(class: Class) -> crate::int_alloc::Class {
            match class {
                Class::Temp => crate::int_alloc::Class::Temp,
                Class::Mask => crate::int_alloc::Class::Mask,
                Class::Computed => crate::int_alloc::Class::Computed,
                Class::LoadDiscarded => crate::int_alloc::Class::LoadDiscarded,
                Class::LoadSurviving => crate::int_alloc::Class::LoadSurviving,
                Class::Shift => crate::int_alloc::Class::Shift,
                Class::ArmShift => crate::int_alloc::Class::ArmShift,
                Class::Scrutinee => crate::int_alloc::Class::Scrutinee,
            }
        }

        /// Lowest register in r3..r10 free over [def,last] (inclusive
        /// overlap), given prior assignments.
        pub fn assign(order: &[usize], values: &[Value]) -> Vec<u8> {
            let mut chosen = vec![0u8; values.len()];
            for &index in order {
                let value = &values[index];
                'reg: for register in 3u8..=10 {
                    for &previous in order.iter().take_while(|&&p| p != index) {
                        if chosen[previous] == register {
                            let other = &values[previous];
                            if value.def <= other.last && other.def <= value.last {
                                continue 'reg;
                            }
                        }
                    }
                    chosen[index] = register;
                    break;
                }
            }
            chosen
        }
    }

    /// Exploratory: enumerate (class permutation x intra-class key x
    /// conditional split) against all twelve register maps. Run with
    /// `cargo test -p mwcc-vreg int_allocator_deep_fit -- --ignored
    /// --nocapture`.
    #[test]
    #[ignore]
    fn int_allocator_deep_fit() {
        use int_alloc_fit::*;
        use Class::*;
        const CLASSES: [Class; 7] =
            [Temp, Mask, Computed, LoadDiscarded, LoadSurviving, Shift, Scrutinee];
        // Intra-class ordering keys.
        #[derive(Clone, Copy, Debug)]
        enum Key {
            DefAsc,
            DefDesc,
            DeathAsc,
            DeathDesc,
        }
        const KEYS: [Key; 4] = [Key::DefAsc, Key::DefDesc, Key::DeathAsc, Key::DeathDesc];
        fn ordered(fixture: &Fixture, permutation: &[Class], load_key: Key) -> Vec<usize> {
            let mut order: Vec<usize> = Vec::new();
            for &class in permutation {
                let mut members: Vec<usize> = (0..fixture.values.len())
                    .filter(|&i| fixture.values[i].class == class)
                    .collect();
                members.sort_by_key(|&i| {
                    let value = &fixture.values[i];
                    match load_key {
                        Key::DefAsc => value.def as i64,
                        Key::DefDesc => -(value.def as i64),
                        Key::DeathAsc => value.last as i64,
                        Key::DeathDesc => -(value.last as i64),
                    }
                });
                order.extend(members);
            }
            order
        }
        fn matches(fixture: &Fixture, permutation: &[Class], load_key: Key) -> bool {
            let order = ordered(fixture, permutation, load_key);
            let chosen = assign(&order, fixture.values);
            (0..fixture.values.len()).all(|i| chosen[i] == fixture.values[i].expected)
        }
        // All permutations of the six classes.
        fn permutations(classes: &[Class]) -> Vec<Vec<Class>> {
            if classes.len() <= 1 {
                return vec![classes.to_vec()];
            }
            let mut result = Vec::new();
            for (index, &head) in classes.iter().enumerate() {
                let mut rest = classes.to_vec();
                rest.remove(index);
                for mut tail in permutations(&rest) {
                    tail.insert(0, head);
                    result.push(tail);
                }
            }
            result
        }
        let all = permutations(&CLASSES);
        // Pass 1: a single unconditional (permutation, key).
        let mut best: (usize, String) = (0, String::new());
        for permutation in &all {
            for &key in &KEYS {
                let score = FIXTURES.iter().filter(|f| matches(f, permutation, key)).count();
                if score > best.0 {
                    best = (score, format!("{permutation:?} {key:?}"));
                }
            }
        }
        println!("UNCONDITIONAL best: {}/{} via {}", best.0, FIXTURES.len(), best.1);
        // Pass 2: conditional on "any r3+ value crosses the first branch"
        // — approximated as: the fixture contains a Shift or LoadSurviving
        // value (D4/V1d are the two without... V1d has LoadSurviving; use
        // the discarded-only test: every load discarded AND no shift).
        let crosserless: Vec<bool> = FIXTURES
            .iter()
            .map(|f| {
                f.values.iter().all(|v| {
                    !matches!(v.class, Class::LoadSurviving | Class::Shift)
                        || v.class == Class::LoadSurviving && false
                }) || f
                    .values
                    .iter()
                    .filter(|v| matches!(v.class, Class::LoadSurviving | Class::Shift))
                    .count()
                    == 0
            })
            .collect();
        let mut winners: Vec<String> = Vec::new();
        for with in &all {
            for &key_with in &KEYS {
                // The with-crossers rule must fit all crosser fixtures.
                if !FIXTURES
                    .iter()
                    .zip(&crosserless)
                    .filter(|(_, &less)| !less)
                    .all(|(f, _)| matches(f, with, key_with))
                {
                    continue;
                }
                for without in &all {
                    for &key_without in &KEYS {
                        if FIXTURES
                            .iter()
                            .zip(&crosserless)
                            .filter(|(_, &less)| less)
                            .all(|(f, _)| matches(f, without, key_without))
                        {
                            winners.push(format!(
                                "crossers: {with:?} {key_with:?} | none: {without:?} {key_without:?}"
                            ));
                        }
                    }
                }
            }
        }
        println!("CONDITIONAL winners: {}", winners.len());
        for winner in winners.iter().take(20) {
            println!("  {winner}");
        }
    }

    /// PINNED: the v2 int-allocator model reproduces all thirteen captured
    /// register maps (docs/int-allocator-frontier.md).
    #[test]
    fn int_allocator_model_v2() {
        use int_alloc_fit::*;
        for fixture in FIXTURES {
            let order = model_order(fixture);
            let chosen = assign(&order, fixture.values);
            for (index, value) in fixture.values.iter().enumerate() {
                assert_eq!(
                    chosen[index], value.expected,
                    "{}: value {} got r{}, expected r{}",
                    fixture.name, index, chosen[index], value.expected
                );
            }
        }
    }

    #[test]
    fn legacy_port_model_pairs_distinct_units() {
        use OpKind::Store as St;
        let nodes = vec![
            DagNode::new("mulli", MUL).gate(2).hazard(HAZARD_MUL).reads(&[1]).writes(&[10]),
            DagNode::new("store", STORE).kind(St).reads(&[10]),
            DagNode::new("mask", ALU).reads(&[2]).writes(&[20]).after(0),
            DagNode::new("ori", ALU).reads(&[20]).writes(&[21]),
        ];
        let order: Vec<&str> = linearize_with(&nodes, LEGACY_PORT_AWARE)
            .into_iter()
            .map(|node| nodes[node].label)
            .collect();
        assert_eq!(order, ["mulli", "mask", "store", "ori"]);
    }

    #[test]
    fn legacy_allocator_reuses_forbidden_dying_input_in_return_mode() {
        use OpKind::Store as St;
        let nodes = vec![
            DagNode::new("mulli", MUL)
                .gate(2)
                .hazard(HAZARD_MUL)
                .reads(&[1])
                .writes(&[10])
                .forbid_r0(),
            DagNode::new("addi", ALU).reads(&[10]).writes(&[11]),
            DagNode::new("store", STORE).kind(St).reads(&[11]),
            DagNode::new("mask", ALU).reads(&[2]).writes(&[20]),
            DagNode::new("ori", ALU).reads(&[20]).writes(&[21]),
        ];
        let registers = assign_registers_legacy(
            &nodes,
            &[0, 1, 3, 2, 4],
            &[(1, 3), (2, 4)],
        );
        assert_eq!(registers, [Some(3), Some(3), None, Some(0), Some(3)]);
    }

    #[test]
    fn legacy_shared_float_rotates_three_product_homes() {
        use OpKind::Store as St;
        let nodes = vec![
            DagNode::new("z", ALU).local_home().reads(&[1]).writes(&[10]),
            DagNode::new("v", ALU).local_home().reads(&[10, 1]).writes(&[11]),
            DagNode::new("w", ALU).local_home().reads(&[11, 10]).writes(&[12]),
            DagNode::new("sink", STORE).kind(St).reads(&[10, 11, 12]),
        ];
        let mut model = FROZEN_FLOAT_REG;
        model.tier_position_desc = true;
        model.legacy_three_tier_rotation = true;
        model.window_floor = 6;
        model.void_forward = false;
        assert_eq!(
            assign_float_registers(&nodes, &[0, 1, 2, 3], &[(1, 1)], model),
            [Some(4), Some(3), Some(5), None]
        );
    }

    #[test]
    fn legacy_deep_inframe_prefix_keeps_deferred_local_in_tier() {
        use OpKind::Store as St;
        let nodes = vec![
            DagNode::new("z", ALU).local_home().reads(&[1]).writes(&[10]),
            DagNode::new("coefficient", LOAD).writes(&[11]),
            DagNode::new("fshared", ALU).reads(&[10, 11]).writes(&[12]),
            DagNode::new("v", ALU).local_home().reads(&[10, 1]).writes(&[13]),
            DagNode::new("sink", STORE).kind(St).reads(&[12, 13]),
        ];
        let order = [0, 1, 2, 3, 4];
        let mut modern = FROZEN_FLOAT_REG;
        modern.tier_position_desc = true;
        modern.window_floor = 4;
        modern.void_forward = false;
        let modern_registers = assign_float_registers(&nodes, &order, &[(1, 1)], modern);

        let mut legacy = modern;
        legacy.legacy_inframe_full_tier = true;
        let legacy_registers = assign_float_registers(&nodes, &order, &[(1, 1)], legacy);

        assert_eq!(modern_registers[0], Some(3));
        assert_eq!(legacy_registers[0], Some(2));
        assert_eq!(legacy_registers[3], Some(3));
    }
