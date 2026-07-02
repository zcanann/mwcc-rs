/* The PUNNED-BITS guard + float-tail composition (fire 346, the k_sin
 * prefix): stwu -16 / staged lis / stfd x / lwz / clrlwi / cmpw / bge+8 /
 * b EPILOGUE, extra int guards in branch form, the float DAG tail, the
 * shared addi/blr epilogue; extab/extabindex from the frame; the epilogue
 * consumes one extra @N ahead of the pool. */
double punned_sign(double x)
{
	int ix = *(int*)&x;
	if (ix < 0) {
		return x;
	}
	return x * (1.5 + x * 2.5);
}

double punned_masked(double x)
{
	int ix = *(int*)&x & 0x7fffffff;
	if (ix < 0x3e400000) {
		return x;
	}
	return x * (11.5 + x * 12.5);
}

double punned_two_guards(double x, int iy)
{
	int ix = *(int*)&x & 0x7fffffff;
	if (ix < 0x3e400000) {
		return x;
	}
	if (iy) {
		return x;
	}
	return x * (21.5 + x * 22.5);
}
