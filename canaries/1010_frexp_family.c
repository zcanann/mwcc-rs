/* The frexp family — the fdlibm leaf-diamond acceptance target, whole-function:
 * punned locals living as virtuals across the writeback diamond (redefinition
 * keeps one home = merge agreement; the allocator reproduces hx->r5, ix->r4,
 * lx->r6 with eptr pinned in r3), the param-returning disjunction guard with
 * its join reload, the subnormal float-multiply writeback block, and the
 * interleaved closing arithmetic — 35 instructions, every order measured. */

static const double two54 = 1.80143985094819840000e+16;

double frexp(double x, int* eptr)
{
	int hx, ix, lx;
	hx = *(int*)&x;
	ix = 0x7fffffff & hx;
	lx = *(1 + (int*)&x);
	*eptr = 0;
	if (ix >= 0x7ff00000 || ((ix | lx) == 0))
		return x;
	if (ix < 0x00100000) {
		x *= two54;
		hx = *(int*)&x;
		ix = hx & 0x7fffffff;
		*eptr = -54;
	}
	*eptr += (ix >> 20) - 1022;
	hx = (hx & 0x800fffff) | 0x3fe00000;
	*(int*)&x = hx;
	return x;
}
