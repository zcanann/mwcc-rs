/* All source reads precede aliased destination stores after scalar replacement. */
struct Pair { float x, y; };
void aggregate_snapshot(float* destination, const float* source, float scale) {
    struct Pair temporary;
    temporary.x = source[0] * scale;
    temporary.y = source[1] * scale;
    destination[0] = temporary.y;
    destination[1] = temporary.x;
}
