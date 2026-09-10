extern int query(int);
extern void observe(int);
int guarded(int x) { int r=query(x); if(r<0)return r; return query(x+1); }
int nested(int x,int flag) { if(flag) { int r=query(x); if(r<0)return r; } return query(x+1); }
int conjunction(int x,int flag) { int r=query(x); if(flag && r<0)return r; return query(x+1); }
int either(int x,int flag) { int r=query(x); if(flag || r<0)return r; return query(x+1); }
int twice(int x) { int r=query(x); if(r<0)return r; r=query(x+1); if(r<0)return r; return x; }
int three(int x,int flag,int limit) { int r=query(x); if(flag && r<limit)return r; return query(x+1); }
int guard_after(int x,int flag) { int r=query(x); if(r<0)return flag; return query(x+1); }
int flag_twice(int x,int flag) { int r=query(x); if(flag && r<0)return flag; return query(x+1); }
int single_argument(int x) { int r=query(x); if(r<0)return r; return r+1; }
int constants(void) { int r=query(0); if(r<0)return r; return query(1); }
int before_effect(int x) { int r=query(x); if(r<0)return r; observe(x); return 0; }
struct Record { int first,middle,last; };
extern struct Record records[16];
void indexed_argument(int i) { observe(records[i].last); }
void indexed_next(int i) { observe(records[i+1].last); }
int indexed_guard(int x,int i) { int r=query(x); if(r<0)return records[i].last; return query(x+1); }
