// builds: GC/1.1 GC/2.6
// flags: -O4,p -inline auto -sdata 0 -sdata2 0

typedef struct ReadBuffer {
    unsigned char* ptr;
} ReadBuffer;

typedef struct Player {
    unsigned count;
} Player;

extern Player active_player;

unsigned char* pointer_member_scaled_member_offset(ReadBuffer* read_buffer)
{
    return read_buffer->ptr + active_player.count * 4 + 8;
}
