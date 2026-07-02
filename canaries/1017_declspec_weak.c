/* __declspec(weak) marks the symbol WEAK (STB_WEAK) — on a prior prototype
 * it applies to the later definition too — and the .comment record's flags
 * word (after the align word) carries 0x0e000000 for the weak symbol
 * (measured across weak-only / weak-first / weak-last orderings). */
__declspec(weak) int weak_by_prototype(int c);

int weak_by_prototype(int c)
{
	return c + 1;
}

__declspec(weak) int weak_direct(int a)
{
	return a + 2;
}

int plain_after(int a)
{
	return a + 3;
}
