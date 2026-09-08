// flags:
// Variations of BfBB AXVPB member-value scheduling and address lifetimes.
struct Packet {
    unsigned short state, flag;
    unsigned sync, count, total;
    unsigned short *cursor;
    unsigned short data[5];
    unsigned short on;
};
void two_values(struct Packet *p) { p->state=0; p->flag=0; p->cursor=p->data; p->on=0; }
void pointer_first_two(struct Packet *p) { p->cursor=p->data; p->state=0; p->flag=0; p->on=0; }
void marker_first(struct Packet *p) { p->sync=164; p->state=0; p->flag=0; p->cursor=p->data; p->on=0; }
void pointer_middle(struct Packet *p) { p->state=0; p->cursor=p->data; p->sync=164; p->flag=0; p->on=0; }
void zero_last(struct Packet *p) { p->sync=164; p->cursor=p->data; p->state=0; p->flag=0; p->on=0; }
void marker_last(struct Packet *p) { p->state=0; p->flag=0; p->cursor=p->data; p->on=0; p->sync=164; }
void repeated_marker(struct Packet *p) { p->state=0; p->sync=164; p->count=164; p->cursor=p->data; p->total=164; }
void both_repeated(struct Packet *p) { p->state=0; p->sync=164; p->flag=0; p->count=164; p->cursor=p->data; p->on=0; }
void negative_value(struct Packet *p) { p->state=65535; p->flag=65535; p->sync=0; p->cursor=p->data; p->on=65535; }
void same_target(struct Packet *p) { p->sync=0; p->sync=164; p->cursor=p->data; p->state=0; p->on=0; }
void ordered_first(volatile struct Packet *p) { p->cursor=(unsigned short *)p->data; p->state=0; p->flag=0; p->sync=164; p->on=0; }
void ordered_marker(volatile struct Packet *p) { p->sync=164; p->state=0; p->flag=0; p->cursor=(unsigned short *)p->data; p->on=0; }
void ordered_default(volatile struct Packet *p) { p->state=0; p->flag=0; p->sync=164; p->cursor=(unsigned short *)p->data; p->on=0; }
struct Packet *return_receiver(struct Packet *p) { p->state=0; p->flag=0; p->sync=164; p->cursor=p->data; p->on=0; return p; }
