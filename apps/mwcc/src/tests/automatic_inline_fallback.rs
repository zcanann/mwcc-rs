use super::elf_object::symbols;
use crate::{compile, SourceLanguage};

#[test]
fn retains_an_ordinary_call_when_its_automatic_inline_topology_declines() {
    let source = br#"
        float blend(float weight, float left, float middle, float right) {
            float inverse;
            float result;
            inverse = 1.0f - weight;
            result = (right * (weight * weight))
                + ((left * (inverse * inverse))
                + (2.0f * (middle * (inverse * weight))));
            return result;
        }

        void apply(float* left, float* middle, float* right, float* output,
                   float weight) {
            int index;
            for (index = 0; index < 3; index++) {
                *output = blend(weight, *left++, *middle++, *right++);
                output++;
            }
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.optimization = mwcc_versions::Optimization::O0;
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.emit_mwcats = false;
    let object = compile(
        source,
        "automatic-inline-fallback.c",
        mwcc_versions::CompilerConfig {
            build: mwcc_versions::GC_1_3_2,
            flags,
        },
        Some(SourceLanguage::C),
        None,
        false,
    )
    .expect("the unexpanded ordinary call should remain a valid fallback");

    let emitted = symbols(&object)
        .into_iter()
        .map(|(name, _, _, _)| name)
        .collect::<Vec<_>>();
    assert!(emitted.iter().any(|name| name == "blend"));
    assert!(emitted.iter().any(|name| name == "apply"));
}
