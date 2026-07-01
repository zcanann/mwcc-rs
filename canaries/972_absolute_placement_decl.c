// MWERKS absolute-placement: `T name[dims] : <address>;` binds a name to a FIXED
// address (memory-mapped hardware registers — dolphin/hw_regs.h's
// `volatile u16 __VIRegs[59] : 0xCC002000;`). mwcc emits NO symbol or data for it
// (references resolve to the absolute address); we previously emitted it as a `.bss`
// tentative definition, a whole-object DIFF for every dolphin.h-including TU (the
// ssbm PPCPm.c real-file DIFF: 7 such __*Regs arrays = 1606 bytes of spurious .bss).
// The declaration is skipped entirely — this object has only `.text` for `anchor`.
volatile unsigned short __VIRegs[59] : 0xCC002000;
volatile unsigned long  __PIRegs[12] : 0xCC003000;
volatile unsigned long  __MEMRegs[64] : 0xCC004000;

int anchor(void) { return 0; }
