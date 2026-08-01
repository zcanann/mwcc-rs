// builds: GC/1.3.2 GC/2.6
// flags: -O0,p -Cpp_exceptions off -sdata 0 -sdata2 0 -pool off

typedef struct Position {
    float x;
    float y;
} Position;

extern void barrier(void);

float clamp_position(Position* position)
{
    barrier();
    if (position->x < -600.0f) {
        position->x = -600.0f;
    }
    if (position->x > 600.0f) {
        position->x = 600.0f;
    }
    if (position->y < 350.0f) {
        position->y = 350.0f;
    }
    if (position->y > 1200.0f) {
        position->y = 1200.0f;
    }
    return position->x;
}
