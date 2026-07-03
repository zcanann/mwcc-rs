/* Fire 377: GUARD-BLOCK MUTATIONS — the s_floor skeleton's first rung.
 * A chain of nested no-else ifs whose innermost body assigns constants
 * to int params, then an expression return: every guard branches to
 * ONE join; the block mutates params in their own registers (li); the
 * join computes the return. Bare-variable returns stay with the
 * bclr-fold arms. */
int guard_block_two(int i0, int i1, int c)
{
	if (c) {
		i0 = 0;
		i1 = 0;
	}
	return i0 | i1;
}

int guard_block_nested(int i0, int i1, int j0)
{
	if (j0 < 20) {
		if (j0 < 0) {
			i0 = 0;
			i1 = 0;
		}
	}
	return i0 | i1;
}

int guard_block_deep(int i0, int i1, int j0, int c)
{
	if (c) {
		if (j0 < 20) {
			if (j0 < 0) {
				i0 = 0;
				i1 = 0;
			}
		}
	}
	return i0 + i1;
}

int guard_block_lis(int i0, int i1, int c)
{
	if (c) {
		i0 = 0xbff00000;
		i1 = 0;
	}
	return i0 | i1;
}

int guard_block_leaf(int i0, int i1, int c)
{
	if (c) {
		i0 = i1 + 1;
		i1 = 0;
	}
	return i0 | i1;
}

int guard_block_early_const(int i0, int i1, int j0)
{
	if (j0 < 20) {
		if ((i0 | i1) == 0) {
			return 7;
		}
		i1 = 0;
	}
	return i0 | i1;
}

int guard_block_early_fold(int i0, int i1, int j0)
{
	if (j0 < 20) {
		if ((i0 | i1) == 0) {
			return i0;
		}
		i1 = 0;
	}
	return i0 | i1;
}

int guard_block_mask(int i0, int i1, int c)
{
	if (c) {
		i0 &= 0x7ff;
		i1 = 0;
	}
	return i0 | i1;
}
