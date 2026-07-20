// builds: 1.3 1.3.2 2.0 2.0p1 2.5 2.6 2.7
// flags: -Cpp_exceptions off -use_lmw_stmw on -inline deferred -str reuse,pool,readonly

typedef long long OSTime;
typedef unsigned int u32;

extern u32 __OSBusClock : 0x800000F8;
extern "C" OSTime OSGetTime();

class TimerData {
public:
    void Wait(float) const;
};

void TimerData::Wait(float seconds) const {
    OSTime duration = seconds * (__OSBusClock / 4);
    OSTime end = OSGetTime() + duration;
    volatile OSTime current;
    volatile int difference;
    do {
        current = OSGetTime();
        difference = current - end;
    } while (difference < 0);
}
