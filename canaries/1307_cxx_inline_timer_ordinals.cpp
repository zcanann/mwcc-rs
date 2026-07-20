// builds: 1.3 1.3.2 2.0 2.0p1 2.5 2.6 2.7
// flags: -Cpp_exceptions off -inline deferred -str reuse,pool,readonly

typedef long long s64;
extern s64 timer_now();

class Timer {
public:
    class Data {
    public:
        Data() : frequency(0), scaled_frequency(0), period(0.0f) {}
        s64 GetFrequency() const { return frequency; }
        s64 GetScaledFrequency() const { return scaled_frequency; }
        float GetPeriod() const { return period; }
        s64 GetCycles() const { return timer_now(); }

    private:
        s64 frequency;
        s64 scaled_frequency;
        float period;
    };

    Timer() : start(data.GetCycles()) {}
    inline void Reset() {
        if (data.GetFrequency() == 0) {
            start = data.GetCycles();
        }
        start = data.GetCycles();
    }
    inline float Elapsed() const {
        return (data.GetCycles() - start) * data.GetPeriod();
    }
    inline s64 Micros() const {
        return (data.GetCycles() - start) / data.GetScaledFrequency();
    }
    static s64 GlobalMicros() { return global.Micros(); }

private:
    static Data data;
    static Timer global;
    s64 start;
};

float cxx_inline_timer_ordinal_probe() {
    return 1.0f;
}
