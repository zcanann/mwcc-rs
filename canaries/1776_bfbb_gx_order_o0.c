// Switch macro and GX context execution coverage.
// flags: -O0 -Cpp_exceptions off -pragma "cats off"
typedef unsigned u32;
typedef int GXIndTexStageID;
typedef int GXIndTexScale;
typedef int GXTexCoordID;
typedef int GXTexMapID;
struct Context {
 unsigned short pad, bpSentNot;
 unsigned char padding[284];
 unsigned iref, bpMask, IndTexScale0, IndTexScale1;
 unsigned char remaining[1148];
 unsigned dirtyState;
};
extern struct Context* const __GXData;
typedef union { unsigned char byte; unsigned word; } Fifo;
extern unsigned __rlwimi(unsigned, unsigned, unsigned, unsigned, unsigned);
#define CHECK_GXBEGIN(line, name) ((void)0)
#define SET_REG_FIELD(line, reg, size, shift, value) do { (reg) = (u32)__rlwimi((u32)(reg), (value), (shift), 32-(shift)-(size), 31-(shift)); } while (0)
#define GX_WRITE_SOME_REG5(command, data) do { ((volatile Fifo*)0xCC008000)->byte=(command); ((volatile Fifo*)0xCC008000)->word=(data); } while (0)
#define GX_LOAD_BP_REG 0x61
#define GX_INDTEXSTAGE0 0
#define GX_INDTEXSTAGE1 1
#define GX_INDTEXSTAGE2 2
#define GX_INDTEXSTAGE3 3
#define GX_TEXMAP_NULL 255
#define GX_TEXCOORD_NULL 255
#define GX_TEXMAP0 0
#define GX_TEXCOORD0 0
void GXSetIndTexOrder(GXIndTexStageID ind_stage, GXTexCoordID tex_coord, GXTexMapID tex_map)
{
    CHECK_GXBEGIN(302, "GXSetIndTexOrder");

    if (tex_map == GX_TEXMAP_NULL)
    {
        tex_map = GX_TEXMAP0;
    }

    if (tex_coord == GX_TEXCOORD_NULL)
    {
        tex_coord = GX_TEXCOORD0;
    }

    switch (ind_stage)
    {
    case GX_INDTEXSTAGE0:
        SET_REG_FIELD(319, __GXData->iref, 3, 0, tex_map);
        SET_REG_FIELD(320, __GXData->iref, 3, 3, tex_coord);
        break;
    case GX_INDTEXSTAGE1:
        SET_REG_FIELD(323, __GXData->iref, 3, 6, tex_map);
        SET_REG_FIELD(324, __GXData->iref, 3, 9, tex_coord);
        break;
    case GX_INDTEXSTAGE2:
        SET_REG_FIELD(327, __GXData->iref, 3, 12, tex_map);
        SET_REG_FIELD(328, __GXData->iref, 3, 15, tex_coord);
        break;
    case GX_INDTEXSTAGE3:
        SET_REG_FIELD(331, __GXData->iref, 3, 18, tex_map);
        SET_REG_FIELD(332, __GXData->iref, 3, 21, tex_coord);
        break;
    default:
        break;
    }
    GX_WRITE_SOME_REG5(GX_LOAD_BP_REG, __GXData->iref);
    __GXData->dirtyState |= 3;
    __GXData->bpSentNot = 0;
}
