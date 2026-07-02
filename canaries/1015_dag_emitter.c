/* THE DAG EMITTER's first flight: leaf multi-store bodies compiled through the
 * MEASURED models — linearize (dual-issue, critical-path, 10/10 on the
 * scheduler dataset) orders the block, assign_registers_v3 (closed intervals,
 * r0-mid-pool, 10/10 on the register fixtures) picks every register. No
 * bespoke arm: these shapes deferred before this emitter existed. */

extern int g;
extern int h;
extern int k;

void two_chains(int a, int b)
{
	g = (a + 1) * 2;
	h = (b + 2) * 3;
}

void three_ties(int a, int b, int c)
{
	g = a + 1;
	h = b + 2;
	k = c + 3;
}

void deep_last(int a, int b)
{
	g = a + 1;
	h = ((b + 2) * 3) + 4;
}

void mixed_ops(int a, int b)
{
	g = a * 3;
	h = (b >> 2) + 7;
}

void with_load(int* p, int b)
{
	g = *p + 5;
	h = (b + 2) * 3;
}

void equal_pair(int a, int b)
{
	g = (a >> 1) + 5;
	h = (b >> 2) + 7;
}

void three_two_op(int a, int b, int c)
{
	g = (a + 1) * 2;
	h = (b + 2) * 3;
	k = (c + 3) * 4;
}

/* constants as values: li nodes through the same models — including the
 * slot-0 FINAL in-place datum (g's addi reuses r3 first-of-pair because it
 * feeds its store directly; an intermediate there takes the closed pool). */
void const_after(int a)
{
	g = a + 1;
	h = 5;
}

void const_before(int a)
{
	g = 5;
	h = a + 1;
}

void const_amid(int a, int b)
{
	g = a + 1;
	h = 7;
	k = (b + 2) * 3;
}
