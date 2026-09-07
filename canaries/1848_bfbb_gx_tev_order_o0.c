// TEV-driven aggregate, select, and shift boundary coverage.
// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned u32;
typedef int GXTevStageID;
typedef int GXTexCoordID;
typedef int GXTexMapID;
typedef int GXChannelID;
struct Context { unsigned short pad, bpSentNot; unsigned char gap0[252]; unsigned tref[8]; unsigned char gap1[1076]; unsigned texmapId[16], tcsManEnab, tevTcEnab; unsigned char gap2[16]; unsigned dirtyState; };
extern struct Context* __GXData;
typedef union { unsigned char byte; unsigned word; } Fifo;
volatile Fifo PORT : 0xCC008000;
#define CHECK_GXBEGIN(line,name) ((void)0)
#define SET_REG_FIELD(line,reg,size,shift,value) do { (reg)=__rlwimi((reg),(value),(shift),32-(shift)-(size),31-(shift)); } while(0)
#define GX_WRITE_RAS_REG(value) do { PORT.byte=97; PORT.word=(value); } while(0)
#define GX_TEX_DISABLE 256
#define GX_MAX_TEXMAP 8
#define GX_TEXMAP0 0
#define GX_MAX_TEXCOORD 8
#define GX_TEXCOORD0 0
#define GX_COLOR_NULL 255
#define GX_TEXMAP_NULL 255
void GXSetTevOrder(GXTevStageID stage, GXTexCoordID coord, GXTexMapID map, GXChannelID color)
{
    u32* ptref;
    u32 tmap;
    u32 tcoord;
    static int c2r[] = { 0, 1, 0, 1, 0, 1, 7, 5, 6 };

    CHECK_GXBEGIN(1131, "GXSetTevOrder");

    ptref = &__GXData->tref[stage / 2];
    __GXData->texmapId[stage] = map;

    tmap = map & ~GX_TEX_DISABLE;
    tmap = (tmap >= GX_MAX_TEXMAP) ? GX_TEXMAP0 : tmap;

    if (coord >= GX_MAX_TEXCOORD)
    {
        tcoord = GX_TEXCOORD0;
        __GXData->tevTcEnab = __GXData->tevTcEnab & ~(1 << stage);
    }
    else
    {
        tcoord = coord;
        __GXData->tevTcEnab = __GXData->tevTcEnab | (1 << stage);
    }

    if (stage & 1)
    {
        SET_REG_FIELD(1158, *ptref, 3, 12, tmap);
        SET_REG_FIELD(1159, *ptref, 3, 15, tcoord);
        SET_REG_FIELD(1161, *ptref, 3, 19, (color == GX_COLOR_NULL) ? 7 : c2r[color]);
        SET_REG_FIELD(1163, *ptref, 1, 18, (map != GX_TEXMAP_NULL && !(map & GX_TEX_DISABLE)));
    }
    else
    {
        SET_REG_FIELD(1166, *ptref, 3, 0, tmap);
        SET_REG_FIELD(1167, *ptref, 3, 3, tcoord);
        SET_REG_FIELD(1169, *ptref, 3, 7, (color == GX_COLOR_NULL) ? 7 : c2r[color]);
        SET_REG_FIELD(1171, *ptref, 1, 6, (map != GX_TEXMAP_NULL && !(map & GX_TEX_DISABLE)));
    }

    GX_WRITE_RAS_REG(*ptref);
    __GXData->bpSentNot = 0;
    __GXData->dirtyState |= 1;
}
