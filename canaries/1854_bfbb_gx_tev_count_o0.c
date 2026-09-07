// TEV count updates preserve byte promotion before arithmetic.
// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned char u8;
struct Context { unsigned char pad[516]; unsigned long genMode; unsigned char gap[932]; unsigned long dirtyState; };
extern struct Context* const __GXData;
void GXSetNumTevStages(u8 nStages) {
    ((void)0);
    do { __GXData->genMode = __rlwimi(__GXData->genMode, nStages - 1, 10, 18, 21); } while (0);
    __GXData->dirtyState |= 4;
}
