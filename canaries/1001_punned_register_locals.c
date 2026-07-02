/* Register locals initialized from a type-punned frame read (`int hx =
 * *(int*)&x;`) inline into their single use, landing on the direct punning
 * form the frame-resident path already compiles: stwu; stfd f1,8(r1); lwz;
 * <op>; addi r1; blr. The load stages through r0 when it feeds an operation,
 * and lands in r3 directly when returned bare. */

/* bare return: lwz r3,8(r1). */
int ident(double x)
{
	int hx = *(int*)&x;
	return hx;
}

/* mask: lwz r0; clrlwi r3,r0,1. */
int mask_hi(double x)
{
	int hx = *(int*)&x;
	return hx & 0x7fffffff;
}

/* shift + bias: lwz r0; srawi r3,r0,20; addi r3,r3,-1022. */
int shifted(double x)
{
	int hx = *(int*)&x;
	return (hx >> 20) - 1022;
}

/* both words, chained substitution: lwz; lwz; or. */
int both(double x)
{
	int hx = *(int*)&x;
	int lx = *(1 + (int*)&x);
	return hx | lx;
}

/* unsigned local, wrapping mask: rlwinm. */
unsigned int umask(double x)
{
	unsigned int hx = *(unsigned int*)&x;
	return hx & 0x800fffff;
}

/* low word only. */
int lohi(double x)
{
	int lx = *(1 + (int*)&x);
	return lx >> 3;
}
