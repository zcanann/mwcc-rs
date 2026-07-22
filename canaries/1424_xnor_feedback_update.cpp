// builds: GC/1.2.5n
// flags: -O4,s -inline off -Cpp_exceptions off -pragma "cats off" -func_align 32 -use_lmw_stmw on

typedef unsigned int u32;

extern u32 produce();
extern u32 state;

static void step()
{
    u32 word;
    word = produce();
    state = word | ((~(word ^ (word << 7) ^ (word << 15) ^ (word << 23)) >> 31) & 1);
}
