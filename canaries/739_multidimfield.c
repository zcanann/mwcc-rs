// A struct with a multi-dimensional array field (`int grid[4][8]`) registers with
// the right layout — its size is the product of the dimensions times the element,
// and fields after it sit at the correct offset. Access to OTHER fields (before
// and after the 2D array) is byte-exact; access to the 2D member itself defers.
struct Mdf { int head; int grid[4][8]; int tail; };
struct Mdf mdf_g;
int mdf_head(struct Mdf *p) { return p->head; }
int mdf_tail(struct Mdf *p) { return p->tail; }
int mdf_gtail(void)         { return mdf_g.tail; }
