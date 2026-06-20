// A const float array (> 8 bytes) lands in read-only `.rodata`, each element the
// 32-bit IEEE pattern. Integer literals in a float initializer are converted to
// float (`{1,2,3,4}` becomes 1.0f..4.0f), and the array object is word-aligned.
const float constfloatarr[4] = { 1, 2, 3, 4 };
