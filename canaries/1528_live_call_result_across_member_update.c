typedef struct State {
    int value;
} State;

extern State state;
extern int disable(void);
extern void restore(int token);

void live_call_result_across_member_update(void) {
    int token = disable();
    state.value++;
    restore(token);
}
