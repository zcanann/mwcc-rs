/* Fire 406: the FLOAT callee-saved survivor — a double parameter
   surviving one external call. stfd f31,8(r1); fmr f31,f1 (the copy
   leaves f1 holding x for the call); bl; the LR reload FIRST for
   add/sub but AFTER the fmul (its latency starts early); lfd f31.
   The extab carries saved_fpr_count=1 (0x48). */
extern double survivor_g(double);
double survivor_add(double x)
{
	return survivor_g(x) + x;
}
double survivor_sub(double x)
{
	return x - survivor_g(x);
}
double survivor_mul(double x)
{
	return survivor_g(x) * x;
}
