#pragma cplusplus on

typedef float f32;

template <typename T>
struct Vector3 {
    T x, y, z;
    T length() const;
    static Vector3<T> zero;
};

typedef Vector3<f32> Vector3f;

template <typename T>
T unused_passthrough(T value)
{
    return value;
}

inline Vector3f& operator*=(Vector3f& value, f32 scale)
{
    value.x *= scale;
    value.y *= scale;
    value.z *= scale;
    return value;
}

#pragma cplusplus off

f32 first_component(Vector3f* value) { return value->x; }
