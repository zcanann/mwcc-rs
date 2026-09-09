typedef unsigned long long U64;
struct Context { unsigned words[178]; };
typedef U64 (*Callback)(struct Context *, unsigned);
extern void fill_context(struct Context *, unsigned);
extern unsigned read_context(struct Context *);
U64 context_callback(Callback callback, U64 value, unsigned count) {
    struct Context context;
    U64 result;
    fill_context(&context, count);
    result = callback(&context, count);
    return value + result + read_context(&context);
}
U64 local_callback(Callback callback, U64 value, unsigned count) {
    Callback selected;
    struct Context context;
    selected = callback;
    fill_context(&context, count);
    value = value + selected(&context, count);
    return value + read_context(&context);
}
