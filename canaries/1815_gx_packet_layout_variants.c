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
volatile Fifo PORT : 0xCC008004;
struct Context { unsigned short pad[7], bpSentNot; };
extern struct Context* __GXData;
#define SET_REG_FIELD(line, reg, size, shift, val) do { (reg) = (u32)__rlwimi((u32)(reg), (val), (shift), 32-(shift)-(size), 31-(shift)); } while (0)
void initialized(GXTevStageID tev_stage, GXIndTexStageID ind_stage, GXIndTexFormat format,
                      GXIndTexBiasSel bias_sel, GXIndTexMtxID matrix_sel, GXIndTexWrap wrap_s,
                      GXIndTexWrap wrap_t, GXBool add_prev, GXBool utc_lod,
                      GXIndTexAlphaSel alpha_sel)
{
    u32 reg = 0;

    (void)0;
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
void flat(GXTevStageID tev_stage, GXIndTexStageID ind_stage, GXIndTexFormat format,
                      GXIndTexBiasSel bias_sel, GXIndTexMtxID matrix_sel, GXIndTexWrap wrap_s,
                      GXIndTexWrap wrap_t, GXBool add_prev, GXBool utc_lod,
                      GXIndTexAlphaSel alpha_sel)
{
    u32 reg = 0;

    reg = __rlwimi(reg, ind_stage, 0, 30, 31);
    reg = __rlwimi(reg, format, 2, 28, 29);
    reg = __rlwimi(reg, bias_sel, 4, 25, 27);
    reg = __rlwimi(reg, alpha_sel, 7, 23, 24);
    reg = __rlwimi(reg, matrix_sel, 9, 19, 22);
    reg = __rlwimi(reg, wrap_s, 13, 16, 18);
    reg = __rlwimi(reg, wrap_t, 16, 13, 15);
    reg = __rlwimi(reg, utc_lod, 19, 12, 12);
    reg = __rlwimi(reg, add_prev, 20, 11, 11);
    reg = __rlwimi(reg, tev_stage + 16, 24, 0, 7);
    PORT.byte=42; PORT.word=reg;
    __GXData->bpSentNot = 0;
}
