/* `#pragma cplusplus reset` restores C linkage after a language-scoped MSL
 * inline: the first local-static parent mangles, while the second does not. */
#pragma cplusplus on
extern inline float scoped_inline(float x)
{
	static const double mangled = .5;
	return x;
}

#pragma cplusplus reset
extern inline float ordinary_inline(float x)
{
	static const double unmangled = .25;
	return x;
}

double cplusplus_reset_anchor(double x)
{
	return x * 3.0;
}
