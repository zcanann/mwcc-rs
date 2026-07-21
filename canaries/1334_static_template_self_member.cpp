// builds: GC/1.3 GC/1.3.2 GC/2.0 GC/2.0p1 GC/2.6 GC/2.7

template <typename T>
struct Vector3 {
    T x, y, z;
    static Vector3<T> zero;
};

int read_z(Vector3<int>* value) {
    return value->z;
}
