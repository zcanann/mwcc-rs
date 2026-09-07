// Dense dispatch must preserve the pointer used by the continuation.
// flags: -Cpp_exceptions off -pragma "cats off"
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
typedef int s32;
typedef signed char s8;
typedef float f32;
typedef int GXIndTexMtxID;
#define GX_ITM_0 1
#define GX_ITM_1 2
#define GX_ITM_2 3
#define GX_ITM_S0 5
#define GX_ITM_S1 6
#define GX_ITM_S2 7
#define GX_ITM_T0 9
#define GX_ITM_T1 10
#define GX_ITM_T2 11
void GXSetIndTexMtx(GXIndTexMtxID mtx_id, const f32 offset[2][3], s8 scale_exp)
{
    s32 mtx[6];
    u32 reg;
    u32 id;

    CHECK_GXBEGIN(186, "GXSetIndTexMtx");

    switch (mtx_id)
    {
    case GX_ITM_0:
    case GX_ITM_1:
    case GX_ITM_2:
        id = mtx_id - 1;
        break;
    case GX_ITM_S0:
    case GX_ITM_S1:
    case GX_ITM_S2:
        id = mtx_id - 5;
        break;
    case GX_ITM_T0:
    case GX_ITM_T1:
    case GX_ITM_T2:
        id = mtx_id - 9;
        break;
    default:
        id = 0;
        break;
    }

    mtx[0] = (int)(1024.0f * offset[0][0]) & 0x7FF;
    mtx[1] = (int)(1024.0f * offset[1][0]) & 0x7FF;
    scale_exp += 17;
    reg = 0;
    SET_REG_FIELD(208, reg, 11, 0, mtx[0]);
    SET_REG_FIELD(209, reg, 11, 11, mtx[1]);
    SET_REG_FIELD(210, reg, 2, 22, scale_exp & 3);
    SET_REG_FIELD(211, reg, 8, 24, id * 3 + 6);
    GX_WRITE_SOME_REG5(GX_LOAD_BP_REG, reg);

    mtx[2] = (int)(1024.0f * offset[0][1]) & 0x7FF;
    mtx[3] = (int)(1024.0f * offset[1][1]) & 0x7FF;
    reg = 0;
    SET_REG_FIELD(217, reg, 11, 0, mtx[2]);
    SET_REG_FIELD(218, reg, 11, 11, mtx[3]);
    SET_REG_FIELD(219, reg, 2, 22, (scale_exp >> 2) & 3);
    SET_REG_FIELD(220, reg, 8, 24, id * 3 + 7);
    GX_WRITE_SOME_REG5(GX_LOAD_BP_REG, reg);

    mtx[4] = (int)(1024.0f * offset[0][2]) & 0x7FF;
    mtx[5] = (int)(1024.0f * offset[1][2]) & 0x7FF;
    reg = 0;
    SET_REG_FIELD(226, reg, 11, 0, mtx[4]);
    SET_REG_FIELD(227, reg, 11, 11, mtx[5]);
    SET_REG_FIELD(228, reg, 2, 22, (scale_exp >> 4) & 3);
    SET_REG_FIELD(229, reg, 8, 24, id * 3 + 8);
    GX_WRITE_SOME_REG5(GX_LOAD_BP_REG, reg);

    __GXData->bpSentNot = 0;
}
