// A store-through-pointer beside a PRODUCT-ADDRESS return — the __va_arg diamond-arm cluster reduced:
// `*reg = (char)(g + inc); return base + g * rs;`. mwcc hoists the long-latency multiply FIRST (into
// the scratch), computes the store value IN PLACE on its first operand's register, stores, then the
// return add:
//   mullw r0,g,rs; add g,g,inc; stb g,0(reg); add r3,base,r0; blr
// Gated to a CHAR-pointee return base (stride 1): a wider pointee scales the product by its stride
// (int* would multiply by 4), which the unscaled emission would MISCOMPILE — measured and gated.
// (fire 669)
char* spr(char* base, int g, int rs, char* reg, int inc) { *reg = (char)(g + inc); return base + g * rs; }
// MEMBER-base variant (fire 670): `return list->area + g * rs;` FLIPS the schedule — the store issues
// FIRST with its value in the SCRATCH (the multiply still needs its operand), the product reuses the
// freed scratch, and the member load reclaims the dying struct-pointer register:
//   add r0,g,inc; stb r0,0(reg); mullw r0,g,rs; lwz r3,4(list); add r3,r3,r0; blr
typedef struct VL2 { int gpr; char* area; } VL2;
char* spr_member(VL2* list, int g, int rs, char* reg, int inc) { *reg = (char)(g + inc); return list->area + g * rs; }
