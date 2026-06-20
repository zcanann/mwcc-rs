// A `static` (file-local) global is emitted with a LOCAL symbol — placed among
// the local symbols, in declaration order, ahead of any function's `@N` entries —
// while still routing to the same section as a non-static global of its shape.
static int staticdata_small = 5;
static int staticdata_arr[4] = { 1, 2, 3, 4 };
