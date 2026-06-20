// A parameter read after a call is clobbered (caller-saved); mwcc preserves it in
// a callee-saved register (r31). Until that allocator exists this must DEFER, never
// emit a read of the clobbered register. Flips to byte-exact when r31 spilling lands.
int g(void);
int callclobber(int a){ g(); return a; }
