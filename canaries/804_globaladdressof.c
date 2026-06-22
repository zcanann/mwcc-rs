// `&global` materializes the global's address. Under small-data it is the addi
// counterpart of the SDA value load — `addi d,r13,ga@sda21`, an EMB_SDA21 relocation,
// rendered `li d,0` pre-link. Ours deferred it (address-of only handled frame-resident
// locals). The fix also stops `&global` from forcing a stack frame: collect_address_taken
// now keeps only parameters/locals, so a leaf `&global` function stays frameless
// (`li r0,0; stw r0,gp; blr`) rather than gaining a spurious extab frame.
int ga;
char gc;
int *gpi;
char *gpc;
void h(int *);
void store_addr(void) { gpi = &ga; }      // li r0,0 (sda21); stw r0,gpi
int *ret_addr(void)   { return &ga; }      // li r3,0 (sda21)
void store_char(void) { gpc = &gc; }       // &gc — a byte global
void pass_addr(void)  { h(&ga); }          // bl h with &ga in r3
