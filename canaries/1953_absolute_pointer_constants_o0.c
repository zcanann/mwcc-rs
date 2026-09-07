// flags: -Cpp_exceptions off -pragma "cats off"
extern void observe(unsigned);
unsigned physical_address(void) { return (unsigned)((unsigned char*)(int*)0xcc008000 - 0xc0000000); }
void physical_argument(void) { observe((unsigned)((unsigned char*)(int*)0xcc008000 - 0xc0000000)); }
unsigned int_stride(void) { return (unsigned)((int*)0x12345678 + 3); }
unsigned short_stride(void) { return (unsigned)((unsigned short*)0x12345678 - 3); }
unsigned byte_stride(void) { return (unsigned)((unsigned char*)0x12345678 + 3); }
unsigned recast_stride(void) { return (unsigned)((unsigned char*)((int*)0x12345678 + 3) - 5); }
unsigned integer_stride(void) { return (unsigned)(int*)0x12345678 + 3; }
unsigned reverse_stride(void) { return (unsigned)(3 + (int*)0x12345678); }
unsigned wrap_stride(void) { return (unsigned)((int*)0xfffffff8 + 3); }
int pointer_difference(void) { return (int*)0x12345670 - (int*)0x12345678; }
struct Triple { unsigned a, b, c; };
unsigned struct_stride(void) { return (unsigned)((struct Triple*)0x12345678 + 3); }
unsigned narrow_address(void) { return (unsigned short)((int*)0x12345678 + 3); }
unsigned null_stride(void) { return (unsigned)((int*)0 + 3); }
