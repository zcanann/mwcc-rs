// Legacy C++ DWARF retains statement/epilogue line rows, drops unused
// parameters, and describes a pointer kept across a call in r31.
// builds: GC/1.3.2
// flags: -sym on -sdata 0 -sdata2 0 -Cpp_exceptions off -inline auto,deferred

typedef unsigned int word;

extern "C" {
extern word acquire_lock(void);
extern int clear_lock(word value);

void ignore_lock(word* lock)
{
}

void save_acquired_lock(word* lock)
{
    *lock = acquire_lock();
}

void clear_saved_lock(word* lock)
{
    clear_lock(*lock);
}

void shutdown_locks(void)
{
}
}
