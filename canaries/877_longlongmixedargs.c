// Mixed int/long-long parameter lists (EABI register-pair alignment). A long-long argument starts
// on an ODD GPR, so the allocator aligns up when the next register is even: `f(int x, long long a)`
// puts x in r3 and a in r5:r6 (skipping r4), while `f(long long a, int x)` puts a in r3:r4 and x in
// r5. The modeled return/truncate/add/subtract shapes all use this allocation, so returning the
// second-aligned pair emits `mr r4,r6; mr r3,r5`. Pointers count as one GPR like ints; a float/
// double/struct param alongside a long long, or an arg list overflowing r3..r10, defers.
long long retsecond_ix(int x, long long a)             { return a; }
long long retfirst_xi(long long a, int x)              { return a; }
int       trunc_ix(int x, long long a)                 { return (int)a; }
long long add_ixx(int x, long long a, long long b)     { return a + b; }
long long sub_xxi(long long a, long long b, int x)     { return a - b; }
long long retptr(int *p, long long a)                  { return a; }
