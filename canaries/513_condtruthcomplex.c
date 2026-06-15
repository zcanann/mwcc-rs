// `cond ? 1 : 0` for a non-comparison condition is the truthiness cond != 0;
// `? 0 : 1` is cond == 0. The value (even a both-complex one) flows through the
// comparison idiom the allocator unlocked: mullw; neg; or; srwi.
int condtruthcomplex(int a, int b, int c, int d){ return ((a + b) * (c + d)) ? 1 : 0; }
