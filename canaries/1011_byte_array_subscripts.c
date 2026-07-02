/* Variable subscripts of a byte global array (the ctype table shape, ADDR16):
 * no scale — the index feeds lbzx raw. A plain index keeps its register and
 * the base takes one free register (lis b; addi b,b; lbzx d,b,i); a (unsigned
 * char) cast stages through r0 in the lis latency, and the base's addi lands
 * in the register the dead index frees (lis h; clrlwi r0,i,24; addi b,h;
 * lbzx d,b,r0). A masked accessor stages the load through r0 and folds the
 * single-bit mask as rlwinm. */

unsigned char map[256] = {1, 2, 3};

int f(int c)
{
	return map[c];
}

int g2(int c)
{
	return map[(unsigned char)c];
}

int isal(int c)
{
	return map[(unsigned char)c] & 0x40;
}
