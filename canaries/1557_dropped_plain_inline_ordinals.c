// flags: -sym on -Cpp_exceptions off
/* Plain and static dropped bodies share the later frontend's analysis cost. */
inline int discarded_plain(int value) { return value + 3; }
static inline int discarded_static(int value) { return value + 5; }
float emitted_constant(void) { return 2.5f; }
