// flags: -Cpp_exceptions off -pragma "cats off"
typedef void (*Callback)(int value, void* context);
extern Callback callback;
extern void (*empty_callback)(void);

void invoke_empty(void) {
    if (empty_callback) empty_callback();
}

void invoke_forwarded(int value, void* context) {
    if (callback) callback(value, context);
}

void invoke_constant(int value, void* context) {
    if (callback != 0) callback(7, context);
}

void invoke_after_guard(int value, void* context) {
    if (!callback) return;
    callback(value, context);
}
