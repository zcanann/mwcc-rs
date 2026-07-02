// A guarded computed-index global-array store with a constant return -- the full
// signal.c store shape. The address build interleaves with the live return value:
//   const value, offset 0:   `lis r4; slwi r0,i; addi r3,r4; li r4,C; stwx r4,r3,r0; li r3,R`
//   const value, offset -1:  `lis r4; slwi; addi r3,r4; li r5,C; add r4,r3,r0; li r3,R; stw r5,-4(r4)`
//   register value:          `lis r5; slwi; addi r5,r5; li r3,R; stwx v,r5,r0`
// (The register-value form previously MISCOMPILED: the sequential path put the base in
// the index register and the return after the store.)
int arr[6];

int guarded_offset(int i)         { if (i < 1) return -1; arr[i - 1] = 0; return 0; }
int guarded_zero(int i)           { if (i < 1) return -1; arr[i] = 0; return 0; }
int guarded_register(int i, int v){ if (i < 1) return -1; arr[i] = v; return 0; }
int guarded_pos_offset(int i)     { if (i < 1) return -1; arr[i + 2] = 9; return 0; }
void void_guarded(int i)          { if (i < 1) return; arr[i - 1] = 0; }
