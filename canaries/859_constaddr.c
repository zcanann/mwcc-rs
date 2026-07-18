// `*(T *)0xADDR` — a constant-address load/store (memory-mapped hardware registers; the entry
// shape behind the GX write-gather FIFO). The 32-bit address splits into a sign-adjusted high
// half (`lis`) and a low displacement: `0xCC008000` -> `lis -13311` + displacement `-32768`.
// When the address fits the signed 16-bit displacement (high half zero), mwcc loads/stores off
// the r0=0 base with no `lis`. The store materializes the base before the value (kept clear of
// the value's registers), mirroring the absolute global store. Float/double pointees and the
// A value loaded into scratch r0 uses a separate lowest-free GPR for the base,
// because r0 in a D-form load's address field means literal zero.
void st_u8 (unsigned char  v) { *(volatile unsigned char  *)0xCC008000 = v; }  // lis r4; stb v,lo(r4)
void st_u16(unsigned short v) { *(volatile unsigned short *)0xCC008000 = v; }  // lis r4; sth v,lo(r4)
void st_u32(unsigned int   v) { *(volatile unsigned int   *)0xCC008000 = v; }  // lis r4; stw v,lo(r4)
int  ld_int(void)             { return *(volatile int *)0x80000034; }          // lis r3; lwz r3,lo(r3)
int  ld_small(void)           { return *(int *)0x1234; }                       // lwz r3,4660(0) (hi==0)
void st_small(int v)          { *(int *)0x00007000 = v; }                      // stw v,28672(0)
