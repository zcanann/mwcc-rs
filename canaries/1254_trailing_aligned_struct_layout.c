typedef struct {
    unsigned char** animation;
    int count;
} __attribute__((aligned(32))) TextureAnimation;

typedef struct {
    int value;
} PostAligned __attribute__((aligned(32)));

int pre_alias_aligned_size(void) { return sizeof(TextureAnimation); }
int post_alias_aligned_size(void) { return sizeof(PostAligned); }
// builds: GC/1.1 GC/1.1p1 GC/1.2.5 GC/1.2.5n GC/1.3 GC/1.3.2 GC/2.0 GC/2.0p1 GC/2.6 GC/2.7 GC/3.0a3 GC/3.0a3p1 Wii/1.0
