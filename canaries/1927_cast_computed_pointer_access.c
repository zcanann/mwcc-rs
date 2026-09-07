// flags: -Cpp_exceptions off -pragma "cats off"
// GX register macros cast a computed address again before accessing it.
extern volatile unsigned short *regs;
void global_store(unsigned v) { *(volatile unsigned short *)((volatile unsigned short *)regs + 3) = v; }
unsigned global_load(void) { return *(volatile unsigned short *)((volatile unsigned short *)regs + 3); }
void local_store(unsigned short *p, unsigned i, unsigned v) { *(unsigned short *)((unsigned short *)p + i) = v; }
unsigned local_load(unsigned short *p, unsigned i) { return *(unsigned short *)((unsigned short *)p + i); }
unsigned different_width(unsigned *p, unsigned i) { return *(unsigned short *)((unsigned *)p + i); }
void negative_offset(unsigned *p, unsigned v) { *(unsigned short *)((unsigned *)p - 3) = v; }
unsigned byte_offset(unsigned char *p, unsigned i) { return *(unsigned short *)((unsigned char *)p + i); }
unsigned index_reused(unsigned short *p, unsigned i) { return *(unsigned short *)((unsigned short *)p + i) + i; }
unsigned constant_width(unsigned *p) { return *(unsigned short *)((unsigned *)p + 3); }
unsigned negative_load(unsigned *p) { return *(unsigned short *)((unsigned *)p - 3); }
unsigned constant_byte(unsigned char *p) { return *(unsigned short *)(p + 6); }
unsigned cast_change(unsigned *p) { return *(unsigned short *)((unsigned char *)p + 6); }
