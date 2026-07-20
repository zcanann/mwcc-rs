// builds: 1.3 1.3.2 2.0 2.0p1 2.5 2.6 2.7
// flags: -Cpp_exceptions off -use_lmw_stmw on -inline deferred -str reuse,pool,readonly

typedef long long s64;
typedef unsigned int u32;

extern u32 __OSBusClock : 0x800000F8;

class Stopwatch {
public:
    class Data {
    public:
        Data() : timer_frequency(0), scaled_frequency(0), timer_period(0.0f) {}

        bool Initialize();

    private:
        s64 timer_frequency;
        s64 scaled_frequency;
        float timer_period;
    };
};

bool Stopwatch::Data::Initialize() {
    timer_frequency = __OSBusClock / 4;
    scaled_frequency = timer_frequency / 1000000ll;
    timer_period = 1.0f / static_cast<float>(timer_frequency);
    return true;
}
