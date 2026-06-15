// extern is an undefined reference; the non-extern definition is placed in .sbss.
extern int outside;
int inside;
int mix(void){ return outside + inside; }
