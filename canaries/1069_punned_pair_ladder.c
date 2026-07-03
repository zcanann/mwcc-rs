/* Fire 430: the PUNNED PAIR LADDER — the frame/int marriage. Two
   double params spill (stwu -32; stfd 8/16), the four __HI/__LO
   extracts load in first-use order with ly DELAYED past the cmpw
   into its branch latency (reusing dead hx's r0), the fire-427
   ladder runs on the loaded registers, and the arms JOIN at the
   shared epilogue (li; b JOIN; addi r1; blr — inline blr is a
   FRAMELESS-only behavior). @N +7 (the ladder's internal labels;
   extab lands at @12, measured). */
int pun_pair_ladder(double x, double y)
{
	int hx, hy;
	unsigned lx, ly;
	hx = *(int *)&x;
	lx = *(1 + (int *)&x);
	hy = *(int *)&y;
	ly = *(1 + (int *)&y);
	if (hx <= hy) {
		if ((hx < hy) || (lx < ly))
			return 1;
		if (lx == ly)
			return 2;
	}
	return 3;
}
/* Fire 432: the WRITEBACK NORM (e_fmod's normalize-output tail).
   hx - HI_BIT folds to addis (high-half subtract); the stfd spill
   DELAYS into the int computation; the two punned stores REORDER BY
   READINESS (LO first — lx was ready before the or-chain); lfd
   reloads for the return; frame 16. @N +0. */
double wb_norm(double x, int hx, unsigned lx, int iy, int sx)
{
	hx = ((hx - 0x00100000) | ((iy + 1023) << 20));
	*(int *)&x = hx | sx;
	*(1 + (int *)&x) = lx;
	return x;
}
