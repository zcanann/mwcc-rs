/* if/ELSE diamonds over a returned parameter — the two branchy lowerings.
 *
 * RE-TEST SPLIT (one arm at most rewrites v last): two independent guards, the
 * second re-emitting the compare with the branch sense inverted; the second
 * guard folds to a conditional return when the merge is empty.
 *
 * ARM-EXIT (both arms rewrite v last): each arm computes the return value
 * directly into r3 and returns; an arm that would emit nothing (a copy whose
 * source is already r3) folds its branch to b<c>lr.
 *
 * Pure-assign diamonds (no store) take the select layouts, and three-statement
 * arms the working-register diamond — both defer. */

extern int g;
extern int h;

/* re-test: store then, assign else. */
int sel3(int a, int b)
{
	if (a == 0) {
		g = b;
	} else {
		b = b + 7;
	}
	return b;
}

/* re-test: assign then, store else. */
int r1(int a, int b)
{
	if (a == 0) {
		b = b + 7;
	} else {
		g = b;
	}
	return b;
}

/* re-test: stores both sides. */
int r2(int a, int b)
{
	if (a == 0) {
		g = b;
	} else {
		h = b;
	}
	return b;
}

/* re-test with v in r3: the second guard folds to beqlr. */
int r3fold(int a)
{
	if (a == 0) {
		g = a;
	} else {
		a = a - 1;
	}
	return a;
}

/* arm-exit: both arms store+assign; the else copy from r3 emits nothing. */
int r4mix(int a, int b)
{
	if (a < b) {
		g = a;
		b = b + 1;
	} else {
		h = b;
		b = a;
	}
	return b;
}

/* arm-exit: bare assign then-arm. */
int x1(int a, int b)
{
	if (a == 0) {
		b = b + 1;
	} else {
		h = b;
		b = a;
	}
	return b;
}

/* arm-exit, EMPTY else arm: the branch folds to bnelr. */
int x2(int a, int b)
{
	if (a == 0) {
		g = b;
		b = b + 1;
	} else {
		b = a;
	}
	return b;
}

/* arm-exit with a constant arm (li r3,5). */
int x3(int a, int b)
{
	if (a == 0) {
		g = b;
		b = 5;
	} else {
		h = b;
		b = a;
	}
	return b;
}

/* arm-exit, EMPTY then arm: the mirror fold (beqlr). */
int x5(int a, int b)
{
	if (a == 0) {
		b = a;
	} else {
		g = b;
		b = b + 1;
	}
	return b;
}
