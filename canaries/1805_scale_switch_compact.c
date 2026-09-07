// Distinct C mask/shift and intrinsic scale-switch schedules.
// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned u32;
typedef int GXIndTexStageID;
typedef int GXIndTexScale;
typedef int GXTexCoordID;
typedef int GXTexMapID;
struct Context {
 unsigned short pad, bpSentNot;
 unsigned IndTexScale0, untouched, IndTexScale1, dirtyState;
};
extern struct Context* const __GXData;
typedef union { unsigned char byte; unsigned word; } Fifo;
volatile Fifo PORT : 0xCC008000;
extern unsigned __rlwimi(unsigned, unsigned, unsigned, unsigned, unsigned);
#define CHECK_GXBEGIN(line, name) ((void)0)
#define SET_REG_FIELD(line, reg, size, shift, value) do { (reg) = (u32)__rlwimi((u32)(reg), (value), (shift), 32-(shift)-(size), 31-(shift)); } while (0)
#define GX_WRITE_SOME_REG5(command, data) do { PORT.byte=(command); PORT.word=(data); } while (0)
#define GX_LOAD_BP_REG 0x61
#define GX_INDTEXSTAGE0 0
#define GX_INDTEXSTAGE1 1
#define GX_INDTEXSTAGE2 2
#define GX_INDTEXSTAGE3 3
#define GX_TEXMAP_NULL 255
#define GX_TEXCOORD_NULL 255
#define GX_TEXMAP0 0
#define GX_TEXCOORD0 0
void wrapped(GXIndTexStageID ind_state, GXIndTexScale scale_s, GXIndTexScale scale_t)
{
    CHECK_GXBEGIN(249, "GXSetIndTexScale");

    switch (ind_state)
    {
    case GX_INDTEXSTAGE0:
        SET_REG_FIELD(253, __GXData->IndTexScale0, 4, 0, scale_s);
        SET_REG_FIELD(254, __GXData->IndTexScale0, 4, 4, scale_t);
        SET_REG_FIELD(254, __GXData->IndTexScale0, 8, 24, 0x25);
        GX_WRITE_SOME_REG5(GX_LOAD_BP_REG, __GXData->IndTexScale0);
        break;
    case GX_INDTEXSTAGE1:
        SET_REG_FIELD(259, __GXData->IndTexScale0, 4, 8, scale_s);
        SET_REG_FIELD(260, __GXData->IndTexScale0, 4, 12, scale_t);
        SET_REG_FIELD(260, __GXData->IndTexScale0, 8, 24, 0x25);
        GX_WRITE_SOME_REG5(GX_LOAD_BP_REG, __GXData->IndTexScale0);
        break;
    case GX_INDTEXSTAGE2:
        SET_REG_FIELD(265, __GXData->IndTexScale1, 4, 0, scale_s);
        SET_REG_FIELD(266, __GXData->IndTexScale1, 4, 4, scale_t);
        SET_REG_FIELD(266, __GXData->IndTexScale1, 8, 24, 0x26);
        GX_WRITE_SOME_REG5(GX_LOAD_BP_REG, __GXData->IndTexScale1);
        break;
    case GX_INDTEXSTAGE3:
        SET_REG_FIELD(0x10F, __GXData->IndTexScale1, 4, 8, scale_s);
        SET_REG_FIELD(0x110, __GXData->IndTexScale1, 4, 12, scale_t);
        SET_REG_FIELD(0x110, __GXData->IndTexScale1, 8, 24, 0x26);
        GX_WRITE_SOME_REG5(GX_LOAD_BP_REG, __GXData->IndTexScale1);
        break;
    default:

        break;
    }
    __GXData->bpSentNot = 0;
}
void flat(GXIndTexStageID ind_state, GXIndTexScale scale_s, GXIndTexScale scale_t)
{


    switch (ind_state)
    {
    case GX_INDTEXSTAGE0:
        __GXData->IndTexScale0 = __rlwimi(__GXData->IndTexScale0, scale_s, 0, 28, 31);
        __GXData->IndTexScale0 = __rlwimi(__GXData->IndTexScale0, scale_t, 4, 24, 27);
        __GXData->IndTexScale0 = __rlwimi(__GXData->IndTexScale0, 0x25, 24, 0, 7);
        PORT.byte=97; PORT.word=__GXData->IndTexScale0;
        break;
    case GX_INDTEXSTAGE1:
        __GXData->IndTexScale0 = __rlwimi(__GXData->IndTexScale0, scale_s, 8, 20, 23);
        __GXData->IndTexScale0 = __rlwimi(__GXData->IndTexScale0, scale_t, 12, 16, 19);
        __GXData->IndTexScale0 = __rlwimi(__GXData->IndTexScale0, 0x25, 24, 0, 7);
        PORT.byte=97; PORT.word=__GXData->IndTexScale0;
        break;
    case GX_INDTEXSTAGE2:
        __GXData->IndTexScale1 = __rlwimi(__GXData->IndTexScale1, scale_s, 0, 28, 31);
        __GXData->IndTexScale1 = __rlwimi(__GXData->IndTexScale1, scale_t, 4, 24, 27);
        __GXData->IndTexScale1 = __rlwimi(__GXData->IndTexScale1, 0x26, 24, 0, 7);
        PORT.byte=97; PORT.word=__GXData->IndTexScale1;
        break;
    case GX_INDTEXSTAGE3:
        __GXData->IndTexScale1 = __rlwimi(__GXData->IndTexScale1, scale_s, 8, 20, 23);
        __GXData->IndTexScale1 = __rlwimi(__GXData->IndTexScale1, scale_t, 12, 16, 19);
        __GXData->IndTexScale1 = __rlwimi(__GXData->IndTexScale1, 0x26, 24, 0, 7);
        PORT.byte=97; PORT.word=__GXData->IndTexScale1;
        break;
    default:

        break;
    }
    __GXData->bpSentNot = 0;
}

