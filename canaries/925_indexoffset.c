// `base[index +/- const]` — a computed-offset subscript, not a bare `base[index]` — is now byte-exact
// for both load and store. mwcc scales the VARIABLE part, adds it to the base, and folds the CONSTANT
// into the load/store displacement: `slwi r0,i,k; add base,base,r0; lwz/stw d,off(base)`. (A bare
// variable index uses `lwzx`/`stwx`, which has no displacement field for the constant; a fully
// constant index folds entirely into the displacement off the original base.) The element width
// follows the pointee.
//
// `a[i * const]` folds the constant into the element scale: a power-of-two total scale uses `slwi`,
// otherwise `mulli`, then the bare `lwzx` (`a[i*2]` of int is `slwi r0,i,3`; `a[i*3]` is `mulli r0,i,12`).
//
// DEFERS rather than emit wrong bytes: a computed store VALUE (`a[i+1] = v+1`), multiple subscripts of
// one base (`p[i] + p[i+1]`, which would need the base preserved across the mutating `add`), a
// variable+variable index (`a[i+j]` / `a[i*j]`), and a left-constant add (`a[const + i]`, not commuted).
int   load_next (int *a, int i)         { return a[i + 1]; }  // slwi r0,r4,2; add r3,r3,r0; lwz r3,4(r3)
int   load_prev (int *a, int i)         { return a[i - 1]; }  // lwz r3,-4(r3)
int   load_far  (int *a, int i)         { return a[i + 5]; }  // lwz r3,20(r3)
short load_short(short *a, int i)       { return a[i + 1]; }  // slwi r0,r4,1; add; lha r3,2(r3)
char  load_char (char *a, int i)        { return a[i + 1]; }  // add r3,r3,r4; lbz r3,1(r3)
void  store_next(int *a, int i, int v)  { a[i + 1] = v; }     // slwi; add; stw r5,4(r3)
void  store_prev(int *a, int i, int v)  { a[i - 1] = v; }     // stw r5,-4(r3)
int   load_x2   (int *a, int i)         { return a[i * 2]; }  // slwi r0,r4,3; lwzx r3,r3,r0
int   load_x3   (int *a, int i)         { return a[i * 3]; }  // mulli r0,r4,12; lwzx r3,r3,r0
