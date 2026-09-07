// The first five BfBB TEV entry points, retaining table and macro structure.
// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned u32;
typedef int GXTevStageID;
typedef int GXTevMode;
typedef int GXTevColorArg;
typedef int GXTevAlphaArg;
typedef int GXTevOp;
typedef int GXTevBias;
typedef int GXTevScale;
typedef int GXTevRegID;
typedef unsigned char GXBool;
struct Context { unsigned short pad, bpSentNot; unsigned char padding[300]; unsigned tevc[16], teva[16]; };
extern struct Context* __GXData;
typedef union { unsigned char byte; unsigned word; } Fifo;
volatile Fifo PORT : 0xCC008000;
extern unsigned __rlwimi(unsigned,unsigned,unsigned,unsigned,unsigned);
#define CHECK_GXBEGIN(line,name) ((void)0)
#define GX_TEVSTAGE0 0
#define SET_REG_FIELD(line,reg,size,shift,value) do { (reg)=__rlwimi((reg),(value),(shift),32-(shift)-(size),31-(shift)); } while(0)
#define GX_WRITE_RAS_REG(value) do { PORT.byte=97; PORT.word=(value); } while(0)
static struct
{
    u32 rid : 8;
    u32 dest : 2;
    u32 shift : 2;
    u32 clamp : 1;
    u32 sub : 1;
    u32 bias : 2;
    u32 sela : 4;
    u32 selb : 4;
    u32 selc : 4;
    u32 seld : 4;
} TEVCOpTableST0[5] = {
    { 192, 0, 0, 1, 0, 0, 15, 8, 10, 15 }, // modulate
    { 192, 0, 0, 1, 0, 0, 10, 8, 9, 15 }, // decal
    { 192, 0, 0, 1, 0, 0, 10, 12, 8, 15 }, // blend
    { 192, 0, 0, 1, 0, 0, 15, 15, 15, 8 }, // replace
    { 192, 0, 0, 1, 0, 0, 15, 15, 15, 10 }, // passclr
};

static struct
{
    u32 rid : 8;
    u32 dest : 2;
    u32 shift : 2;
    u32 clamp : 1;
    u32 sub : 1;
    u32 bias : 2;
    u32 sela : 4;
    u32 selb : 4;
    u32 selc : 4;
    u32 seld : 4;
} TEVCOpTableST1[5] = {
    { 192, 0, 0, 1, 0, 0, 15, 8, 0, 15 }, // modulate
    { 192, 0, 0, 1, 0, 0, 0, 8, 9, 15 }, // decal
    { 192, 0, 0, 1, 0, 0, 0, 12, 8, 15 }, // blend
    { 192, 0, 0, 1, 0, 0, 15, 15, 15, 8 }, // replace
    { 192, 0, 0, 1, 0, 0, 15, 15, 15, 0 }, // passclr
};

static struct
{
    u32 rid : 8;
    u32 dest : 2;
    u32 shift : 2;
    u32 clamp : 1;
    u32 sub : 1;
    u32 bias : 2;
    u32 sela : 3;
    u32 selb : 3;
    u32 selc : 3;
    u32 seld : 3;
    u32 swap : 2;
    u32 mode : 2;
} TEVAOpTableST0[5] = {
    { 193, 0, 0, 1, 0, 0, 7, 4, 5, 7, 0, 0 }, // modulate
    { 193, 0, 0, 1, 0, 0, 7, 7, 7, 5, 0, 0 }, // decal
    { 193, 0, 0, 1, 0, 0, 7, 4, 5, 7, 0, 0 }, // blend
    { 193, 0, 0, 1, 0, 0, 7, 7, 7, 4, 0, 0 }, // replace
    { 193, 0, 0, 1, 0, 0, 7, 7, 7, 5, 0, 0 }, // passclr
};

static struct
{
    u32 rid : 8;
    u32 dest : 2;
    u32 shift : 2;
    u32 clamp : 1;
    u32 sub : 1;
    u32 bias : 2;
    u32 sela : 3;
    u32 selb : 3;
    u32 selc : 3;
    u32 seld : 3;
    u32 swap : 2;
    u32 mode : 2;
} TEVAOpTableST1[5] = {
    { 193, 0, 0, 1, 0, 0, 7, 4, 0, 7, 0, 0 }, // modulate
    { 193, 0, 0, 1, 0, 0, 7, 7, 7, 0, 0, 0 }, // decal
    { 193, 0, 0, 1, 0, 0, 7, 4, 0, 7, 0, 0 }, // blend
    { 193, 0, 0, 1, 0, 0, 7, 7, 7, 4, 0, 0 }, // replace
    { 193, 0, 0, 1, 0, 0, 7, 7, 7, 0, 0, 0 }, // passclr
};

#define SOME_SET_REG_MACRO(reg, size, shift, val)                                                  \
    do                                                                                             \
    {                                                                                              \
        (reg) =                                                                                    \
            (u32)__rlwimi((u32)(reg), (val), (shift), (32 - (shift) - (size)), (31 - (shift)));    \
    } while (0);