#define IndTexScale1 IndTexScale0
void aliased(GXIndTexStageID ind_state, GXIndTexScale scale_s, GXIndTexScale scale_t)
{


    switch (ind_state)
    {
    case GX_INDTEXSTAGE0:
        __GXData->IndTexScale0 = __rlwimi(__GXData->IndTexScale0, scale_s, 0, 28, 31);
        __GXData->IndTexScale0 = __rlwimi(__GXData->IndTexScale0, scale_t, 4, 24, 27);
        __GXData->IndTexScale0 = __rlwimi(__GXData->IndTexScale0, 0x25, 24, 0, 7);
        PORT.byte=97; PORT.word=__GXData->IndTexScale0;
        break;
    case GX_INDTEXSTAGE1:
        __GXData->IndTexScale0 = __rlwimi(__GXData->IndTexScale0, scale_s, 8, 20, 23);
        __GXData->IndTexScale0 = __rlwimi(__GXData->IndTexScale0, scale_t, 12, 16, 19);
        __GXData->IndTexScale0 = __rlwimi(__GXData->IndTexScale0, 0x25, 24, 0, 7);
        PORT.byte=97; PORT.word=__GXData->IndTexScale0;
        break;
    case GX_INDTEXSTAGE2:
        __GXData->IndTexScale1 = __rlwimi(__GXData->IndTexScale1, scale_s, 0, 28, 31);
        __GXData->IndTexScale1 = __rlwimi(__GXData->IndTexScale1, scale_t, 4, 24, 27);
        __GXData->IndTexScale1 = __rlwimi(__GXData->IndTexScale1, 0x26, 24, 0, 7);
        PORT.byte=97; PORT.word=__GXData->IndTexScale1;
        break;
    case GX_INDTEXSTAGE3:
        __GXData->IndTexScale1 = __rlwimi(__GXData->IndTexScale1, scale_s, 8, 20, 23);
        __GXData->IndTexScale1 = __rlwimi(__GXData->IndTexScale1, scale_t, 12, 16, 19);
        __GXData->IndTexScale1 = __rlwimi(__GXData->IndTexScale1, 0x26, 24, 0, 7);
        PORT.byte=97; PORT.word=__GXData->IndTexScale1;
        break;
    default:

        break;
    }
    __GXData->bpSentNot = 0;
}
#undef IndTexScale1
