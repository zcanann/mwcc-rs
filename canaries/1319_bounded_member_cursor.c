// builds: GC/1.1p1

typedef struct Cursor {
    int lock;
    int used;
    unsigned int length;
    unsigned int position;
} Cursor;

int set_position(Cursor* cursor, unsigned int position)
{
    int error = 0;

    if (position > 2176) {
        error = 769;
    } else {
        cursor->position = position;
        if (position > cursor->length) {
            cursor->length = position;
        }
    }

    return error;
}
