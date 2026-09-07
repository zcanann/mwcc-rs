// Absolute member addresses and dependent pointer arguments.
// flags: -Cpp_exceptions off -pragma "cats off"
typedef float f32;
typedef unsigned long u32;
typedef f32 Mtx[3][4];
typedef int GXTexMtxType;
#define qr0 0
#define GX_MTX3x4 0
#define GX_MTX2x4 1
#define GX_PTTEXMTX0 64
#define CHECK_GXBEGIN(line,name) ((void)0)
typedef union { unsigned char u8; u32 u32; f32 f32; } Fifo;
volatile Fifo GXWGFifo : 0xCC008000;
#define GX_WRITE_U8(v) (GXWGFifo.u8=(v))
#define GX_WRITE_U32(v) (GXWGFifo.u32=(v))
static inline void WriteMTXPS4x3(const register f32 mtx[3][4], register volatile f32* dest)
{
    register f32 a00_a01;
    register f32 a02_a03;
    register f32 a10_a11;
    register f32 a12_a13;
    register f32 a20_a21;
    register f32 a22_a23;

    asm {
        psq_l a00_a01, 0x00(mtx), 0, qr0
        psq_l a02_a03, 0x08(mtx), 0, qr0
        psq_l a10_a11, 0x10(mtx), 0, qr0
        psq_l a12_a13, 0x18(mtx), 0, qr0
        psq_l a20_a21, 0x20(mtx), 0, qr0
        psq_l a22_a23, 0x28(mtx), 0, qr0
        psq_st a00_a01, 0(dest), 0, qr0
        psq_st a02_a03, 0(dest), 0, qr0
        psq_st a10_a11, 0(dest), 0, qr0
        psq_st a12_a13, 0(dest), 0, qr0
        psq_st a20_a21, 0(dest), 0, qr0
        psq_st a22_a23, 0(dest), 0, qr0
    }
}

static inline void WriteMTXPS3x3from3x4(register f32 mtx[3][4], register volatile f32* dest)
{
    register f32 a00_a01;
    register f32 a02_a03;
    register f32 a10_a11;
    register f32 a12_a13;
    register f32 a20_a21;
    register f32 a22_a23;

    asm {
        psq_l  a00_a01, 0x00(mtx), 0, qr0
        lfs    a02_a03, 0x08(mtx)
        psq_l  a10_a11, 0x10(mtx), 0, qr0
        lfs    a12_a13, 0x18(mtx)
        psq_l  a20_a21, 0x20(mtx), 0, qr0
        lfs    a22_a23, 0x28(mtx)
        psq_st a00_a01, 0(dest), 0, qr0
        stfs   a02_a03, 0(dest)
        psq_st a10_a11, 0(dest), 0, qr0
        stfs   a12_a13, 0(dest)
        psq_st a20_a21, 0(dest), 0, qr0
        stfs   a22_a23, 0(dest)
    }
}

static inline void WriteMTXPS4x2(const register f32 mtx[2][4], register volatile f32* dest)
{
    register f32 a00_a01;
    register f32 a02_a03;
    register f32 a10_a11;
    register f32 a12_a13;

    asm {
        psq_l a00_a01, 0x00(mtx), 0, qr0
        psq_l a02_a03, 0x08(mtx), 0, qr0
        psq_l a10_a11, 0x10(mtx), 0, qr0
        psq_l a12_a13, 0x18(mtx), 0, qr0
        psq_st a00_a01, 0(dest), 0, qr0
        psq_st a02_a03, 0(dest), 0, qr0
        psq_st a10_a11, 0(dest), 0, qr0
        psq_st a12_a13, 0(dest), 0, qr0
    }
}

