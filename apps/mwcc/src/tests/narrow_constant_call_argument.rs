use crate::{compile, SourceLanguage};

#[test]
fn converts_constant_arguments_before_callback_address_scheduling() {
    for build in [
        mwcc_versions::GC_1_2_5N,
        mwcc_versions::GC_1_3_2,
        mwcc_versions::GC_3_0A3,
    ] {
        // These are the ABI constants emitted by the reference compilers for
        // out-of-range arguments, including signed and unsigned byte formals.
        for (parameter, value, immediate) in [
            ("signed char", "255", -1i16),
            ("unsigned char", "-1", 255),
            ("short", "0x10003", 3),
        ] {
            let source = format!(
                "void consume({parameter}, void (*)(void)); void handler(void); \
                 void invoke(void) {{ consume({value}, handler); }}"
            );
            let mut flags = mwcc_versions::Flags::default();
            flags.debug_info = false;
            flags.cpp_exceptions = false;
            flags.emit_mwcats = false;
            let object = compile(
                source.as_bytes(),
                "narrow-constant-call-argument.c",
                mwcc_versions::CompilerConfig { build, flags },
                Some(SourceLanguage::C),
                None,
                false,
            )
            .expect("constant prototype conversions must lower before scheduling");
            let load = (0x3860_0000u32 | u32::from(immediate as u16)).to_be_bytes();
            assert!(
                object.windows(4).any(|bytes| bytes == load),
                "{parameter} argument {value} must materialize ABI value {immediate}"
            );
        }
    }
}
