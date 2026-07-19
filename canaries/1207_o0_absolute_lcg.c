// flags: -O0,p -sdata 0 -sdata2 0 -pool off

typedef unsigned long u32;
typedef signed long s32;

u32 rng_state;

void seed_rng(u32 seed) {
    rng_state = seed;
}

s32 next_rng(void) {
    return (rng_state = (rng_state * 0x41C64E6D) + 0x3039) / 65536 & 0x7FFF;
}
