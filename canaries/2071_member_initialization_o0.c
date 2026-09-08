// flags: -O0
// Reduced from the initialization pattern in BfBB dolphin AXVPB.c.
struct Packet {
    unsigned short state, flag;
    unsigned sync, count, total;
    unsigned short *cursor;
    unsigned short data[5];
    unsigned short on;
};
void defaults(struct Packet *p) {
    p->state=0; p->flag=0; p->sync=164;
    p->total=p->count=0; p->cursor=p->data;
    p->data[0]=p->data[1]=p->data[2]=p->data[3]=p->data[4]=0;
    p->on=0;
}
void nonzero(struct Packet *p) {
    p->state=7; p->flag=7; p->sync=93;
    p->total=p->count=7; p->cursor=p->data+2;
    p->data[0]=p->data[1]=p->data[2]=p->data[3]=p->data[4]=7;
    p->on=7;
}
void address_first(struct Packet *p) {
    p->cursor=p->data; p->state=0; p->flag=0; p->sync=164;
    p->total=p->count=0; p->on=0;
}
void constants_only(struct Packet *p) {
    p->state=0; p->flag=0; p->sync=164;
    p->total=p->count=0; p->data[0]=p->data[4]=0; p->on=0;
}
void indexed(struct Packet *p) {
    p->data[0]=1; p->data[2]=2; p->data[4]=3;
}
void dynamic(struct Packet *p, int index) {
    p->data[index]=7; p->on=0;
}
void second_argument(int unused, struct Packet *p) {
    p->state=0; p->flag=0; p->sync=164;
    p->total=p->count=0; p->cursor=p->data; p->on=0;
}
void guarded(struct Packet *p, int enabled) {
    if (enabled) { p->state=0; p->flag=0; p->sync=164;
        p->total=p->count=0; p->cursor=p->data; p->on=0; }
}
void ordered(volatile struct Packet *p) {
    p->state=0; p->flag=0; p->sync=164;
    p->count=0; p->total=0; p->cursor=(unsigned short *)p->data; p->on=0;
}
void address_plus(struct Packet *p) { p->cursor=p->data+4; }
void address_commuted(struct Packet *p) { p->cursor=3+p->data; }
void address_subtract(struct Packet *p) { p->cursor=p->data-(-1); }
