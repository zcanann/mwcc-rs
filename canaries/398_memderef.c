struct P{int a;};
int memderef(int* q,struct P* p){return p->a+*q;}
