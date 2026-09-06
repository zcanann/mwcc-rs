/* An intervening output store invalidates shared input reads. */
struct Pair { float x, y; };
void alias_snapshot(float* destination, const float* source, float x, float y, float z) {
    struct Pair temporary;
    temporary.x = source[0] * x;
    temporary.y = source[0] * y;
    destination[0] = temporary.x;
    temporary.x = source[0] * z;
    destination[1] = temporary.y;
    destination[2] = temporary.x;
}
