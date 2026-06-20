// An array length is a constant expression, so an enum constant or a folded
// expression (`grid[COUNT * STRIDE]`) sizes the array — not just a bare integer
// literal. (The marioparty4 game code sizes many global tables by enum counts.)
enum { ENUMLEN_COUNT = 8, ENUMLEN_STRIDE = 4 };
int enumlen_grid[ENUMLEN_COUNT * ENUMLEN_STRIDE];
int enumlen_get(int i) { return enumlen_grid[i]; }
