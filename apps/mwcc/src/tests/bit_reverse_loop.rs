use crate::{compile, SourceLanguage};

const SOURCE: &[u8] = br#"
    typedef unsigned int u32;
    static u32 bitrev(u32 data) {
        u32 work;
        u32 index;
        u32 low_count = 0;
        u32 high_shift = 1;
        work = 0;
        for (index = 0; index < 32; ++index) {
            if (index > 15) {
                if (index == 31) {
                    work |= (data & 1 << 31) >> 31;
                } else {
                    work |= (data & 1 << index) >> high_shift;
                    high_shift += 2;
                }
            } else {
                work |= (data & 1 << index) << (31 - index - low_count);
                low_count += 1;
            }
        }
        return work;
    }
"#;

fn compile_with(model: mwcc_versions::SchedulingModel) -> Vec<u8> {
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    flags.optimization_goal = mwcc_versions::OptimizationGoal::Size;
    flags.scheduling_model = model;
    compile(
        SOURCE,
        "bit-reverse-loop.cpp",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        },
        Some(SourceLanguage::Cxx),
        None,
        false,
    )
    .expect("the bit-reversal loop should compile")
}

#[test]
fn scheduling_7400_interleaves_bit_reverse_scaffolding_and_counters() {
    let ordinary = compile_with(mwcc_versions::SchedulingModel::Default);
    let power_pc_7400 = compile_with(mwcc_versions::SchedulingModel::PowerPc7400);

    let ordinary_entry = [
        0x38, 0x00, 0x00, 0x20, // li r0,32
        0x7c, 0x09, 0x03, 0xa6, // mtctr r0
        0x54, 0x66, 0x0f, 0xfe, // srwi r6,r3,31
    ];
    let power_pc_7400_entry = [
        0x38, 0x00, 0x00, 0x20, // li r0,32
        0x54, 0x66, 0x0f, 0xfe, // srwi r6,r3,31
        0x39, 0x20, 0x00, 0x00, // li r9,0
        0x39, 0x40, 0x00, 0x01, // li r10,1
        0x38, 0xe0, 0x00, 0x00, // li r7,0
        0x39, 0x00, 0x00, 0x00, // li r8,0
        0x38, 0xa0, 0x00, 0x01, // li r5,1
        0x7c, 0x09, 0x03, 0xa6, // mtctr r0
    ];
    let ordinary_high = [
        0x7c, 0x00, 0x54, 0x30, // srw r0,r0,r10
        0x7c, 0xe7, 0x03, 0x78, // or r7,r7,r0
        0x39, 0x4a, 0x00, 0x02, // addi r10,r10,2
    ];
    let power_pc_7400_high = [
        0x7c, 0x00, 0x54, 0x30, // srw r0,r0,r10
        0x39, 0x4a, 0x00, 0x02, // addi r10,r10,2
        0x7c, 0xe7, 0x03, 0x78, // or r7,r7,r0
    ];
    let ordinary_low = [
        0x7c, 0x80, 0x00, 0x30, // slw r0,r4,r0
        0x7c, 0xe7, 0x03, 0x78, // or r7,r7,r0
        0x39, 0x29, 0x00, 0x01, // addi r9,r9,1
    ];
    let power_pc_7400_low = [
        0x7c, 0x80, 0x00, 0x30, // slw r0,r4,r0
        0x39, 0x29, 0x00, 0x01, // addi r9,r9,1
        0x7c, 0xe7, 0x03, 0x78, // or r7,r7,r0
    ];

    for expected in [&ordinary_entry, &ordinary_high, &ordinary_low] {
        assert!(ordinary
            .windows(expected.len())
            .any(|code| code == expected.as_slice()));
    }
    for expected in [
        power_pc_7400_entry.as_slice(),
        power_pc_7400_high.as_slice(),
        power_pc_7400_low.as_slice(),
    ] {
        assert!(power_pc_7400
            .windows(expected.len())
            .any(|code| code == expected));
    }
}
