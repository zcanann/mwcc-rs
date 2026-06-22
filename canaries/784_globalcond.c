// A global used as a condition (`if (gFlag) ...`) has no home register: load it into
// the scratch (`lwz r0,gFlag@sda21`) and compare, like a memory load. In a non-leaf
// function the load clobbers r0, so the LR store precedes it — and the load's SDA21
// relocation must shift with it (the earlier LR-store insert moved the instruction
// but not its reloc, which only a relocated condition like a global exposes).
int gFlag;
unsigned gMask;
void sink(void);
void on_flag(void)  { if (gFlag) sink(); }
void on_mask(void)  { if (gMask) sink(); }
void on_nflag(void) { if (!gFlag) sink(); }
