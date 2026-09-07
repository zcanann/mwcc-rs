// flags: -Cpp_exceptions off -pragma "cats off"
// Reduced directly from BFBB dolphin GXFrameBuf.c.
typedef unsigned int u32;
typedef unsigned short u16;
typedef float f32;
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

f32 GXGetYScaleFactor(u16 efbHeight, u16 xfbHeight)
{
    f32 fScale;
    f32 yScale;
    u32 iScale;
    u32 tgtHt;
    u32 realHt;

    tgtHt = xfbHeight;
    yScale = (f32)xfbHeight / (f32)efbHeight;
    iScale = (u32)(256.0f / yScale) & 0x1FF;
    realHt = __GXGetNumXfbLines(efbHeight, iScale);

    while (realHt > xfbHeight)
    {
        tgtHt--;
        yScale = (f32)tgtHt / (f32)efbHeight;
        iScale = (u32)(256.0f / yScale) & 0x1FF;
        realHt = __GXGetNumXfbLines(efbHeight, iScale);
    }

    fScale = yScale;
    while (realHt < xfbHeight)
    {
        fScale = yScale;
        tgtHt++;
        yScale = (f32)tgtHt / (f32)efbHeight;
        iScale = (u32)(256.0f / yScale) & 0x1FF;
        realHt = __GXGetNumXfbLines(efbHeight, iScale);
    }

    return fScale;
}
