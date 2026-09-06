// flags: -Cpp_exceptions off -pragma "cats off"
// EXI interrupt initialization publishes one callback before registering another.
typedef void (*Callback)(int, void*);
extern Callback published;
void callback(int, void*);
void handler(int, void*);
void mask(unsigned);
Callback install(short, Callback);
void unmask(unsigned);

int initialize(void) {
    mask(0x18000);
    mask(0x40);
    published = callback;
    install(25, handler);
    unmask(0x40);
}

void register_pair(void) {
    published = callback;
    install(7, handler);
    unmask(4);
}
