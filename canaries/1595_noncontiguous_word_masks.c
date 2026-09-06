// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned int u32;
u32 narrow(u32 value) { return value & 0x405; }
u32 upper(u32 value) { return value & 0xA0050000; }
u32 wide(u32 value) { return value & 0xA0050405; }
u32 load(volatile u32* value) { return *value & 0x405; }
u32 wide_carry(u32 value) { return value & 0x7FFF8005; }
u32 high_bit(u32 value) { return value & 0x80058005; }
