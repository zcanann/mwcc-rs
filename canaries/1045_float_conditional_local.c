/* Fire 361: the CONDITIONAL-LOCAL diamond (k_cos's qx, register form).
 * `if (c) { qx = A; } else { qx = B; }` + a float tail reading qx: both
 * arms load the SAME register — the one the tail's DAG assigns qx as a
 * window-top tier value (the PHANTOM node, value id 8, emits nothing) —
 * and fall through the join into the tail. The if pair + join consume 3
 * pre-pool labels; pool order = then-arm, else-arm, tail literals. */
double cond_local_deep(double x, int c)
{
	double qx;

	if (c) {
		qx = 0.28125;
	} else {
		qx = 12.5;
	}
	return x + qx * (1.5 + qx * 2.5);
}

double cond_local_mul(double x, int c)
{
	double qx;

	if (c == 0) {
		qx = 0.28125;
	} else {
		qx = 12.5;
	}
	return x * qx + 3.5;
}

double cond_local_cmp(double x, int c)
{
	double qx;

	if (c > 3) {
		qx = 0.28125;
	} else {
		qx = 12.5;
	}
	return x + qx * (1.5 + qx * 2.5);
}
