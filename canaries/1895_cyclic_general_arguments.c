// GXFrameBuf-driven discarded values, escaped scalar operands, and argument dependencies.
// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned int u32;
extern u32 sink3(u32,u32,u32);
extern u32 sink4(u32,u32,u32,u32);
extern u32 sink8(u32,u32,u32,u32,u32,u32,u32,u32);
extern u32 sink_narrow(u32,unsigned short,unsigned short);
extern u32 sink_signed(int,signed char,short);
u32 right_cycle(u32 a,u32 b,u32 c) { return sink3(c,a,b); }
u32 left_cycle(u32 a,u32 b,u32 c) { return sink3(b,c,a); }
u32 swap_three(u32 a,u32 b,u32 c) { return sink3(b,a,c); }
u32 two_cycles(u32 a,u32 b,u32 c,u32 d) { return sink4(b,a,d,c); }
u32 narrow_cycle(unsigned short a,unsigned short b,u32 c) { return sink_narrow(c,a,b); }
u32 signed_cycle(signed char a,short b,int c) { return sink_signed(c,a,b); }
u32 eight_cycle(u32 a,u32 b,u32 c,u32 d,u32 e,u32 f,u32 g,u32 h) { return sink8(h,a,b,c,d,e,f,g); }
u32 repeated(u32 a,u32 b,u32 c) { return sink4(c,a,b,a); }
extern u32 sink_byte(unsigned char,u32);
extern u32 sink_char(signed char,u32);
u32 narrowed_alias(u32 a) { return sink_byte(a,a); }
u32 signed_alias(u32 a) { return sink_char(a,a); }
