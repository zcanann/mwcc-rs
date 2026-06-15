// Float both-complex temp now goes through the allocator (a fresh float virtual),
// so a both-complex float expression can compute into the scratch — unlocking
// float negation over it: -((a+b)*(c+d)).
float floatbothcomplexneg(float a, float b, float c, float d){ return -((a + b) * (c + d)); }
