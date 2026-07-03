/* Fire 398: the SHIFT-WRITEBACK family (s_floor arm2's core) — the
   first shapes allocated by the fitted int_alloc v2 model (13/13
   captures, docs/int-allocator-frontier.md). `i = C >> j0` leads,
   an integral early-return tests through the variable mask, then
   mutations: `l &= ~i` (fused andc; TWO share one not r0), `l &= K`
   (clrlwi r0, store from r0), `l = K` (li r0 SUNK below the other
   mutations, store from r0; the home is discarded when it was read
   in the test, and never loaded when read nowhere). @N: 1 + one per
   loaded local + one for the shared not temp. */
double shiftwb_single(double x)
{
	int i0, j0;
	unsigned i;
	i0 = *(int *)&x;
	j0 = ((i0 >> 20) & 0x7ff) - 0x3ff;
	i = (0x000fffff) >> j0;
	if ((i0 & i) == 0)
		return x;
	i0 &= (~i);
	*(int *)&x = i0;
	return x;
}
double shiftwb_small_mask(double x)
{
	int i0, j0;
	unsigned i;
	i0 = *(int *)&x;
	j0 = ((i0 >> 20) & 0x7ff) - 0x3ff;
	i = (0x7ff0) >> j0;
	if ((i0 & i) == 0)
		return x;
	i0 &= (~i);
	*(int *)&x = i0;
	return x;
}
double shiftwb_discarded(double x)
{
	int i0, i1, j0;
	unsigned i;
	i0 = *(int *)&x;
	i1 = *((int *)&x + 1);
	j0 = ((i0 >> 20) & 0x7ff) - 0x3ff;
	i = (0x000fffff) >> j0;
	if (((i0 & i) | i1) == 0)
		return x;
	i0 &= (~i);
	i1 = 0;
	*(int *)&x = i0;
	*((int *)&x + 1) = i1;
	return x;
}
double shiftwb_two_andc(double x)
{
	int i0, i1, j0;
	unsigned i;
	i0 = *(int *)&x;
	i1 = *((int *)&x + 1);
	j0 = ((i0 >> 20) & 0x7ff) - 0x3ff;
	i = (0x000fffff) >> j0;
	if (((i0 & i) | i1) == 0)
		return x;
	i0 &= (~i);
	i1 &= (~i);
	*(int *)&x = i0;
	*((int *)&x + 1) = i1;
	return x;
}
double shiftwb_never_read(double x)
{
	int i0, i1, j0;
	unsigned i;
	i0 = *(int *)&x;
	i1 = *((int *)&x + 1);
	j0 = ((i0 >> 20) & 0x7ff) - 0x3ff;
	i = (0x000fffff) >> j0;
	if ((i0 & i) == 0)
		return x;
	i0 &= (~i);
	i1 = 0;
	*(int *)&x = i0;
	*((int *)&x + 1) = i1;
	return x;
}
double shiftwb_const_mask_mutation(double x)
{
	int i0, i1, j0;
	unsigned i;
	i0 = *(int *)&x;
	i1 = *((int *)&x + 1);
	j0 = ((i0 >> 20) & 0x7ff) - 0x3ff;
	i = (0x000fffff) >> j0;
	if (((i0 & i) | i1) == 0)
		return x;
	i0 &= (~i);
	i1 &= 0x7ff;
	*(int *)&x = i0;
	*((int *)&x + 1) = i1;
	return x;
}
double shiftwb_sunk_rewrite(double x)
{
	int i0, i1, j0;
	unsigned i;
	i0 = *(int *)&x;
	i1 = *((int *)&x + 1);
	j0 = ((i0 >> 20) & 0x7ff) - 0x3ff;
	i = (0x000fffff) >> j0;
	if (((i0 & i) | i1) == 0)
		return x;
	i1 = 0;
	i0 &= (~i);
	*(int *)&x = i0;
	*((int *)&x + 1) = i1;
	return x;
}
double shiftwb_nonzero_rewrite(double x)
{
	int i0, i1, j0;
	unsigned i;
	i0 = *(int *)&x;
	i1 = *((int *)&x + 1);
	j0 = ((i0 >> 20) & 0x7ff) - 0x3ff;
	i = (0x000fffff) >> j0;
	if (((i0 & i) | i1) == 0)
		return x;
	i0 &= (~i);
	i1 = 5;
	*(int *)&x = i0;
	*((int *)&x + 1) = i1;
	return x;
}
