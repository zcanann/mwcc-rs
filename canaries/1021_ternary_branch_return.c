/* A conditional return whose false arm is outside the branchless-select
 * vocabulary (a table load) emits mwcc's early-return BRANCH — `cmpwi; bne;
 * li; blr; <fall-through>; blr` — the ctype tolower shape, both as a ternary
 * and as an if-guard; (int) casts of array elements are no-op wrappers
 * (unsigned zero-extends in the load, signed extends inside the Index path). */
typedef unsigned char u8;
extern u8 lower_map[256];
extern signed char signed_map[256];
extern int int_map[256];

inline int table_pick(int c)
{
	return (c == -1 ? -1 : (int)lower_map[(u8)c]);
}

int as_ternary(int c)
{
	return table_pick(c);
}

int as_guard(int c)
{
	if (c == -1)
		return -1;
	return (int)lower_map[(u8)c];
}

int int_element(int c)
{
	if (c == -1)
		return -1;
	return (int)int_map[c];
}

int signed_element(int c)
{
	if (c == -1)
		return -1;
	return (int)signed_map[c];
}
