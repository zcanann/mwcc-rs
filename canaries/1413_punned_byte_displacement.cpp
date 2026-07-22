// builds: GC/1.2.5n GC/1.3.2 GC/2.6
// flags: -O4,s -inline off -Cpp_exceptions off -pragma "cats off" -func_align 32

typedef unsigned char u8;
typedef unsigned int u32;

static u32 load_punned_byte_displacement(u8* bytes)
{
    return *(u32*)(bytes + 4);
}

static void store_punned_byte_displacement(u8* bytes, u32 value)
{
    *(u32*)(bytes + 12) = value;
}
