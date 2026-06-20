// A string-pointer global in a unit that also has functions: the pooled string
// is still @1, but a function's anonymous symbols shift — its first float
// constant moves from @5 to @(5 + number-of-distinct-strings). The .rela.sdata
// section orders before .rela.mwcats.text (by target-section order).
char* strpoolfn_msg = "hi";
float strpoolfn_f(void) { return 1.5f; }
