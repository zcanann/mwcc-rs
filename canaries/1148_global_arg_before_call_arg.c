// The first slice of the callee-saved argument scheduler: a call whose SECOND argument is an
// argument-free call and whose FIRST is a scalar GLOBAL — `h(gg, g())`. The global is reloadable (it
// lives in memory), so mwcc needs no callee-saved register: it evaluates the call FIRST (result in r3),
// copies it to the second argument register, then loads the global into r3:
//   bl g; mr r4,r3; lwz r3,gg; bl h
// Previously deferred ("a call in a non-first argument needs the callee-saved argument scheduler"). The
// param-first form (`h(a, g())` — a must survive the call in a callee-saved register) and a `&array`
// first argument (whose lis/addi address interleaves with the mr) still defer. (fire 637)
extern void hv(int, int);
extern int hi(int, int);
extern int g(void);
int gg;
void gac_void(void) { hv(gg, g()); }          // bl g; mr r4,r3; lwz r3,gg; bl hv
int gac_ret(void)   { return hi(gg, g()); }   // bl g; mr r4,r3; lwz r3,gg; bl hi
