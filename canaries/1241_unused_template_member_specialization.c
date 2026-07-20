// builds: GC/1.1 GC/1.1p1 GC/1.2.5 GC/1.2.5n GC/1.3 GC/1.3.2 GC/2.0 GC/2.0p1 GC/2.6 GC/2.7 GC/3.0a3
#pragma cplusplus on

typedef float f32;

template <int N, typename T>
struct Table {
    T get(int) const { return 0.0f; }
};

f32 Table<8, f32>::get(int value) const { return 1.0f; }

#pragma cplusplus off

int compiled(void) { return 3; }
