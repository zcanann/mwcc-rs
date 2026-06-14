int g(int, int);
int call2deref(int* p, int* q){ return g(*p, *q); }
