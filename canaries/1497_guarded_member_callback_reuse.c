// builds: GC/1.2.5n
// flags: -O4,p -inline auto -Cpp_exceptions off -pragma "cats off"

typedef struct Request Request;
typedef void (*Callback)(int, Request*);

struct Request {
    int state;
    Callback callback;
};

extern void ready(void);
extern Request* take_request(void);

void guarded_member_callback_reuse(void)
{
    Request* finished = take_request();
    finished->state = 0;
    if (finished->callback != 0) {
        finished->callback(0, finished);
    }
    ready();
}
