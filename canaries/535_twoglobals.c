// Two defined globals: mwcc lays them out in REVERSE declaration order in .sbss
// (so first gets the higher offset), and emits a defined OBJECT symbol for each
// in first-reference order before the function.
int alpha;
int beta;
int sum_ab(void){ return alpha + beta; }
