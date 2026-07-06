// A DOUBLE dereference through a word-pointer-to-pointer (`int **`, `unsigned **`) is a chained word
// load: `**pp` -> `lwz rN,0(rN); lwz ...,0(rN)`, the inner `*pp` loading the inner pointer value into
// the leaf's own register (as mwcc reuses it). The parser records `Pointee::WordPointer` for an
// int/unsigned inner (whose second deref is a plain `lwz`); a `char **`/`float **` inner keeps the
// opaque `Pointee::Pointer` and still defers, since its `**pp` would need `lbz`/`lfs`. Covers the
// return, arithmetic-subexpression (second load into r0), and store forms. (fire 606)
int      dd_int(int **pp)             { return **pp; }        // lwz r3,0(r3); lwz r3,0(r3)
unsigned dd_uns(unsigned **pp)        { return **pp; }        // lwz r3,0(r3); lwz r3,0(r3)
int      dd_add(int **pp, int y)      { return **pp + y; }    // lwz r3,0(r3); lwz r0,0(r3); add r3,r0,r4
void     dd_store(int **pp, int v)    { **pp = v; }           // lwz r3,0(r3); stw r4,0(r3)
