// flags: -O4,p -inline auto,deferred

/* Deferred compilation retains additional internal control-flow labels before
 * numbering the pooled scale constant: five in builds 163/53, three in build
 * 81 and the later mainline. */

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
