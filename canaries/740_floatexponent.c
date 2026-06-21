// Float literals in scientific notation: `e`/`E` followed by an optional sign and
// digits (`1.0e300`, `2.5e-10`, `1.5E5`), plus the no-fractional-dot form `1e10`
// (still a double) and a single-precision `6.28e2f`. The lexer consumes the whole
// exponent rather than splitting off `e300` as an identifier.
double fe_huge   = 1.0e300;
double fe_tiny   = 2.5e-10;
double fe_nodot  = 1e10;
double fe_caps   = 1.5E5;
float  fe_single = 6.28e2f;
