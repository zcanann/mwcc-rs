// Packet operations retain distinct masks and ABI schedules.
// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned u32;
typedef int GXTevStageID;
typedef int GXIndTexStageID;
typedef int GXIndTexFormat;
typedef int GXIndTexBiasSel;
typedef int GXIndTexMtxID;
typedef int GXIndTexWrap;
typedef int GXIndTexAlphaSel;
typedef unsigned char GXBool;
extern unsigned __rlwimi(unsigned, unsigned, unsigned, unsigned, unsigned);
typedef union { unsigned char byte; unsigned word; } Fifo;
volatile Fifo PORT : 0xCC008000;
struct Context { unsigned short pad, bpSentNot; };
typedef struct Context* ContextPointer;
extern volatile ContextPointer __GXData;
#define SET_REG_FIELD(line, reg, size, shift, val) do { (reg) = (u32)__rlwimi((u32)(reg), (val), (shift), 32-(shift)-(size), 31-(shift)); } while (0)
void volatile_intrinsic(GXTevStageID tev_stage, GXIndTexStageID ind_stage, GXIndTexFormat format,
                      GXIndTexBiasSel bias_sel, GXIndTexMtxID matrix_sel, GXIndTexWrap wrap_s,
                      GXIndTexWrap wrap_t, GXBool add_prev, GXBool utc_lod,
                      GXIndTexAlphaSel alpha_sel)
{
    u32 reg;

    (void)0;
    reg = 0;
    SET_REG_FIELD(148, reg, 2, 0, ind_stage);
    SET_REG_FIELD(149, reg, 2, 2, format);
    SET_REG_FIELD(150, reg, 3, 4, bias_sel);
    SET_REG_FIELD(151, reg, 2, 7, alpha_sel);
    SET_REG_FIELD(152, reg, 4, 9, matrix_sel);
    SET_REG_FIELD(153, reg, 3, 13, wrap_s);
    SET_REG_FIELD(154, reg, 3, 16, wrap_t);
    SET_REG_FIELD(155, reg, 1, 19, utc_lod);
    SET_REG_FIELD(156, reg, 1, 20, add_prev);
    SET_REG_FIELD(157, reg, 8, 24, tev_stage + 16);
    do { PORT.byte = 97; PORT.word = reg; } while (0);
    __GXData->bpSentNot = 0;
}
void explicit_port(GXTevStageID tev_stage, GXIndTexStageID ind_stage, GXIndTexFormat format,
                      GXIndTexBiasSel bias_sel, GXIndTexMtxID matrix_sel, GXIndTexWrap wrap_s,
                      GXIndTexWrap wrap_t, GXBool add_prev, GXBool utc_lod,
                      GXIndTexAlphaSel alpha_sel)
{
    u32 reg;

    (void)0;
    reg = 0;
    SET_REG_FIELD(148, reg, 2, 0, ind_stage);
    SET_REG_FIELD(149, reg, 2, 2, format);
    SET_REG_FIELD(150, reg, 3, 4, bias_sel);
    SET_REG_FIELD(151, reg, 2, 7, alpha_sel);
    SET_REG_FIELD(152, reg, 4, 9, matrix_sel);
    SET_REG_FIELD(153, reg, 3, 13, wrap_s);
    SET_REG_FIELD(154, reg, 3, 16, wrap_t);
    SET_REG_FIELD(155, reg, 1, 19, utc_lod);
    SET_REG_FIELD(156, reg, 1, 20, add_prev);
    SET_REG_FIELD(157, reg, 8, 24, tev_stage + 16);
    do { ((volatile Fifo*)0xCC008008)->byte=97; ((volatile Fifo*)0xCC008008)->word=reg; } while (0);
    __GXData->bpSentNot = 0;
}

#undef SET_REG_FIELD
#define SET_REG_FIELD(line, reg, size, shift, val) do { (reg) = ((reg) & ~(((1u << (size))-1u) << (shift))) | ((u32)(val) << (shift)); } while (0)
void volatile_c(GXTevStageID tev_stage, GXIndTexStageID ind_stage, GXIndTexFormat format,
                      GXIndTexBiasSel bias_sel, GXIndTexMtxID matrix_sel, GXIndTexWrap wrap_s,
                      GXIndTexWrap wrap_t, GXBool add_prev, GXBool utc_lod,
                      GXIndTexAlphaSel alpha_sel)
{
    u32 reg;

    (void)0;
    reg = 0;
    SET_REG_FIELD(148, reg, 2, 0, ind_stage);
    SET_REG_FIELD(149, reg, 2, 2, format);
    SET_REG_FIELD(150, reg, 3, 4, bias_sel);
    SET_REG_FIELD(151, reg, 2, 7, alpha_sel);
    SET_REG_FIELD(152, reg, 4, 9, matrix_sel);
    SET_REG_FIELD(153, reg, 3, 13, wrap_s);
    SET_REG_FIELD(154, reg, 3, 16, wrap_t);
    SET_REG_FIELD(155, reg, 1, 19, utc_lod);
    SET_REG_FIELD(156, reg, 1, 20, add_prev);
    SET_REG_FIELD(157, reg, 8, 24, tev_stage + 16);
    do { PORT.byte = 97; PORT.word = reg; } while (0);
    __GXData->bpSentNot = 0;
}
