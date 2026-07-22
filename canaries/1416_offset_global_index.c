// builds: GC/1.2.5n GC/1.3.2 GC/2.6
// flags: -O4,p -inline off -Cpp_exceptions off -pragma "cats off" -func_align 32

typedef unsigned short u16;

u16 volume_table[1024];

static u16 lookup_volume(int index)
{
    if (index <= -904) {
        return 0;
    }
    if (60 <= index) {
        return 0xff64;
    }
    return volume_table[index + 904];
}
