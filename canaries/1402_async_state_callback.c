// Build 163's DVD-style asynchronous state callback. The request pointer
// survives calls in both switch arms and the retry path; the outer comparison
// occupies the linkage prologue's first latency slot.
typedef int s32;
typedef unsigned int u32;

typedef struct Request {
    u32 command;
    u32 offset;
    s32 length;
    u32 maximum_length;
    void *address;
} Request;

extern s32 state;
extern Request *request;
extern void *disk_id;
extern void reset_device(void);
extern void read_async(Request *, void *, s32, u32, void (*)(s32, Request *));
extern void read_id(Request *, void *, void (*)(s32, Request *));

static void callback(s32 result, Request *command)
{
    if (result > 0) {
        switch (state) {
        case 0:
            state = 1;
            read_async(command, request, 32, 1056, callback);
            break;
        case 1:
            state = 2;
            read_async(command, request->address,
                       ((u32)request->length + 31) & ~31,
                       request->offset, callback);
            break;
        }
    } else if (result == -1) {
    } else if (result == -4) {
        state = 0;
        reset_device();
        read_id(command, disk_id, callback);
    }
}

void *callback_address(void)
{
    return callback;
}
