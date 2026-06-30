// A narrow `const` file-scope global reads as its value EXTENDED to int per its signedness — mwcc
// folds the read to that extended value (`const char c=200`->`li r3,-56`; `const unsigned char
// uc=200`->`li r3,200`; `const short s=40000`->`li r3,-25536`; `const unsigned short=40000`->lis+addi)
// while STILL emitting the raw byte/halfword storage (.sdata2: 1 byte for char, 2 for short). The
// parser folds the read via truncate_to_integer(value, declared_type) — the C integer cast on a
// constant — generalizing the const-int fold (911) to char/short with correct sign/zero extension.
const char           C  = 65;
const char           CN = 200;     // signed char -> reads -56
const unsigned char  UC = 200;     // -> 200
const short          S  = 1000;
const short          SN = 40000;   // signed short -> reads -25536
const unsigned short US = 40000;   // -> 40000 (needs lis+addi)
int      r_c(void)        { return C; }        // li r3,65
int      r_cn(void)       { return CN; }       // li r3,-56
int      r_uc(void)       { return UC; }       // li r3,200
int      r_s(void)        { return S; }        // li r3,1000
int      r_sn(void)       { return SN; }       // li r3,-25536
int      r_us(void)       { return US; }       // lis r3,1; addi r3,r3,-25536
int      add_c(int a)     { return a + C; }    // addi r3,r3,65
int      sum_c(void)      { return C + C; }    // li r3,130
