/* Block-scoped declarations hoist to the function with their initializations
 * as positioned Assigns; under `#pragma cplusplus on` (scoped by push/pop) a
 * skipped inline's $localstatic parent name MANGLES CodeWarrior-style
 * (sqrtf(float) -> sqrtf__Ff). */
#pragma push
#pragma cplusplus on
extern inline float sqrtf_like(float x)
{
	static const double _half = .5;
	static const double _three = 3.0;
	return x;
}
#pragma pop

inline float unmangled_after(float x)
{
	static const double _quarter = .25;
	return x;
}

double g(double x)
{
	return x * 3.0;
}
