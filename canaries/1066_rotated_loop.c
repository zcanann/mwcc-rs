/* Fire 413: the ROTATED LOOP (the e_fmod ilogb family) — non-counted
   loops emit as init; b TEST; BODY: [step][body]; TEST: cond;
   b<positive> BODY; [mr] with NO unrolling (counted loops take the
   ctr/unroll machinery — deferred). Registers: params in place; a
   condition-only computed value takes r0 even across the backward
   branch; the returned local takes a param home freed during init,
   else the next free; big bounds hoist to r0 (lis) before the loop.
   @N +0. */
int rotloop_ilogb(int lx)
{
	int ix, i;
	for (ix = -1043, i = lx; i > 0; i <<= 1)
		ix -= 1;
	return ix;
}
int rotloop_shifted_init(int hx)
{
	int ix, i;
	for (ix = -1022, i = (hx << 11); i > 0; i <<= 1)
		ix -= 1;
	return ix;
}
int rotloop_while(int hz, int hx)
{
	while (hz < 0x00100000) {
		hz = hz + hz;
		hx -= 1;
	}
	return hx;
}
/* Fire 414: the char WALK (lbz + extsb. record truthiness test, bne
   back) and the DO-WHILE (no rotation — the body falls into the test;
   cmpw register compares against a param bound). Initialized locals
   (int n = 0) join the init plan at the next free register. */
int rotloop_strlen(char *p)
{
	int n = 0;
	while (*p) {
		p++;
		n++;
	}
	return n;
}
int rotloop_do_while(int n)
{
	int i = 0;
	do {
		i++;
	} while (i < n);
	return i;
}
