// Declarations that emit no code or data do not cause debug sections by
// themselves; a typedef-only unit retains the ordinary comment-only object.
// builds: GC/1.1 GC/1.3.2 GC/2.6
// flags: -sym on
typedef int Word;
