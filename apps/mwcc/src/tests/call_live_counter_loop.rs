use crate::{compile, SourceLanguage};

const SOURCE: &[u8] = br#"
    typedef unsigned int u32;
    extern u32 OSGetTick(void);
    extern void srand(u32);
    extern int rand(void);

    static u32 DummyLen(void) {
        u32 lshift = 1;
        u32 i = 0;
        srand(OSGetTick());
        int result = (rand() & 0x1f) + 1;
        for (; result < 4 && i < 10; i++) {
            result = OSGetTick() << lshift;
            if (++lshift > 0x10)
                lshift = 1;
            srand(result);
            result = (rand() & 0x1f) + 1;
        }
        return result < 4 ? 4 : result;
    }
"#;

fn compile_with(model: mwcc_versions::SchedulingModel, scheduler_enabled: bool) -> Vec<u8> {
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    flags.use_lmw_stmw = true;
    flags.scheduling_model = model;
    flags.scheduler_enabled = scheduler_enabled;
    compile(
        SOURCE,
        "call-live-counter-loop.cpp",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        },
        Some(SourceLanguage::Cxx),
        None,
        false,
    )
    .expect("the call-live counter loop should compile")
}

#[test]
fn scheduling_7400_fills_the_mask_latency_with_the_loop_counter() {
    let ordinary = compile_with(mwcc_versions::SchedulingModel::Default, true);
    let power_pc_7400 = compile_with(mwcc_versions::SchedulingModel::PowerPc7400, true);
    let scheduling_disabled = compile_with(mwcc_versions::SchedulingModel::PowerPc7400, false);

    let ordinary_order = [
        0x54, 0x63, 0x06, 0xfe, // clrlwi r3,r3,27
        0x38, 0x63, 0x00, 0x01, // addi r3,r3,1
        0x3b, 0xde, 0x00, 0x01, // addi r30,r30,1
    ];
    let power_pc_7400_order = [
        0x54, 0x63, 0x06, 0xfe, // clrlwi r3,r3,27
        0x3b, 0xde, 0x00, 0x01, // addi r30,r30,1
        0x38, 0x63, 0x00, 0x01, // addi r3,r3,1
    ];

    assert!(ordinary
        .windows(ordinary_order.len())
        .any(|code| code == ordinary_order));
    assert!(scheduling_disabled
        .windows(ordinary_order.len())
        .any(|code| code == ordinary_order));
    assert!(power_pc_7400
        .windows(power_pc_7400_order.len())
        .any(|code| code == power_pc_7400_order));
}
