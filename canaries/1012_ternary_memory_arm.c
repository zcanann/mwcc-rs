/* The ctype tolower shape: a tail ternary with a constant true arm and a
 * MEMORY-READING false arm takes the early-return layout — test, return the
 * constant on the branch, the load as the fall-through (cmpwi; bne ELSE;
 * li r3,-1; blr; ELSE: lis; clrlwi r0; addi; lbzx; blr). */

unsigned char lower[256] = {1, 2, 3};

int tol(int c)
{
	return c == -1 ? -1 : lower[(unsigned char)c];
}
