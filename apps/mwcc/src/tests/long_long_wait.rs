use crate::{compile, SourceLanguage};

#[test]
fn numbers_deferred_timer_wait_constants_like_legacy_mwcc() {
    let source = br#"
        typedef long long s64;
        typedef s64 OSTime;
        typedef unsigned int u32;
        extern "C" OSTime OSGetTime();
        extern "C" u32 __OSBusClock;

        class Timer {
        public:
            void Wait(float) const;
            bool Initialize();

        private:
            s64 frequency;
            s64 frequency_per_microsecond;
            float period;
        };

        void Timer::Wait(float seconds) const {
            OSTime duration = seconds * ((u32)__OSBusClock / 4);
            OSTime end = OSGetTime() + duration;
            volatile OSTime current;
            volatile int difference;
            do {
                current = OSGetTime();
                difference = current - end;
            } while (difference < 0);
        }

        bool Timer::Initialize() {
            frequency = ((u32)__OSBusClock / 4);
            frequency_per_microsecond = frequency / 1000000ll;
            period = 1.f / static_cast<float>(frequency);
            return true;
        }
    "#;
    let mut flags = mwcc_versions::Flags::default();
    flags.debug_info = false;
    flags.cpp_exceptions = false;
    flags.rtti = false;
    flags.inline_deferred = true;
    let config = mwcc_versions::CompilerConfig {
        build: mwcc_versions::GC_1_3_2,
        flags,
    };
    let object = compile(
        source,
        "timer.cpp",
        config,
        Some(SourceLanguage::Cxx),
        None,
        false,
    )
    .expect("the legacy timer wait shape should compile");

    assert!(object.windows(4).any(|bytes| bytes == b"@12\0"));
    assert!(object.windows(4).any(|bytes| bytes == b"@21\0"));
    assert!(!object.windows(4).any(|bytes| bytes == b"@14\0"));
    assert!(!object.windows(4).any(|bytes| bytes == b"@23\0"));
}
