// A fused multiply-add whose addend is a both-complex expression. This unlocked
// for free once the fits_single_scratch gate was relaxed and the float
// both-complex temp went through the allocator — the addend computes into the
// scratch, then fmadds. No FMA-specific change was needed.
float fmacomplexaddend(float a, float b, float c, float d, float e, float g){ return a * b + (c + d) * (e + g); }