void GXLoadPosMtxImm(Mtx mtx, u32 id)
{
    u32 reg;
    u32 addr;

    CHECK_GXBEGIN(507, "GXLoadPosMtxImm");

    addr = id * 4;
    reg = addr | 0xB0000;

    GX_WRITE_U8(0x10);
    GX_WRITE_U32(reg);
#if DEBUG
    GX_WRITE_MTX_ELEM(addr + 0, mtx[0][0]);
    GX_WRITE_MTX_ELEM(addr + 1, mtx[0][1]);
    GX_WRITE_MTX_ELEM(addr + 2, mtx[0][2]);
    GX_WRITE_MTX_ELEM(addr + 3, mtx[0][3]);
    GX_WRITE_MTX_ELEM(addr + 4, mtx[1][0]);
    GX_WRITE_MTX_ELEM(addr + 5, mtx[1][1]);
    GX_WRITE_MTX_ELEM(addr + 6, mtx[1][2]);
    GX_WRITE_MTX_ELEM(addr + 7, mtx[1][3]);
    GX_WRITE_MTX_ELEM(addr + 8, mtx[2][0]);
    GX_WRITE_MTX_ELEM(addr + 9, mtx[2][1]);
    GX_WRITE_MTX_ELEM(addr + 10, mtx[2][2]);
    GX_WRITE_MTX_ELEM(addr + 11, mtx[2][3]);
#else
    WriteMTXPS4x3(mtx, &GXWGFifo.f32);
#endif
}

void GXLoadNrmMtxImm(Mtx mtx, u32 id)
{
    u32 reg;
    u32 addr;

    CHECK_GXBEGIN(588, "GXLoadNrmMtxImm");

    addr = id * 3 + 0x400;
    reg = addr | 0x80000;

    GX_WRITE_U8(0x10);
    GX_WRITE_U32(reg);
#if DEBUG
    GX_WRITE_MTX_ELEM(addr + 0, mtx[0][0]);
    GX_WRITE_MTX_ELEM(addr + 1, mtx[0][1]);
    GX_WRITE_MTX_ELEM(addr + 2, mtx[0][2]);
    GX_WRITE_MTX_ELEM(addr + 3, mtx[1][0]);
    GX_WRITE_MTX_ELEM(addr + 4, mtx[1][1]);
    GX_WRITE_MTX_ELEM(addr + 5, mtx[1][2]);
    GX_WRITE_MTX_ELEM(addr + 6, mtx[2][0]);
    GX_WRITE_MTX_ELEM(addr + 7, mtx[2][1]);
    GX_WRITE_MTX_ELEM(addr + 8, mtx[2][2]);
#else
    WriteMTXPS3x3from3x4((void*)mtx, &GXWGFifo.f32);
#endif
}

void GXLoadTexMtxImm(f32 mtx[][4], u32 id, GXTexMtxType type)
{
    u32 reg;
    u32 addr;
    u32 count;

    CHECK_GXBEGIN(741, "GXLoadTexMtxImm");

    if (id >= GX_PTTEXMTX0)
    {
        addr = (id - GX_PTTEXMTX0) * 4 + 0x500;
    }
    else
    {
        addr = id * 4;
    }
    count = (type == GX_MTX2x4) ? 8 : 12;
    reg = addr | ((count - 1) << 16);

    GX_WRITE_U8(0x10);
    GX_WRITE_U32(reg);
#if DEBUG
    GX_WRITE_MTX_ELEM(addr + 0, mtx[0][0]);
    GX_WRITE_MTX_ELEM(addr + 1, mtx[0][1]);
    GX_WRITE_MTX_ELEM(addr + 2, mtx[0][2]);
    GX_WRITE_MTX_ELEM(addr + 3, mtx[0][3]);
    GX_WRITE_MTX_ELEM(addr + 4, mtx[1][0]);
    GX_WRITE_MTX_ELEM(addr + 5, mtx[1][1]);
    GX_WRITE_MTX_ELEM(addr + 6, mtx[1][2]);
    GX_WRITE_MTX_ELEM(addr + 7, mtx[1][3]);
    if (type == GX_MTX3x4)
    {
        GX_WRITE_MTX_ELEM(addr + 8, mtx[2][0]);
        GX_WRITE_MTX_ELEM(addr + 9, mtx[2][1]);
        GX_WRITE_MTX_ELEM(addr + 10, mtx[2][2]);
        GX_WRITE_MTX_ELEM(addr + 11, mtx[2][3]);
    }
#else
    if (type == GX_MTX3x4)
    {
        WriteMTXPS4x3(mtx, &GXWGFifo.f32);
    }
    else
    {
        WriteMTXPS4x2(mtx, &GXWGFifo.f32);
    }
#endif
}
