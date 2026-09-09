// flags: -O4,s
typedef unsigned int u32; typedef signed int s32; typedef unsigned short u16;
u32 rol7(u32 a) { return (a<<7)|(a>>25); }
u32 rol16(u32 a) { return (a<<16)|(a>>16); }
u32 ror13(u32 a) { return (a>>13)|(a<<19); }
u32 signed_pair(s32 a) { return (a<<7)|(a>>25); }
u32 packed(u16 a,u16 b) { return ((u32)a<<16)|b; }
u32 volatile_pair(volatile u16*p) { return ((u32)p[0]<<16)|p[1]; }
u32 fixed_mail(void) { u16 a=*(volatile u16*)0xcc005004; u16 b=*(volatile u16*)0xcc005006; return (a<<16)|b; }
u32 shift_sum(u32 a,u32 b) { return (a<<3)+(b>>5); }
u32 shift_product(u32 a,u32 b) { return (a<<3)*(b>>5); }
u32 shift_and(u32 a,u32 b) { return (a<<3)&(b>>5); }
u32 shift_xor(u32 a,u32 b) { return (a<<3)^(b>>5); }
u32 shift_or(u32 a,u32 b) { return (a<<3)|(b>>5); }
u32 memory_sum(u32*p) { return (p[0]<<3)+p[1]; }
u32 memory_pair(u32*p) { return (p[0]<<8)|(p[1]>>24); }
u32 masked_rotate(u32 a) { return ((a<<7)|(a>>25))&0xffff; }
u32 different_fields(u32 a,u32 b) { return (a<<16)|(b>>16); }
extern volatile u16 bank[32] : 0xcc005000;
void write_punned(u32 a) { *(volatile u32*)&bank[16]=a; }
u32 fixed_address(void) { return (u32)&bank[16]; }
