// A file-scope int defined here (not extern) lands in .sbss as a defined OBJECT
// symbol; the load relocates against it via EMB_SDA21. Reloc-exact (the .comment
// for a relocated object is not byte-reproduced yet — see task #2).
int counter;
int get_counter(void){ return counter; }
