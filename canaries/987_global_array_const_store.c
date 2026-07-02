// A CONSTANT stored at a COMPUTED index of a large (ADDR16) global array: the constant
// materializes into the freed base-high register after the `addi` --
// `lis r4,arr@ha; slwi r0,i,2; addi r3,r4,arr@lo; li r4,C; stwx r4,r3,r0`. An index
// with a constant offset (`arr[i-1] = 0`, the signal.c shape) adds the scaled index
// into the base and rides the element offset on the store displacement --
// `...; li r4,0; add r3,r3,r0; stw r4,-4(r3)`. Previously deferred.
int arr[6];
short sarr[16];

void store_zero(int i)      { arr[i] = 0; }
void store_offset_neg(int i){ arr[i - 1] = 0; }
void store_offset_pos(int i){ arr[i + 2] = 7; }
int store_then_return(int i){ arr[i] = 0; return 0; }
void store_short(int i)     { sarr[i] = 3; }
