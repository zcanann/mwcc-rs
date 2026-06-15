// An initialized scalar global lands in .sdata holding its value (here 1), as a
// defined OBJECT symbol; the load relocates against it via EMB_SDA21.
int seed = 1;
int getseed(void){ return seed; }
