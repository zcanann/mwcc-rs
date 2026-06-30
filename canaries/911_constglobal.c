// A `const` scalar-int file-scope global is a compile-time constant: mwcc INLINES its value at each
// read (`return K` -> `li r3,VALUE`; `K+K` -> li 10; `a+K` -> addi a,VALUE) while STILL emitting K's
// read-only `.sdata2` storage (a named symbol). The parser now folds const Int/UnsignedInt global
// reads like an enum constant (the global is still pushed so the writer emits the storage). const
// char/short need sign-extension care (deferred); `&g` folds to AddressOf{literal} and defers (safe).
const int      K = 5;
const int      N = 3;
const unsigned MASK = 0xff;
const int      A = 10, B = 7;
int      read_k(void)         { return K; }         // li r3,5  (+ K in .sdata2)
int      sum_same(void)       { return K + K; }     // li r3,10
int      add_param(int a)     { return a + K; }     // addi r3,r3,5
int      two_consts(void)     { return A + B; }     // li r3,17
unsigned masked(unsigned a)   { return a & MASK; }  // rlwinm
int      indexed(int *p)      { return p[N]; }      // lwz r3,12(r3)
