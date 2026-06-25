// A void function whose body is two or more calls that each pass the same single
// parameter as their sole argument — `f(x){ g(x); h(x); }`. The parameter is live across
// the calls (each call clobbers the argument registers), so mwcc saves it in the
// callee-saved r31 up front; the FIRST call uses the incoming argument register directly
// (no move), and each later call restores it from r31:
//
//     stwu r1,-16(r1); mflr r0; stw r0,20(r1); stw r31,12(r1)
//     mr   r31,r3      ; save the live parameter
//     bl   g           ; g(x) — x already in r3 (incoming), no move
//     mr   r3,r31      ; restore x for the next call
//     bl   h           ; h(x)
//     <epilogue>
//
// This is one of the most common real-code shapes (an object handed to several functions
// in turn) and was the dominant `value live across a call` deferral. Restricted for now
// to a single parameter passed as the sole argument; extra arguments, multiple live
// parameters, a non-void return, or a store in the body still defer to the keystone.
struct Obj;
void Init(struct Obj *);
void Update(struct Obj *);
void Render(struct Obj *);
void log_int(int);
void tick(struct Obj *o)  { Init(o); Update(o); Render(o); }   // mr r31,r3; bl; mr r3,r31; bl; mr r3,r31; bl
void pair(struct Obj *o)  { Init(o); Update(o); }              // mr r31,r3; bl; mr r3,r31; bl
void twice(int x)         { log_int(x); log_int(x); }          // same, an integer parameter

// Multiple parameters passed as the same argument list to each call: saved r31 (last
// parameter) descending, the first call uses the incoming registers, later calls restore.
void at(struct Obj *, int);
void draw(struct Obj *o, int n) { at(o, n); at(o, n); }        // mr r31,r4; mr r30,r3; bl; mr r3,r30; mr r4,r31; bl
