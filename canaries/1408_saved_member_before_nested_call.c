// A direct call's first general argument is a member read through a saved
// pointer while its second argument contains another call. MWCC forms the
// nested expression first, then reloads the first member into r3.
typedef struct SavedMember {
    int value;
} SavedMember;

extern int nested_value(int);
extern void consume_pair(int, int);

void saved_member_before_nested_call(SavedMember* object, int saved)
{
    if (object->value != 6) {
        consume_pair(object->value, saved + nested_value(object->value));
    }
}

// builds: GC/1.2.5n
