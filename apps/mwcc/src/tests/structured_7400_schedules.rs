use crate::{compile, SourceLanguage};

const SOURCE: &[u8] = br#"
    typedef unsigned char u8;
    typedef unsigned int u32;
    typedef int BOOL;
    typedef struct Header { u8 buffer[1]; } Header;
    typedef struct WorkArea { Header header; } WorkArea;
    typedef struct Card {
        u8 pad0[20];
        int latency;
        u8 pad1[104];
        WorkArea* workArea;
    } Card;

    extern Card cards[2];
    extern void OSPanic(const char*, int, const char*, ...);
    extern int EXISelect(int, int, int);
    extern int EXIImmEx(int, void*, int, int);
    extern int EXIDeselect(int);
    extern void* memset(void*, int, unsigned int);

    static int transfer(int chan, u32 data, void* output, int length, BOOL mode) {
        (void)((0 <= chan && chan < 2)
            || (OSPanic("dsp_cardunlock.c", 216, "0 <= chan && chan < 2"), 0));
        Card* card = &cards[chan];
        if (!EXISelect(chan, 0, 4))
            return -3;

        data &= 0xfffff000;
        u8 command[5];
        memset(command, 0, sizeof(command));
        command[0] = 0x52;
        if (mode == 0) {
            command[1] = data >> 29 & 3;
            command[2] = data >> 21 & 0xff;
            command[3] = data >> 19 & 3;
            command[4] = data >> 12 & 0x7f;
        } else {
            command[1] = data >> 24 & 0xff;
            command[2] = data >> 16 & 0xff;
        }

        BOOL error = 0;
        error |= !EXIImmEx(chan, command, sizeof(command), 1);
        error |= !EXIImmEx(chan, card->workArea->header.buffer, card->latency, 1);
        error |= !EXIImmEx(chan, output, length, 0);
        error |= !EXIDeselect(chan);
        return error ? -3 : 0;
    }
"#;

fn compile_7400() -> Vec<u8> {
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    flags.inline_enabled = false;
    flags.use_lmw_stmw = true;
    flags.optimization_goal = mwcc_versions::OptimizationGoal::Size;
    flags.scheduling_model = mwcc_versions::SchedulingModel::PowerPc7400;
    compile(
        SOURCE,
        "structured-7400-schedules.cpp",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_2_5N,
            flags,
        },
        Some(SourceLanguage::Cxx),
        None,
        false,
    )
    .expect("the structured transfer function should compile")
}

#[test]
fn schedules_dense_structured_regions_for_power_pc_7400() {
    let object = compile_7400();
    let assertion_call = [
        0x3c, 0x60, 0x00, 0x00, // lis r3,file@ha
        0x3c, 0x80, 0x00, 0x00, // lis r4,assertion@ha
        0x38, 0xa4, 0x00, 0x00, // addi r5,r4,assertion@l
        0x38, 0x63, 0x00, 0x00, // addi r3,r3,file@l
        0x38, 0x80, 0x00, 0xd8, // li r4,216
        0x4c, 0xc6, 0x31, 0x82, // crclr 6
        0x48, 0x00, 0x00, 0x01, // bl OSPanic
    ];
    let four_byte_fanout = [
        0x57, 0xa0, 0x1f, 0xbe, // first field -> r0
        0x57, 0xa4, 0x5e, 0x3e, // second field -> r4
        0x98, 0x01, 0x00, 0x21, // store first field
        0x57, 0xa3, 0x6f, 0xbe, // third field -> r3
        0x57, 0xa0, 0xa6, 0x7e, // fourth field -> r0
        0x98, 0x81, 0x00, 0x22, // store second field
        0x98, 0x61, 0x00, 0x23, // store third field
        0x98, 0x01, 0x00, 0x24, // store fourth field
    ];
    let accumulated_call = [
        0x7c, 0x60, 0x00, 0x34, // cntlzw r0,r3
        0x38, 0x7c, 0x00, 0x00, // addi r3,r28,0
        0x54, 0x00, 0xde, 0x3e, // normalize the prior result
    ];

    for expected in [&assertion_call[..], &four_byte_fanout, &accumulated_call] {
        assert!(object.windows(expected.len()).any(|code| code == expected));
    }
}
