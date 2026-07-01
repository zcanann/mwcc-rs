// A two-case switch whose cases are separated by a single hole (a gap of exactly 2). mwcc
// pivots on the HOLE value at the range centre -- sending it to the default -- then handles
// each case as an adjacent leaf: `cmpwi hole; beq default; bge up; cmpwi lo; bge case_lo;
// b default; up: cmpwi hi+1; bge default; b case_hi`. The median-case comparison tree would
// instead pivot on the higher case value, a byte-DIFF (previously miscompiled).
int hole_13(int x) { switch (x) { case 1: return 7; case 3: return 8; } return 0; }
int hole_02(int x) { switch (x) { case 0: return 7; case 2: return 8; } return 0; }
int hole_57(int x) { switch (x) { case 5: return 7; case 7: return 8; } return 0; }
int hole_neg(int x) { switch (x) { case -1: return 7; case 1: return 8; } return 0; }
