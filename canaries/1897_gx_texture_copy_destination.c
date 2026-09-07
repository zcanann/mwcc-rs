// GXFrameBuf-driven discarded values, escaped scalar operands, and argument dependencies.
// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned int u32;
typedef unsigned short u16;
typedef unsigned char GXBool;
typedef int GXTexFmt;
enum { GX_TF_I4=0, GX_TF_I8=1, GX_TF_IA4=2, GX_TF_IA8=3, GX_TF_Z16=19, GX_CTF_YUVA8=38, _GX_TF_ZTF=16 };
typedef struct { unsigned char prefix[504]; u32 cpTexStride; u32 cpTex; unsigned char cpTexZ; } GXData;
extern GXData * const __GXData;
extern void __GetImageTileCount(GXTexFmt, u16, u16, u32 *, u32 *, u32 *);
#define SET_REG_FIELD(line, reg, size, shift, value) ((reg) = (u32)__rlwimi((u32)(reg), (value), (shift), 32-(shift)-(size), 31-(shift)))
void GXSetTexCopyDst(u16 wd, u16 ht, GXTexFmt fmt, GXBool mipmap)
{
    u32 rowTiles;
    u32 colTiles;
    u32 cmpTiles;
    u32 peTexFmt;
    u32 peTexFmtH;


    __GXData->cpTexZ = 0;
    peTexFmt = fmt & 0xF;

    if (fmt == GX_TF_Z16)
    {
        peTexFmt = 0xB;
    }

    switch (fmt)
    {
    case GX_TF_I4:
    case GX_TF_I8:
    case GX_TF_IA4:
    case GX_TF_IA8:
    case GX_CTF_YUVA8:
        SET_REG_FIELD(0, __GXData->cpTex, 2, 15, 3);
        break;
    default:
        SET_REG_FIELD(0, __GXData->cpTex, 2, 15, 2);
        break;
    }

    __GXData->cpTexZ = (fmt & _GX_TF_ZTF) == _GX_TF_ZTF;
    peTexFmtH = (peTexFmt >> 3) & 1;
    !peTexFmt;
    SET_REG_FIELD(1381, __GXData->cpTex, 1, 3, peTexFmtH);
    peTexFmt = peTexFmt & 7;
    __GetImageTileCount(fmt, wd, ht, &rowTiles, &colTiles, &cmpTiles);

    __GXData->cpTexStride = 0;
    SET_REG_FIELD(1390, __GXData->cpTexStride, 10, 0, rowTiles * cmpTiles);
    SET_REG_FIELD(1392, __GXData->cpTexStride, 8, 24, 0x4D);
    SET_REG_FIELD(1392, __GXData->cpTex, 1, 9, mipmap);
    SET_REG_FIELD(1393, __GXData->cpTex, 3, 4, peTexFmt);
}
