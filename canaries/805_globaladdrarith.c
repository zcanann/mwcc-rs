// `&global +/- n` is pointer arithmetic: materialize the address, then add the offset
// scaled by the pointee size (`&ga + 1` is `+4`). The address goes in the destination in
// place when it is a real register (return / call arg `addi r3,r3,k`), else a fresh
// register (`li r3,0; addi r0,r3,k` for a store). As a lone call argument the leading
// `li` still hoists into the mflr->save gap even though the offset `addi` follows it.
int ga;
char gc;
int *gpi;
char *gpc;
void h(int *);
void store_off(void) { gpi = &ga + 1; }   // li r3,0; addi r0,r3,4; stw r0,gpi
int *ret_off(void)   { return &ga + 3; }   // li r3,0; addi r3,r3,12
void store_back(void){ gpi = &ga - 1; }    // addi r0,r3,-4
void store_char(void){ gpc = &gc + 5; }    // char stride 1: addi r0,r3,5
void pass_off(void)  { h(&ga + 2); }       // li r3,0 (hoisted); stw LR; addi r3,r3,8; bl
