// A single never-reassigned local whose initializer is a conditional (a branchless
// idiom like abs) and which is then used inside a larger expression: it folds into
// the use, matching the direct form — `int y = x<0?-x:x; return y + 1;` is identical
// to `(x<0?-x:x) + 1`. (Returning the local bare stays on the straight-line path.)
int cle_abs_plus(int x)   { int y = x < 0 ? -x : x; return y + 1; }
int cle_absm_plus(int x)  { int y = x > 0 ? x : -x; return y + 1; }
