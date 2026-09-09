typedef unsigned long long u64;
u64 nested_twice(u64 u,u64 v,int n,u64*out) { return u+n+n; }
u64 assign_twice(u64 u,u64 v,int n,u64*out) { u=u+n; u=u+n; return u; }
u64 assign_thrice(u64 u,u64 v,int n,u64*out) { u=u+n; u=u+n; u=u+n; return u; }
u64 local_twice(u64 u,u64 v,int n,u64*out) { u64 t=u+n; t=t+n; return t; }
u64 named_promotion_twice(u64 u,u64 v,int n,u64*out) { u64 p=n; u=u+p; u=u+p; return u; }
u64 store_add_return_add(u64 u,u64 v,int n,u64*out) { *out=u+n; return v+n; }
u64 store_chain_return_add(u64 u,u64 v,int n,u64*out) { *out=(u+v)+n; return u+n; }
u64 store_add_return_chain(u64 u,u64 v,int n,u64*out) { *out=u+n; return (u+v)+n; }
u64 store_add_return_sub(u64 u,u64 v,int n,u64*out) { *out=u+n; return v-n; }
u64 store_sub_return_add(u64 u,u64 v,int n,u64*out) { *out=u-n; return v+n; }
u64 store_chain_return_sub(u64 u,u64 v,int n,u64*out) { *out=(u+v)+n; return u-n; }
u64 store_sub_return_chain(u64 u,u64 v,int n,u64*out) { *out=u-n; return (u+v)+n; }
u64 store_chain_return_word(u64 u,u64 v,int n,u64*out) { *out=(u+v)+n; return (u64)n; }
u64 store_word_return_chain(u64 u,u64 v,int n,u64*out) { *out=(u64)n; return (u+v)+n; }
u64 add_then_store_word(u64 u,u64 v,int n,u64*out) { u=u+n; *out=(u64)n; return u; }
u64 chain_then_store_word(u64 u,u64 v,int n,u64*out) { u=(u+v)+n; *out=(u64)n; return u; }
u64 store_low_return_chain(u64 u,u64 v,int n,u64*out) { *out=(unsigned)n; return (u+v)+n; }
u64 named_mixed(u64 u,u64 v,int n,u64*out) { u64 p=n; *out=(u+v)+p; return u-p; }
u64 named_chain_add(u64 u,u64 v,int n,u64*out) { u64 p=n; *out=(u+v)+p; return u+p; }
u64 local_copy_twice(u64 u,u64 v,int n,u64*out) { u64 t=u+n; u64 s=t; return s+n; }
u64 add_zero_twice(u64 u,u64 v,int n,u64*out) { u=u+n; return u+(n+0); }
u64 cast_twice(u64 u,u64 v,int n,u64*out) { u=u+n; return u+(int)n; }
u64 add_after_sub(u64 u,u64 v,int n,u64*out) { u=u-n; return u+n; }
u64 sub_after_add(u64 u,u64 v,int n,u64*out) { u=u+n; return u-n; }
u64 sub_twice(u64 u,u64 v,int n,u64*out) { u=u-n; return u-n; }
u64 chain_after_add(u64 u,u64 v,int n,u64*out) { u=u+n; return (u+v)+n; }
u64 add_after_chain(u64 u,u64 v,int n,u64*out) { u=(u+v)+n; return u+n; }
u64 loop(u64 u,u64 v,int n,u64*out) { unsigned count=(unsigned)v&7; u=u+n; while(count){u=u+n; count=count-1;} return u; }
u64 loop_only(u64 u,u64 v,int n,u64*out) { unsigned count=(unsigned)v&7; while(count){u=u+n; count=count-1;} return u; }
u64 loop_after(u64 u,u64 v,int n,u64*out) { unsigned count=(unsigned)v&7; while(count){u=u+n; count=count-1;} return u+n; }
u64 load_twice(u64 u,u64 v,int n,u64*out) { u=u+*(int*)out; return u+*(int*)out; }
u64 load_loop(u64 u,u64 v,int n,u64*out) { unsigned count=(unsigned)v&7; u=u+*(int*)out; while(count){u=u+*(int*)out; count=count-1;} return u; }
u64 store_between_loads(u64 u,u64 v,int n,u64*out) { u=u+*(int*)out; *(int*)out=n; return u+*(int*)out; }
u64 branch_reuse(u64 u,u64 v,int n,u64*out) { u=u+n; if(v) u=u+n; return u; }
u64 branch_only(u64 u,u64 v,int n,u64*out) { if(v) return u+n; return u+n; }
u64 loop_named_promotion(u64 u,u64 v,int n,u64*out) { u64 p=n; unsigned count=(unsigned)v&7; u=u+p; while(count){u=u+p; count=count-1;} return u; }
u64 changed_word_loop(u64 u,u64 v,int n,u64*out) { unsigned count=(unsigned)v&7; u=u+n; while(count){n=n+1;u=u+n;count=count-1;} return u; }
u64 moving_load_loop(u64 u,u64 v,int n,u64*out) { unsigned count=(unsigned)v&7; u=u+*(int*)out; while(count){out=(u64*)((int*)out+1);u=u+*(int*)out;count=count-1;} return u; }
u64 guarded_first(u64 u,u64 v,int n,u64*out) { if(v) u=u+n; return u+n; }
u64 merged_first(u64 u,u64 v,int n,u64*out) { if(v) u=u+v; else u=u-v; u=u+n; return u+n; }
u64 volatile_param_twice(u64 u,u64 v,int n,volatile int*out) { u=u+*out; return u+*out; }
u64 volatile_cast_twice(u64 u,u64 v,int n,u64*out) { u=u+*(volatile int*)out; return u+*(volatile int*)out; }
u64 volatile_loop(u64 u,u64 v,int n,volatile int*out) { unsigned count=(unsigned)v&7; u=u+*out; while(count){u=u+*out;count=count-1;} return u; }
