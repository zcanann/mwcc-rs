typedef unsigned long long u64;
typedef long long s64;
extern u64 bump(u64);
u64 plain(u64 u,u64 v,int n,u64*out) { return u+n; }
u64 chain(u64 u,u64 v,int n,u64*out) { return u+(int)v+n; }
u64 pair_chain(u64 u,u64 v,int n,u64*out) { return (u+v)+n; }
u64 commuted(u64 u,u64 v,int n,u64*out) { return n+(u+v); }
u64 sub_chain(u64 u,u64 v,int n,u64*out) { return (u-v)+n; }
u64 mul_chain(u64 u,u64 v,int n,u64*out) { return (u*v)+n; }
u64 xor_chain(u64 u,u64 v,int n,u64*out) { return (u^v)+n; }
u64 shift_chain(u64 u,u64 v,int n,u64*out) { return (u<<1)+n; }
u64 call_chain(u64 u,u64 v,int n,u64*out) { return bump(u)+n; }
u64 loaded_chain(u64 u,u64 v,int n,u64*out) { return *out+n; }
u64 named(u64 u,u64 v,int n,u64*out) { u64 t=u+v; return t+n; }
u64 signed_chain(u64 u,u64 v,int n,u64*out) { return ((s64)u+(s64)v)+n; }
u64 cast_chain(u64 u,u64 v,int n,u64*out) { return (u64)((s64)u+(s64)v)+n; }
u64 shared(u64 u,u64 v,int n,u64*out) { u64 t=u+v; *out=t; return t+n; }
u64 promoted_shared(u64 u,u64 v,int n,u64*out) { u64 p=n; *out=p; return (u+v)+p; }
u64 promoted_named(u64 u,u64 v,int n,u64*out) { u64 p=n; return (u+v)+p; }
u64 repeated(u64 u,u64 v,int n,u64*out) { *out=(u64)n; return (u+v)+(u64)n; }
u64 short_chain(u64 u,u64 v,int n,u64*out) { return (u+v)+(short)n; }
u64 char_chain(u64 u,u64 v,int n,u64*out) { return (u+v)+(signed char)n; }
u64 load_chain(u64 u,u64 v,int n,u64*out) { return (u+v)+*(int*)out; }
u64 literal_chain(u64 u,u64 v,int n,u64*out) { return (u+v)+(-1); }
u64 unsigned_chain(u64 u,u64 v,int n,u64*out) { return (u+v)+(unsigned)n; }
u64 wide_chain(u64 u,u64 v,int n,u64*out) { return (u+v)+*out; }
u64 grouped(u64 u,u64 v,int n,u64*out) { return u+((int)v+n); }
u64 right_chain(u64 u,u64 v,int n,u64*out) { return n+(u+(int)v); }
u64 twice(u64 u,u64 v,int n,u64*out) { u=u+n; u=u+n; return u; }
u64 loop(u64 u,u64 v,int n,u64*out) {
    unsigned count=(unsigned)v&7;
    u=u+n;
    while(count) { u=u+n; count=count-1; }
    return u;
}
u64 loop_only(u64 u,u64 v,int n,u64*out) {
    unsigned count=(unsigned)v&7;
    while(count) { u=u+n; count=count-1; }
    return u;
}
u64 call_named(u64 u,u64 v,int n,u64*out) { u64 t=bump(u); return t+n; }
u64 local_twice(u64 u,u64 v,int n,u64*out) { u64 t=u+n; t=t+n; return t; }
u64 loop_local(u64 u,u64 v,int n,u64*out) {
    unsigned count=(unsigned)v&7; u64 t=u+n;
    while(count) { t=t+n; count=count-1; }
    return t;
}
u64 loop_distinct(u64 u,u64 v,int n,u64*out) {
    unsigned count=(unsigned)v&7; u=u+n;
    while(count) { u=u+*(int*)out; count=count-1; }
    return u;
}
u64 loop_after(u64 u,u64 v,int n,u64*out) {
    unsigned count=(unsigned)v&7;
    while(count) { u=u+n; count=count-1; }
    return u+n;
}
u64 and_chain(u64 u,u64 v,int n,u64*out) { return (u&v)+n; }
u64 or_chain(u64 u,u64 v,int n,u64*out) { return (u|v)+n; }
u64 divide_chain(u64 u,u64 v,int n,u64*out) { return (u/v)+n; }
extern int word_bump(int);
u64 word_sum_chain(u64 u,u64 v,int n,u64*out) { return (u+v)+(n+1); }
u64 word_mul_chain(u64 u,u64 v,int n,u64*out) { return (u+v)+(n*2); }
u64 word_call_chain(u64 u,u64 v,int n,u64*out) { return (u+v)+word_bump(n); }
u64 indexed_load_chain(u64 u,u64 v,int n,u64*out) { return (u+v)+((int*)out)[(unsigned)n&3]; }
u64 indexed_base_chain(u64 u,u64 v,int n,u64*out) { return (u+out[(unsigned)v&3])+n; }
u64 offset_load_chain(u64 u,u64 v,int n,u64*out) { return (u+v)+((int*)out)[2]; }
u64 shared_mixed(u64 u,u64 v,int n,u64*out) { u64 p=n; *out=(u+v)+p; return u-p; }
u64 repeated_mixed(u64 u,u64 v,int n,u64*out) { *out=(u+v)+n; return u-n; }
u64 commuted_word_mul(u64 u,u64 v,int n,u64*out) { return (n*2)+(u+v); }
u64 commuted_indexed_load(u64 u,u64 v,int n,u64*out) { return ((int*)out)[(unsigned)n&3]+(u+v); }
u64 commuted_word_call(u64 u,u64 v,int n,u64*out) { return word_bump(n)+(u+v); }
u64 named_word_mul(u64 u,u64 v,int n,u64*out) { int t=n*2; return (u+v)+t; }
