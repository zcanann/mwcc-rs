// builds: GC/1.2.5n GC/1.3.2 GC/2.6
// flags: -O4,s -inline off -Cpp_exceptions off -pragma "cats off" -func_align 32

struct Block {
    unsigned char bytes[132];
};

extern Block blocks[2];

static Block* struct_array_element_address(int index)
{
    return &blocks[index];
}
