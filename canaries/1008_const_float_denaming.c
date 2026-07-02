/* Two rules measured this round:
 *
 * A STATIC CONST float/double global is DE-NAMED: every read compiles as the
 * literal value, pooled anonymously (@N in .sdata2) — no named symbol, and the
 * fmul takes the pool operand as frA exactly like the inline-literal form.
 *
 * A spilled double parameter whose slot is never WRITTEN is still live in f1
 * at its return, and mwcc emits no reload (`*eptr = f(hx); return x;` ends at
 * the stw; the writeback shapes reload). */

static const double two54 = 1.80143985094819840000e+16;
static const double half = 0.5;
static const float scale = 2.5f;

double dm(double x)
{
	return x * two54;
}

double dh(double x)
{
	return x * half;
}

float fm(float x)
{
	return x * scale;
}

/* unwritten slot: no reload before the epilogue. */
double m2(double x, int* eptr)
{
	int hx = *(int*)&x;
	*eptr = (hx >> 20) - 1022;
	return x;
}
