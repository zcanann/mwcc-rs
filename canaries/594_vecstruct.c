typedef struct { float x, y, z; } V3, *V3Ptr; void fsetv3(V3* d, float a, float b, float c){ d->x = a; d->y = b; d->z = c; }
