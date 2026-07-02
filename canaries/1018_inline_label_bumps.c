/* Skipped inline definitions advance mwcc's @N counter by the labels their
 * (compiled, then dropped) bodies use — measured per construct: static base
 * 3, plain base 0; if +2; else/switch/case/default/||/&& +1 each; while +4;
 * for +5; a ternary +0. The BfBB __fpclassifyf shape (switch + 2 cases +
 * 2 if/else) totals 9; the double version's two ||s make 11. */
inline int fpclassify_like(int a)
{
	switch (a & 3) {
	case 3:
	{
		if (a & 4)
			return 1;
		else
			return 2;
		break;
	}
	case 0:
	{
		if (a & 8)
			return 5;
		else
			return 3;
		break;
	}
	}
	return 4;
}

inline int with_or_and(int a, int b)
{
	if (a || b)
		return 1;
	if (a && b)
		return 2;
	return 3;
}

inline int with_loops(int a)
{
	int i;
	int s = 0;
	for (i = 0; i < a; i++)
		s += i;
	while (s > 100)
		s -= 3;
	return s ? s : a;
}

static inline int static_with_if(int a)
{
	if (a)
		return 1;
	return 2;
}

double f(double x)
{
	return x * 2.0;
}
