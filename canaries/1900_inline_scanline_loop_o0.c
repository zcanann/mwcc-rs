// flags: -lang c
// Reduced from BFBB dolphin GXFrameBuf.c: integer scanline helper.
typedef unsigned int u32;
inline u32 __GXGetNumXfbLines(u32 efbHt, u32 iScale)
{
    u32 count;
    u32 realHt;
    u32 iScaleD;

    count = (efbHt - 1) * 0x100;
    realHt = (count / iScale) + 1;

    iScaleD = iScale;

    if (iScaleD > 0x80 && iScaleD < 0x100)
    {
        while (iScaleD % 2 == 0)
        {
            iScaleD /= 2;
        }

        if (efbHt % iScaleD == 0)
        {
            realHt++;
        }
    }

    if (realHt > 0x400)
    {
        realHt = 0x400;
    }

    return realHt;
}


u32 lines(u32 h, u32 s) { return __GXGetNumXfbLines(h, s); }
u32 assigned_lines(u32 h, u32 s) { u32 result; result = __GXGetNumXfbLines(h, s); return result; }
u32 repeated_lines(u32 h, u32 s) { u32 a; u32 b; a = __GXGetNumXfbLines(h, s); b = __GXGetNumXfbLines(h + 1, s); return a + b; }
