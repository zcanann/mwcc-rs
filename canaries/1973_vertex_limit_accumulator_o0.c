// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned char u8;
typedef unsigned u32;
typedef unsigned GXCompCnt;
struct VertexState { unsigned short vNum, vLim; unsigned vcdLo, vcdHi, vatA[1]; };
extern struct VertexState* vertex_state;
#define GET_REG_FIELD(reg, size, shift) (((reg) >> (shift)) & ((1 << (size)) - 1))
void calculate_limit(void)
{
    static u8 tbl1[] = { 0, 4, 1, 2 };
    static u8 tbl2[] = { 0, 8, 1, 2 };
    static u8 tbl3[] = { 0, 12, 1, 2 };

    GXCompCnt nc = 0;
    u32 vlm;
    u32 b;
    u32 vl;
    u32 vh;
    u32 va;

    if (vertex_state->vNum != 0)
    {
        vl = vertex_state->vcdLo;
        vh = vertex_state->vcdHi;
        va = vertex_state->vatA[0];
        nc = GET_REG_FIELD(va, 1, 9);

        vlm = GET_REG_FIELD(vl, 1, 0);
        vlm += (u8)GET_REG_FIELD(vl, 1, 1);
        vlm += (u8)GET_REG_FIELD(vl, 1, 2);
        vlm += (u8)GET_REG_FIELD(vl, 1, 3);
        vlm += (u8)GET_REG_FIELD(vl, 1, 4);
        vlm += (u8)GET_REG_FIELD(vl, 1, 5);
        vlm += (u8)GET_REG_FIELD(vl, 1, 6);
        vlm += (u8)GET_REG_FIELD(vl, 1, 7);
        vlm += (u8)GET_REG_FIELD(vl, 1, 8);
        vlm += tbl3[(u8)GET_REG_FIELD(vl, 2, 9)];

        if (nc == 1)
        {
            b = 3;
        }
        else
        {
            b = 1;
        }

        vlm += tbl3[(u8)GET_REG_FIELD(vl, 2, 11)] * b;
        vlm += tbl1[(u8)GET_REG_FIELD(vl, 2, 13)];
        vlm += tbl1[(u8)GET_REG_FIELD(vl, 2, 15)];
        vlm += tbl2[(u8)GET_REG_FIELD(vh, 2, 0)];
        vlm += tbl2[(u8)GET_REG_FIELD(vh, 2, 2)];
        vlm += tbl2[(u8)GET_REG_FIELD(vh, 2, 4)];
        vlm += tbl2[(u8)GET_REG_FIELD(vh, 2, 6)];
        vlm += tbl2[(u8)GET_REG_FIELD(vh, 2, 8)];
        vlm += tbl2[(u8)GET_REG_FIELD(vh, 2, 10)];
        vlm += tbl2[(u8)GET_REG_FIELD(vh, 2, 12)];
        vlm += tbl2[(u8)GET_REG_FIELD(vh, 2, 14)];
        vertex_state->vLim = vlm;
    }
}
