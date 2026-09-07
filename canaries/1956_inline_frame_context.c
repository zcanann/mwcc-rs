// flags: -Cpp_exceptions off -pragma "cats off"
struct Context { double alignment; unsigned words[176]; };
extern void clear_context(struct Context*, unsigned);
extern void set_context(struct Context*);
extern void observe(unsigned);
extern void (*callback)(void);
inline void dispatch_context(struct Context* prior, unsigned token) {
    struct Context temporary;
    if (callback) {
        clear_context(&temporary, token);
        set_context(&temporary);
        callback();
        clear_context(&temporary, token + 1);
        set_context(prior);
    }
}
void dispatch(struct Context* prior, unsigned token) {
    observe(token);
    dispatch_context(prior, token);
    observe(token + 2);
}
void repeated(struct Context* prior, unsigned token) {
    dispatch_context(prior, token);
    dispatch_context(prior, token);
    observe(token + 4);
}
