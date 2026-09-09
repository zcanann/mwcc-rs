namespace A { struct State { unsigned head; unsigned tail; } state; }
namespace B { struct State { unsigned pad; unsigned head; unsigned tail; } state; }
extern "C" unsigned read_a(void) { return A::state.head; }
extern "C" unsigned read_b(void) { return B::state.head; }
extern "C" void write_a(unsigned value) { A::state.tail = value; }
extern "C" void write_b(unsigned value) { B::state.tail = value; }
