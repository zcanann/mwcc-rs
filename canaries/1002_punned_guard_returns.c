/* The frexp guard skeleton: a leaf double function whose single guard tests a
 * type-punned word of the parameter and returns a float literal, falling
 * through to return the still-in-f1 parameter.
 *
 * Measured layout: the guard value (`lfd` from the pooled .sdata2 double)
 * falls INTO the shared epilogue; the skip branch targets the epilogue; the
 * fall-through return emits nothing. mwcc's label counter advances 2 for the
 * guard (the @N symbol numbers). Also locks double-precision literal returns
 * (`lfd` from an 8-byte pool constant, not `lfs` from a rounded 4-byte one). */

/* punned local in the guard, zero literal. */
double keep_or_zero(double x)
{
	int hx = *(int*)&x;
	if (hx == 0)
		return 0.0;
	return x;
}

/* the direct (no-local) form. */
double direct_guard(double x)
{
	if (*(int*)&x == 0)
		return 0.0;
	return x;
}

/* low word, inequality, non-zero literal. */
double lo_guard(double x)
{
	int lx = *(1 + (int*)&x);
	if (lx != 0)
		return 1.5;
	return x;
}

/* non-zero compare constant. */
double ne_guard(double x)
{
	int hx = *(int*)&x;
	if (hx != 3)
		return 0.5;
	return x;
}

/* the double-literal return fix in isolation: lfd from an 8-byte pool. */
double dzero(void)
{
	return 0.0;
}

double dpick(double x)
{
	return 3.5;
}

/* float stays single-precision: lfs from a 4-byte pool. */
float fpick(void)
{
	return 2.5f;
}

/* the pun STORE side, already exact: stw into the slot, lfd back. */
double set_hi(double x, int hx)
{
	*(int*)&x = hx;
	return x;
}
