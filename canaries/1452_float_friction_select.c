typedef struct Fighter {
    char pad0[228];
    float acceleration;
    char pad1[4];
    float velocity;
} Fighter;

#define ABS(value) ((value) < 0 ? -(value) : (value))

void apply_friction(Fighter* fighter, float friction)
{
    if (ABS(friction) > ABS(fighter->velocity)) {
        friction = -fighter->velocity;
    } else if (fighter->velocity > 0) {
        friction = -friction;
    }
    fighter->acceleration = friction;
}
