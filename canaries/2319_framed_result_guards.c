extern int query(int);
extern void observe(int);
int guarded(int x) { int r=query(x); if(r<0)return r; return query(x+1); }
int guarded_constants(void) { int r=query(0); if(r<0)return r; return query(1); }
int twice(int x) { int r=query(x); if(r<0)return r; r=query(x+1); if(r<0)return r; return x; }
int conjunction(int x,int flag) { int r=query(x); if(flag && r<0)return r; return query(x+1); }
int positive(int x) { int r=query(x); if(r>=0)return r; return query(x+1); }
int nested(int x,int flag) { if(flag) { int r=query(x); if(r<0)return r; } return query(x+1); }
int before_effect(int x) { int r=query(x); if(r<0)return r; observe(x); return 0; }
int leaf_guard(int r,int flag) { if(flag && r<0)return r; return r+1; }
int frame_guard(int r,int flag) { int values[2]; values[0]=r; values[1]=flag; if(r<0)return r; observe(values[1]); return values[0]; }
