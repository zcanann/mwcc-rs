// builds: GC/1.2.5n GC/1.3.2 GC/2.6
// flags: -O4,s -inline off -Cpp_exceptions off -pragma "cats off" -func_align 32

typedef unsigned int u32;

extern u32 get_tick(void);
extern void seed_random(u32 seed);
extern int next_random(void);

static u32 call_live_counter_loop(void)
{
    u32 shift = 1;
    u32 iteration = 0;

    seed_random(get_tick());
    int result = (next_random() & 0x1f) + 1;
    for (; result < 4 && iteration < 10; iteration++) {
        result = get_tick() << shift;
        if (++shift > 0x10)
            shift = 1;
        seed_random(result);
        result = (next_random() & 0x1f) + 1;
    }
    return result < 4 ? 4 : result;
}
