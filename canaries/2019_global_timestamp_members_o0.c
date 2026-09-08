// flags: -Cpp_exceptions off -pragma "cats off"
typedef unsigned long long Tick;
struct Profile { Tick start; unsigned count; Tick finish; };
struct Profile profile;
extern Tick clock(void);
extern Tick clock_with_arg(unsigned value);
void stamp(void) { profile.start = clock(); }
void pair(unsigned count) {
    profile.start = clock_with_arg(count);
    profile.count = count;
    profile.finish = clock();
}
