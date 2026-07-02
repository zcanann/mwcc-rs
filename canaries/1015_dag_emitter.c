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

/* return-bearing bodies: the return chain's value lands in r3 by the model's
 * construction; store chains ride r0; the ret2->ret3 handoff and the XER
 * hazard (two srawis) compose. */
int ret_mix(int a, int b)
{
	g = a + 1;
	return b + 2;
}

int ret_deep(int a, int b)
{
	g = (a + 1) * 2;
	return ((b + 2) * 3) + 4;
}

int ret_hazard(int a, int b)
{
	g = a >> 3;
	return b >> 4;
}

int ret_three(int a, int b, int c)
{
	g = a + 1;
	h = b * 3;
	return c - 2;
}

/* the fire-299 audit shapes: xor and variable-shift store values, and a
 * bare-param return (a Move node — the return pre-claims r3, the mr lands
 * after the store chain). All were live DIFFs on the legacy path before the
 * envelope covered them. */
int ret_xor(int a, int b)
{
	g = a ^ 5;
	return b + 1;
}

int ret_varshift(int a, int b)
{
	g = a << b;
	return b + 1;
}

int ret_bare(int a, int b)
{
	g = a + 1;
	return b;
}

/* unsigned right shifts: srwi/srw (rlwinm/logical forms — NO XER hazard,
 * unlike srawi/sraw); the promoted-signedness of the LEFT operand picks the
 * form, so a composite unsigned left ((a+b)>>3) is srwi too. Narrow (char/
 * short) params defer — they need re-extension vocabulary. */
unsigned int u;
int ret_srwi(unsigned int a, int b)
{
	u = a >> 3;
	return b + 1;
}

int ret_srw(unsigned int a, int b)
{
	u = a >> b;
	return b + 1;
}

int ret_srwi_wide(unsigned int a, unsigned int b, int c)
{
	u = (a + b) >> 3;
	return c + 1;
}

int ret_mixed_shift(unsigned int a, int b)
{
	u = a >> 3;
	return b >> 4;
}
