// flags: -O4,p -inline auto,deferred
// builds: 1.3 1.3.2 2.0 2.0p1 2.6 2.7

/* Deferred fdlibm ldexp. Its inlined classifier creates the full exponent
 * adjustment CFG while the five data constants remain pooled. */
extern double copysign(double, double);

inline int __fpclassifyf(float value)
{
	unsigned long integer = *(unsigned long*)&value;
	switch (integer & 0x7f800000) {
	case 0x7f800000:
		if ((integer & 0x7fffff) != 0)
			return 1;
		return 2;
	case 0:
		if ((integer & 0x7fffff) != 0)
			return 5;
		return 3;
	}
	return 4;
}

inline int __fpclassifyd(double value)
{
	switch (*(int*)&value & 0x7ff00000) {
	case 0x7ff00000:
		if ((*(int*)&value & 0x000fffff) || (*(1 + (int*)&value) & 0xffffffff))
			return 1;
		return 2;
	case 0:
		if ((*(int*)&value & 0x000fffff) || (*(1 + (int*)&value) & 0xffffffff))
			return 5;
		return 3;
	}
	return 4;
}

static const double
two54 = 1.80143985094819840000e+16,
twom54 = 5.55111512312578270212e-17,
huge = 1.0e+300,
tiny = 1.0e-300;

double ldexp(double x, int n)
{
	int k, hx, lx;
	if (!(((sizeof(x) == sizeof(float) ? __fpclassifyf(x) : __fpclassifyd(x)) > 2)) || x == 0.0)
		return x;
	hx = *(int*)&x;
	lx = *(1 + (int*)&x);
	k = (hx & 0x7ff00000) >> 20;
	if (k == 0) {
		if ((lx | (hx & 0x7fffffff)) == 0)
			return x;
		x *= two54;
		hx = *(int*)&x;
		k = ((hx & 0x7ff00000) >> 20) - 54;
		if (n < -50000)
			return tiny * x;
	}
	if (k == 0x7ff)
		return x + x;
	k += n;
	if (k > 0x7fe)
		return huge * copysign(huge, x);
	if (k > 0) {
		*(int*)&x = (hx & 0x800fffff) | (k << 20);
		return x;
	}
	if (k <= -54) {
		if (n > 50000)
			return huge * copysign(huge, x);
		else
			return tiny * copysign(tiny, x);
	}
	k += 54;
	*(int*)&x = (hx & 0x800fffff) | (k << 20);
	return x * twom54;
}
