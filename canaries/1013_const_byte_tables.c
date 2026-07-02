/* The SSBM ctype variant: CONST tables (.rodata) and the `c & 0xFF` index,
 * which normalizes to the same clrlwi as the (unsigned char) cast.
 *
 * KNOWN GAP (not covered here): a file mixing const and non-const tables gets
 * one symbol-table position wrong — no corpus file mixes them. */

const unsigned char clower[256] = {1, 2, 3};

int tol2(int c)
{
	return clower[c & 0xFF];
}
