// builds: GC/1.2.5n GC/1.3.2 GC/2.6
// flags: -O4,s -inline off -Cpp_exceptions off -pragma "cats off" -func_align 32

typedef unsigned int u32;

extern u32 get_tick(void);
extern void seed_random(u32 seed);
extern int next_random(void);

static u32 call_reassigned_local_mask(void)
{
    seed_random(get_tick());
    u32 value = 0x7fec8000;
    value |= next_random();
    value &= 0xfffff000;
    return value;
}
