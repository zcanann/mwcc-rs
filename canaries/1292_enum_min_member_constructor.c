// `-enum min` narrows a small nonnegative enum to one byte. In a constructor-
// style member run, the dead enum argument register is reused for the first
// constant while the second constant occupies r0.
// builds: GC/1.1p1
// flags: -enum min -pool off -str readonly -sdatathreshold 0
typedef enum EventKind {
    EventNull = 0,
    EventRequest = 2,
    EventSupport = 5
} EventKind;

typedef struct Event {
    EventKind kind;
    unsigned id;
    int message;
} Event;

void construct_event(Event* event, EventKind kind) {
    event->kind = kind;
    event->id = 0;
    event->message = -1;
}

int event_kind(const Event* event) {
    return event->kind;
}
