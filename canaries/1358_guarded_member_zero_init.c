typedef struct Drive
{
    unsigned flags;
    float old_time;
    float old_timer;
    float old_scale;
    float time;
    float timer;
    float scale;
    void* old_driver;
    void* driver;
    void* driven;
} Drive;

void initialize_drive(Drive* drive, void* driven)
{
    if (drive == 0)
    {
        return;
    }

    drive->flags = 0;
    drive->driven = driven;
    drive->driver = 0;
    drive->scale = 0.0f;
    drive->time = 0.0f;
    drive->timer = 0.0f;
    drive->old_driver = 0;
    drive->old_scale = 0.0f;
    drive->old_time = 0.0f;
    drive->old_timer = 0.0f;
}
