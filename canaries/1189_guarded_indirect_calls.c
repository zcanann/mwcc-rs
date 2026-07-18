// Calls through a checked function-pointer parameter establish the frame before
// the conditional branch. This exposes build 163's already-framed indirect-call
// schedule independently from its linkage-prefix schedule.
typedef void (*GuardedOne)(int);
typedef void (*GuardedTwo)(int, int);

void guarded_indirect_one(GuardedOne call, int a) {
    if (!call) return;
    call(a);
}

void guarded_indirect_two(GuardedTwo call, int a, int b) {
    if (!call) return;
    call(a, b);
}

void guarded_indirect_literals(GuardedTwo call) {
    if (!call) return;
    call(3, 7);
}

void guarded_indirect_mixed(GuardedTwo call, int a) {
    if (!call) return;
    call(a, 7);
}
