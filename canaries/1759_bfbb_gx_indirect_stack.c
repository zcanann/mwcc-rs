// BfBB GXSetTevIndirect packet reduction, compared with linked FIFO writes.
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
// Spell the original constant-mask rlwimi operations as their C value expression.
#define SET_REG_FIELD(line, reg, size, shift, val) ((reg) = ((reg) & ~(((1u << (size)) - 1) << (shift))) | (((u32)(val) & ((1u << (size)) - 1)) << (shift)))
unsigned gx_indirect_word(GXTevStageID tev_stage, GXIndTexStageID ind_stage, GXIndTexFormat format,
                      GXIndTexBiasSel bias_sel, GXIndTexMtxID matrix_sel, GXIndTexWrap wrap_s,
                      GXIndTexWrap wrap_t, GXBool add_prev, GXBool utc_lod,
                      GXIndTexAlphaSel alpha_sel)
{
    u32 reg;

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
    return reg;
}
