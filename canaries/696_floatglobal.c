// Float/double file-scope globals serialize to their IEEE-754 bit patterns. A
// non-const float scalar lands in writable `.sdata` (4 bytes); a `double` is the
// 8-byte pattern. The initializer parser encodes the literal to raw bytes.
float floatglobal = 1.5f;
double doubleglobal = 1.5;
