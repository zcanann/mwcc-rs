/* frexp's opening composed: a leading `*eptr = C;` store through an int-pointer
 * parameter riding ahead of punned guards.
 *
 * Measured schedule: the store's li hoists into the prologue AHEAD of the
 * guard's lis (statement order); the guard word's lwz hoists ABOVE the store;
 * the store fills the load latency BEFORE the mask; and the masked value is a
 * NEW value home — the allocator hands it the register the dead pointer frees
 * (clrlwi r3,r5,1 after eptr's stw), pure liveness. */

/* unmasked: li r4; lis r0; lwz r5; stw r4,0(r3); cmpw r5,r0. */
double f2(double x, int* eptr)
{
	int hx = *(int*)&x;
	*eptr = 0;
	if (hx < 0x00100000)
		return 0.0;
	return x;
}

/* masked: the mask result takes the freed r3 (clrlwi r3,r5,1). */
double f3(double x, int* eptr)
{
	int hx = *(int*)&x;
	int ix = hx & 0x7fffffff;
	*eptr = 0;
	if (ix >= 0x7ff00000)
		return 0.5;
	return x;
}
