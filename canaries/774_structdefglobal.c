// A struct definition with trailing declarators in one statement
// (`static struct OSAlarmQueue { ... } AlarmQueue;`) declares struct-valued
// globals carrying the tag, so `AlarmQueue.head` resolves the member layout. The
// parser previously registered the layout only for a bare `struct T { ... };` and
// dropped the trailing variable, deferring the member access on an untagged base.
struct Single { int a; int b; } one;
static struct AlarmQueue { int head; int tail; } Queue;
int read_one(void)   { return one.b; }
int read_queue(void) { return Queue.head; }
