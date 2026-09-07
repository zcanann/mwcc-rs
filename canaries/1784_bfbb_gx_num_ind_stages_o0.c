// Complete BfBB stage-count update, including the dirty mask.
// flags: -O0 -Cpp_exceptions off -pragma "cats off"
typedef unsigned char u8;
struct Context { unsigned short pad, bpSentNot; unsigned char padding[512]; unsigned genMode; unsigned char remaining[932]; unsigned dirtyState; };
extern struct Context* const __GXData;
extern unsigned __rlwimi(unsigned, unsigned, unsigned, unsigned, unsigned);
#define CHECK_GXBEGIN(line, name)
#define SET_REG_FIELD(line, reg, size, shift, value) do { (reg) = __rlwimi((reg), (value), (shift), 32-(shift)-(size), 31-(shift)); } while (0)
void GXSetNumIndStages(u8 nIndStages)
{
    CHECK_GXBEGIN(353, "GXSetNumIndStages");

    SET_REG_FIELD(356, __GXData->genMode, 3, 16, nIndStages);
    __GXData->dirtyState |= 6;
}
