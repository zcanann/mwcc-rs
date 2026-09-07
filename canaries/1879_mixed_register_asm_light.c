// Mixed integer and paired-single assembly register locals.
// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned long u32;
typedef float f32;
typedef struct { unsigned int reserved[4]; float values[12]; } GXLightObj;
static inline void PushLight(const register GXLightObj* lt_obj, register void* dest)
{
    register u32 zero, color;
    register f32 a0_a1, a2_k0, k1_k2;
    register f32 px_py, pz_dx, dy_dz;
#ifdef __MWERKS__ // clang-format off
	asm {
		lwz     color, 12(lt_obj)
		xor     zero, zero, zero
		psq_l   a0_a1, 16(lt_obj), 0, 0
		psq_l   a2_k0, 24(lt_obj), 0, 0
		psq_l   k1_k2, 32(lt_obj), 0, 0
		psq_l   px_py, 40(lt_obj), 0, 0
		psq_l   pz_dx, 48(lt_obj), 0, 0
		psq_l   dy_dz, 56(lt_obj), 0, 0

		stw     zero,  0(dest)
		stw     zero,  0(dest)
		stw     zero,  0(dest)
		stw     color, 0(dest)
		psq_st  a0_a1, 0(dest), 0, 0
		psq_st  a2_k0, 0(dest), 0, 0
		psq_st  k1_k2, 0(dest), 0, 0
		psq_st  px_py, 0(dest), 0, 0
		psq_st  pz_dx, 0(dest), 0, 0
		psq_st  dy_dz, 0(dest), 0, 0
	}
#endif // clang-format on
}

void copy_light(GXLightObj *source, void *destination) { PushLight(source, destination); }
void fifo_light(GXLightObj *source) { PushLight(source, (void*)0xcc008000); }
