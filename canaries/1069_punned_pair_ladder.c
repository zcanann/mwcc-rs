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
