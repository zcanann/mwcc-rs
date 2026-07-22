// builds: GC/1.2.5n GC/1.3.2 GC/2.6
// flags: -O4,s -inline off -Cpp_exceptions off -pragma "cats off" -func_align 32

typedef unsigned char u8;
typedef unsigned int u32;

static u8* in_place_pointer_round_up(u8* input)
{
    input = (u8*)(((u32)input + 31) & -32);
    return input;
}
