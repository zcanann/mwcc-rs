// Two function definitions in one translation unit: they share a single .text
// (concatenated), one .mwcats.text with a record each, a symbol per function,
// and a .comment that grows by eight bytes per function. Whole-object exact.
int add2(int a, int b){ return a + b; }
int sub2(int a, int b){ return a - b; }
