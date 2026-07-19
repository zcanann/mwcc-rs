// flags: -O4,p -inline auto,deferred

/* Deferred companion to 1064_tan_dispatcher.c. The parity-tail instruction
 * schedule is unchanged, but deferred compilation retains internal labels
 * before the zero constant is pooled. */
extern double __kernel_tan(double, double, int);
extern int __ieee754_rem_pio2(double, double*);

double tan(double x)
{
	double y[2], z = 0.0;
	int n, ix;
	ix = *(int*)&x;
	ix &= 0x7fffffff;
	if (ix <= 0x3fe921fb)
		return __kernel_tan(x, z, 1);
	else if (ix >= 0x7ff00000)
		return x - x;
	else {
		n = __ieee754_rem_pio2(x, y);
		return __kernel_tan(y[0], y[1], 1 - ((n & 1) << 1));
	}
}
