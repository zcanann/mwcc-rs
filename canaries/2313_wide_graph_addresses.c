typedef unsigned long long u64;
struct Record { unsigned char pad[3], flag; short left, right; int count; u64 stamp; };
extern void alter(struct Record *);
u64 member_sum(u64 seed, struct Record *p) {
    return seed + p->left + p->right + p->count + p->stamp;
}
void member_write(u64 value, struct Record *p) {
    p->left=(short)value; p->right=(short)(value>>16);
    p->stamp=value; p->count=(int)(value>>32);
}
u64 indexed_pair(u64 seed, struct Record *p, unsigned i) {
    return seed + p[i].stamp + p[i].count;
}
u64 big_displacements(u64 seed, char *p) {
    return seed + *(int *)(p+32764) + *(int *)(p+32768) + *(int *)(p-32768);
}
u64 pair_boundary(u64 seed, char *p) { return seed + *(u64 *)(p+32764); }
u64 saved_base(u64 seed, int *p, int *q, unsigned n) {
    int *saved=p+2;
    p=q;
    seed=seed+*saved;
    while(n) { seed=seed+*saved; p=p+1; n=n-1; }
    return seed+*p;
}
u64 frame_members(u64 seed, struct Record *p) {
    struct Record local;
    local.left=p->left; local.right=p->right; local.count=7; local.stamp=seed;
    alter(&local);
    return local.stamp+local.left+local.right+local.count;
}
u64 branch_base(u64 seed, struct Record *p, struct Record *q, int pick) {
    struct Record *r=p;
    if(pick) r=q;
    return seed+r->stamp+r->count;
}
u64 volatile_members(u64 seed, volatile struct Record *p) {
    return seed+p->left+p->stamp;
}
u64 address_escape(u64 seed, struct Record *p) {
    alter(p+1);
    return seed+p[1].stamp;
}
