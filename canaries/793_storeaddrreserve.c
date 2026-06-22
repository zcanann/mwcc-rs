// A store through a pointer/member whose value needs a temporary register (a magic-
// number divide) was a miscompile: the temp picked the address register itself
// (`*p = a/3` chose r3 = p for the magic constant, clobbering p), then stored to the
// wrong place. The store address must be reserved while the value is computed, as mwcc
// does — it picks a higher free register (r5) for the magic.
void dp(int *p, int a)             { *p = a / 3; }     // p in r3 must survive
void dp7(int *p, int a)            { *p = a / 7; }
struct S { int x; };
void dm(struct S *p, int a)        { p->x = a / 3; }
void di(int *p, int a)             { p[2] = a / 5; }
