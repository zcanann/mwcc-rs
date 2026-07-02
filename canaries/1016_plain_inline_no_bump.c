/* A PLAIN (non-static) skipped `inline` definition does NOT advance mwcc's
 * anonymous-@N counter; a STATIC inline one advances it by 3 (measured:
 * baseline @5, static inline @8, plain inline @5, both @8). The pool
 * constant below lands at @8 — bumped only by the static one. */
inline void pad_stack(void) { int pad = 0; }
static inline double fab(double v) { return v; }

double f(double x)
{
	return x * 2.0;
}
