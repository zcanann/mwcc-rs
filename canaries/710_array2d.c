// A multi-dimensional array global flattens row-major into one element list of
// the dimensions' product, with nested braces in the initializer flattened to
// match. Exercises a writable 2D array (.data) and a partially-initialized one
// (the unspecified tail is zero) — the data emission is the same as a 1D array.
int array2d[2][3] = { {1, 2, 3}, {4, 5, 6} };
static int array2d_partial[3][2] = { {1, 2}, {3, 4} };