void GXSetTevOp(GXTevStageID id, GXTevMode mode)
{
    u32* ctmp;
    u32* atmp;
    u32 tevReg;

    CHECK_GXBEGIN(420, "GXSetTevOp");

    if (id == GX_TEVSTAGE0)
    {
        ctmp = (u32*)TEVCOpTableST0 + mode;
        atmp = (u32*)TEVAOpTableST0 + mode;
    }
    else
    {
        ctmp = (u32*)TEVCOpTableST1 + mode;
        atmp = (u32*)TEVAOpTableST1 + mode;
    }

    tevReg = __GXData->tevc[id];
    tevReg = (*ctmp & ~0xFF000000) | (tevReg & 0xFF000000);
    GX_WRITE_RAS_REG(tevReg);
    __GXData->tevc[id] = tevReg;

    tevReg = __GXData->teva[id];
    tevReg = (*atmp & ~0xFF00000F) | (tevReg & 0xFF00000F);
    GX_WRITE_RAS_REG(tevReg);
    __GXData->teva[id] = tevReg;

    __GXData->bpSentNot = 0;
}

void GXSetTevColorIn(GXTevStageID stage, GXTevColorArg a, GXTevColorArg b, GXTevColorArg c,
                     GXTevColorArg d)
{
    u32 tevReg;

    CHECK_GXBEGIN(578, "GXSetTevColorIn");

    tevReg = __GXData->tevc[stage];
    SET_REG_FIELD(586, tevReg, 4, 12, a);
    SET_REG_FIELD(587, tevReg, 4, 8, b);
    SET_REG_FIELD(588, tevReg, 4, 4, c);
    SET_REG_FIELD(589, tevReg, 4, 0, d);

    GX_WRITE_RAS_REG(tevReg);
    __GXData->tevc[stage] = tevReg;
    __GXData->bpSentNot = 0;
}

void GXSetTevAlphaIn(GXTevStageID stage, GXTevAlphaArg a, GXTevAlphaArg b, GXTevAlphaArg c,
                     GXTevAlphaArg d)
{
    u32 tevReg;

    CHECK_GXBEGIN(614, "GXSetTevAlphaIn");

    tevReg = __GXData->teva[stage];
    SET_REG_FIELD(622, tevReg, 3, 13, a);
    SET_REG_FIELD(623, tevReg, 3, 10, b);
    SET_REG_FIELD(624, tevReg, 3, 7, c);
    SET_REG_FIELD(625, tevReg, 3, 4, d);

    GX_WRITE_RAS_REG(tevReg);
    __GXData->teva[stage] = tevReg;
    __GXData->bpSentNot = 0;
}

void GXSetTevColorOp(GXTevStageID stage, GXTevOp op, GXTevBias bias, GXTevScale scale, GXBool clamp,
                     GXTevRegID out_reg)
{
    u32 tevReg;

    CHECK_GXBEGIN(653, "GXSetTevColorOp");

    tevReg = __GXData->tevc[stage];
    SET_REG_FIELD(663, tevReg, 1, 18, op & 1);
    if (op <= 1)
    {
        SET_REG_FIELD(665, tevReg, 2, 20, scale);
        SET_REG_FIELD(666, tevReg, 2, 16, bias);
    }
    else
    {
        SET_REG_FIELD(668, tevReg, 2, 20, (op >> 1) & 3);
        SET_REG_FIELD(672, tevReg, 2, 16, 3);
    }
    SET_REG_FIELD(672, tevReg, 1, 19, clamp & 0xFF);
    SET_REG_FIELD(673, tevReg, 2, 22, out_reg);

    GX_WRITE_RAS_REG(tevReg);
    __GXData->tevc[stage] = tevReg;
    __GXData->bpSentNot = 0;
}

void GXSetTevAlphaOp(GXTevStageID stage, GXTevOp op, GXTevBias bias, GXTevScale scale, GXBool clamp,
                     GXTevRegID out_reg)
{
    u32 tevReg;

    CHECK_GXBEGIN(699, "GXSetTevAlphaOp");

    tevReg = __GXData->teva[stage];
    SET_REG_FIELD(708, tevReg, 1, 18, op & 1);
    if (op <= 1)
    {
        SET_REG_FIELD(710, tevReg, 2, 20, scale);
        SET_REG_FIELD(711, tevReg, 2, 16, bias);
    }
    else
    {
        SET_REG_FIELD(713, tevReg, 2, 20, (op >> 1) & 3);
        SET_REG_FIELD(717, tevReg, 2, 16, 3);
    }
    SET_REG_FIELD(717, tevReg, 1, 19, clamp & 0xFF);
    SET_REG_FIELD(718, tevReg, 2, 22, out_reg);

    GX_WRITE_RAS_REG(tevReg);
    __GXData->teva[stage] = tevReg;
    __GXData->bpSentNot = 0;
}
