// Storing a member through a GLOBAL struct ARRAY (`arr[i].field = v`): the
// interleaved base/scale schedule (`lis hi,arr@ha; slwi r0,i,k; addi base,hi,
// arr@lo`) then `stwx` (offset 0) or `add; stw offset`. A register value keeps
// its register (and @ha avoids it); a constant value reuses @ha's now-free
// register. A constant index folds index*stride+offset into the displacement.
// This is the marioparty4 `GWPlayer[player].field = v` write shape.
struct Gams { int first; int second; };
struct Gams gams_table[16];
void gams_var(int i, int x)  { gams_table[i].second = x; }
void gams_const(int i)       { gams_table[i].second = 3; }
void gams_off0(int i, int x) { gams_table[i].first = x; }
void gams_cidx(int x)        { gams_table[2].second = x; }
