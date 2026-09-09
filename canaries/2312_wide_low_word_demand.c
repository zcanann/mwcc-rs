typedef long long s64;
typedef unsigned long long u64;
unsigned low_add(u64 a, u64 b) { return (unsigned)(a + b); }
unsigned low_sub(s64 a, int b) { return (unsigned)(a - b); }
unsigned low_mul(u64 a, u64 b) { return (unsigned)(a * b); }
unsigned low_mask(u64 a) { return (unsigned)(a & 65535ULL); }
unsigned low_chain(u64 a, int b) { return (unsigned)(((a + b) * 7ULL) ^ 0x123456789abcdef0ULL); }
unsigned low_shift(u64 a) { return (unsigned)((a + 1) >> 17); }
unsigned low_loop(u64 a, unsigned n) { unsigned i; for (i=0;i<n;i++) a=a*3+1; return (unsigned)a; }
u64 shared_pair(u64 a, int b, unsigned *out) { u64 v=a+b; *out=(unsigned)v; return v; }
unsigned load_pair(volatile u64 *p) { u64 value=*p; return (unsigned)(value + 1); }
unsigned call_pair(u64 (*f)(u64), u64 a) { return (unsigned)(f(a) + 1); }
unsigned branch_pair(u64 a, u64 b, int c) { u64 v; if(c) v=a+1; else v=b*3; return (unsigned)v; }
unsigned loop_observed(u64 a, unsigned n, u64 *out) { unsigned i; for(i=0;i<n;i++) a=a*3+1; *out=a; return (unsigned)a; }
unsigned branch_observed(u64 a, u64 b, int c, u64 *out) { u64 v; if(c) v=a+1; else v=b*3; *out=v; return (unsigned)v; }
