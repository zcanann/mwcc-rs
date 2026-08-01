// builds: GC/1.1 GC/2.6
// flags: -O4,p -inline auto -sdata 0 -sdata2 0

typedef struct GlobalState {
    int first;
    int second;
    int third;
    int padding[8];
} GlobalState;

extern GlobalState global_state;
extern void barrier(void);

void global_aggregate_base_across_call(int* output)
{
    output[0] = global_state.first;
    barrier();
    output[1] = global_state.second;
    output[2] = global_state.third;
}
