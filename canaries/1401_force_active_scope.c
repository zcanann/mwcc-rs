// `#pragma push`/`pop` scopes force-active object metadata to one definition.
// builds: GC/1.2.5n
// flags: -Cpp_exceptions off -O4,p -inline auto -sdata 0 -sdata2 0
#pragma push
#pragma force_active on
int forced(void) { return 1; }
#pragma pop

int plain(void) { return 2; }
