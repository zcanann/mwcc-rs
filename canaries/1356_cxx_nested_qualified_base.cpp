struct Collision
{
    struct tri_data
    {
        unsigned index;
        float radius;
        float distance;
    };
};

struct Drive
{
    struct tri_data : Collision::tri_data
    {
        float x;
    };

    unsigned flags;
    float time;
    tri_data tri;
};

float read_drive_x(Drive* drive)
{
    return drive->tri.x;
}
