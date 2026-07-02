/* Pure-assign diamonds — `if (c) v = X; else v = Y; return v;` with no side
 * effects — take mwcc's SELECT layouts.
 *
 * A CONSTANT arm is SPECULATED into the phi register in the compare latency
 * slot (both constant: the else), the branch skipping the other arm; with no
 * constant a COPY else COALESCES (phi = the copy's source, no else code);
 * otherwise the else speculates. The phi is r3 itself when the conditional arm
 * does not read r3 — merge elided, branch folded to b<c>lr — else r0; a
 * coalesced phi is wherever the else source lives. */

/* coalesce: else copy — phi=r4, then-arm computes into it from r3. */
int sel(int a, int b)
{
	if (a < b) {
		a = a + 1;
	} else {
		a = b;
	}
	return a;
}

/* speculate: computed else into r0 in the latency slot, copy then overwrites. */
int sel2(int a, int b)
{
	if (a < b) {
		b = a;
	} else {
		b = b - 1;
	}
	return b;
}

/* speculate: both computed — else into r0, then conditionally overwrites. */
int p1(int a, int b)
{
	if (a < b) {
		b = a + 1;
	} else {
		b = b - 1;
	}
	return b;
}

/* coalesce: both copies — phi = else source (r5), then-arm mr into it. */
int p2(int a, int b, int c, int d)
{
	if (a == 0) {
		d = b;
	} else {
		d = c;
	}
	return d;
}

/* both constants: else speculated INTO r3, branch folds to bnelr. */
int p3(int a, int b)
{
	if (a == 0) {
		b = 5;
	} else {
		b = 7;
	}
	return b;
}

/* coalesce with phi already r3 (else copies a): fold to bgelr. */
int p4(int a, int b)
{
	if (a < b) {
		b = b - 1;
	} else {
		b = a;
	}
	return b;
}

/* then-constant speculated (the direction flips): li r0,5; beq; addi r0,r3,1. */
int p6(int a, int b)
{
	if (a == 0) {
		b = 5;
	} else {
		b = a + 1;
	}
	return b;
}

/* else-constant speculated, conditional then reads r3 so phi=r0. */
int p7(int a, int b)
{
	if (a == 0) {
		b = a + 1;
	} else {
		b = 7;
	}
	return b;
}

/* const-speculation BEATS coalescing: li r3,5; beqlr; mr r3,r4. */
int p8(int a, int b, int c)
{
	if (a == 0) {
		c = 5;
	} else {
		c = b;
	}
	return c;
}

/* coalesce where the then-arm reads the phi itself: addi r4,r4,2 in place. */
int p9(int a, int b, int c)
{
	if (a == 0) {
		c = b + 2;
	} else {
		c = b;
	}
	return c;
}
