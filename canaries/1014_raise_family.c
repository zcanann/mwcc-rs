/* The raise family — the call-class acceptance target, whole-function: a
 * fn-pointer local loaded from a static dispatch table via lwzu's folded
 * pre-decrement, guard blocks sharing cold return blocks, a conditional
 * clear through the updated base, a branch-over exit call, and the dispatch
 * through ctr — with the local and parameter in callee-saved registers
 * (allocator-chosen: temp -> r31, sig -> r30 from the call-crossing pool;
 * the address chain takes the freed r3). 44 instructions, every order from
 * the fire-242 capture. */

typedef void (*__signal_func_ptr)(int);
extern void exit(int);
__signal_func_ptr signal_funcs[6];

int raise(int sig)
{
	__signal_func_ptr temp_r31;
	if (sig < 1 || sig > 6) {
		return -1;
	}
	temp_r31 = signal_funcs[sig - 1];
	if ((unsigned long)temp_r31 != 1) {
		signal_funcs[sig - 1] = 0;
	}
	if ((unsigned long)temp_r31 == 1 || (temp_r31 == 0 && sig == 1)) {
		return 0;
	}
	if (temp_r31 == 0) {
		exit(0);
	}
	temp_r31(sig);
	return 0;
}
