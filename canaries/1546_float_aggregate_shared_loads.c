/* Repeated ordinary inputs are shared before aliased output stores. */
struct Pair { float x, y; };
void shared_snapshot(float* destination, const float* source, float x, float y) {
    struct Pair temporary;
    temporary.x = source[0] * x;
    temporary.y = source[0] * y;
    destination[0] = temporary.x;
    destination[1] = temporary.y;
}
