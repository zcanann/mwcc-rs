// Two signed char globals: mwcc batches the loads (lbz;lbz) ahead of the
// sign-extensions (extsb;extsb), not interleaved. On build 53 (unsigned char)
// neither extsb appears.
extern char a, b;
int gcharadd2(void){ return a + b; }
