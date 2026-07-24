use crate::{compile, SourceLanguage};

const SOURCE: &[u8] = br#"
    typedef unsigned int u32;
    static u32 exnor_1st(u32 data, u32 count) {
        for (u32 i = 0; i < count; ++i) {
            data = (data >> 1)
                | (~(data ^ (data >> 7) ^ (data >> 15) ^ (data >> 23)) << 30)
                    & 0x40000000;
        }
        return data;
    }
"#;

fn compile_with(model: mwcc_versions::SchedulingModel) -> Vec<u8> {
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    flags.scheduling_model = model;
    compile(
        SOURCE,
        "xnor-feedback-loop.cpp",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        },
        Some(SourceLanguage::Cxx),
        None,
        false,
    )
    .expect("the XNOR feedback loop should compile")
}

#[test]
fn scheduling_7400_fills_both_xnor_feedback_latency_slots() {
    let ordinary = compile_with(mwcc_versions::SchedulingModel::Default);
    let power_pc_7400 = compile_with(mwcc_versions::SchedulingModel::PowerPc7400);

    let ordinary_entry = [
        0x28, 0x04, 0x00, 0x00, // cmplwi r4,0
        0x7c, 0x89, 0x03, 0xa6, // mtctr r4
        0x4c, 0x81, 0x00, 0x20, // blelr
    ];
    let power_pc_7400_entry = [
        0x7c, 0x89, 0x03, 0xa6, // mtctr r4
        0x28, 0x04, 0x00, 0x00, // cmplwi r4,0
        0x4c, 0x81, 0x00, 0x20, // blelr
    ];
    let ordinary_tail = [
        0x7c, 0x80, 0x02, 0x78, // xor r0,r4,r0
        0x7c, 0xa0, 0x02, 0x38, // eqv r0,r5,r0
        0x54, 0x63, 0xf8, 0x7e, // srwi r3,r3,1
    ];
    let power_pc_7400_tail = [
        0x7c, 0x80, 0x02, 0x78, // xor r0,r4,r0
        0x54, 0x63, 0xf8, 0x7e, // srwi r3,r3,1
        0x7c, 0xa0, 0x02, 0x38, // eqv r0,r5,r0
    ];

    for expected in [&ordinary_entry, &ordinary_tail] {
        assert!(ordinary
            .windows(expected.len())
            .any(|code| code == expected.as_slice()));
    }
    for expected in [&power_pc_7400_entry, &power_pc_7400_tail] {
        assert!(power_pc_7400
            .windows(expected.len())
            .any(|code| code == expected.as_slice()));
    }
}
