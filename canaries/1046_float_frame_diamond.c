/* Fire 362: the FRAME-punned conditional-local diamond (k_cos's actual
 * qx form). The else arm stores through the pun (`*(int*)&qx = HI;
 * *((int*)&qx+1) = 0;`) — HI a general leaf (direct stw) or leaf minus a
 * lis-able constant (addis into the freed condition register); the then
 * arm stores a pooled literal through the f0 scratch; the cmpwi HOISTS
 * above the stwu; the tail reads qx as a FrameLoad node (value id 7).
 * A frame-local FACTOR of the root multiply blocks the root contraction
 * (fmul+fadd / fmul+fsub, measured) while inner fmadds keep fusing and
 * an ADDEND qx fuses normally. */
double frame_diamond(double x, int c, int k)
{
	double qx;

	if (c) {
		qx = 0.28125;
	} else {
		*(int *)&qx = k;
		*((int *)&qx + 1) = 0;
	}
	return x + qx * (1.5 + qx * 2.5);
}

double frame_diamond_addis(double x, int c, int k)
{
	double qx;

	if (c) {
		qx = 0.28125;
	} else {
		*(int *)&qx = k - 0x00200000;
		*((int *)&qx + 1) = 0;
	}
	return x + qx * (1.5 + qx * 2.5);
}

double frame_diamond_fsub(double x, int c, int k)
{
	double qx;

	if (c) {
		qx = 0.28125;
	} else {
		*(int *)&qx = k;
		*((int *)&qx + 1) = 0;
	}
	return x - qx * (1.5 + qx * 2.5);
}

double frame_diamond_addend(double x, int c, int k)
{
	double qx;

	if (c) {
		qx = 0.28125;
	} else {
		*(int *)&qx = k;
		*((int *)&qx + 1) = 0;
	}
	return qx + x * (1.5 + x * 2.5);
}
