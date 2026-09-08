// flags: -O0
// BSS page boundaries, reduced from BfBB __AXVPBInit's clearing loops.
static unsigned char before[32752];
static unsigned char window[16];
static unsigned char boundary[32];
static unsigned char gap[32736];
static unsigned char next_page[32];
static unsigned char gap2[32736];
static unsigned char next_half[32];
extern void sink(void *);
void *result;
void publish(void) {
    sink(before); sink(window); sink(boundary); sink(gap);
    sink(next_page); sink(gap2); sink(next_half);
}
void clear(void) {
    unsigned char *p; int i;
    p=boundary; for(i=32;i!=0;i--) { *p=0; p++; }
    p=next_page; for(i=32;i!=0;i--) { *p=7; p++; }
    p=next_half; for(i=32;i!=0;i--) { *p=11; p++; }
    sink(before);
}
void conditional(int yes) {
    sink(before);
    if(yes) sink(next_half); else sink(boundary);
    sink(next_page);
}
void repeated(int count) {
    int i; for(i=0;i<count;i++) sink(boundary);
    sink(before); sink(next_half);
}
void *get_late(void) { sink(before); return next_half; }
void publish_pointer(void) { sink(before); result=next_half; sink(next_page); }
void local_address(void) {
    static unsigned char local[32];
    sink(before); sink(local); sink(next_half);
}
static unsigned char after_local[32];
void later(void) { sink(before); sink(after_local); }
