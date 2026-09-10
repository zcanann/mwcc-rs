typedef void (*Callback)(int,int);
extern void fallback(int,int);
extern void alternate(int,int);
extern void consume(Callback);
extern void disturb(void);
struct Slot { unsigned guard; Callback callback; unsigned tail; };
Callback global_callback;
void install(struct Slot* slot, Callback callback) { slot->callback=callback ? callback : fallback; }
void install_reverse(struct Slot* slot, Callback callback) { slot->callback=callback ? fallback : callback; }
void install_two(struct Slot* slot, int enabled) { slot->callback=enabled ? fallback : alternate; }
void install_addressed(struct Slot* slot, Callback callback) { slot->callback=callback ? callback : &fallback; }
void install_cast(struct Slot* slot, Callback callback) { slot->callback=callback ? callback : (Callback)fallback; }
void install_null(struct Slot* slot, int enabled) { slot->callback=enabled ? fallback : 0; }
void install_null_reverse(struct Slot* slot, int enabled) { slot->callback=enabled ? 0 : fallback; }
void install_global(Callback callback) { global_callback=callback ? callback : fallback; }
Callback select_callback(Callback callback) { return callback ? callback : fallback; }
Callback select_reverse(Callback callback) { return callback ? fallback : callback; }
Callback select_two(int enabled) { return enabled ? fallback : alternate; }
Callback select_null(int enabled) { return enabled ? fallback : 0; }
Callback select_null_reverse(int enabled) { return enabled ? 0 : fallback; }
Callback address_value(void) { return fallback; }
Callback explicit_address(void) { return &fallback; }
void pass_selected(Callback callback) { consume(callback ? callback : fallback); }
void install_after_call(struct Slot* slot, Callback callback) { disturb(); slot->callback=callback ? callback : fallback; }
void install_before_call(struct Slot* slot, Callback callback) { slot->callback=callback ? callback : fallback; disturb(); }
Callback shadowed(Callback fallback,Callback alternate,int enabled) { return enabled ? fallback : alternate; }
void shadowed_store(struct Slot* slot,Callback fallback,Callback alternate,int enabled) { slot->callback=enabled ? fallback : alternate; }
