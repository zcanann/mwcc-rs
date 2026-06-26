// Two stores to distinct integer SDA globals where one value is a single-op computation
// and the other a constant (`gi = a+1; gj = 5;`). The constant is a low-latency `li`, so
// the overlap fill schedules it like any other value — but it is materialized LAST, once
// the computation has freed its operand register, which the constant then reuses:
//
//     gi=a+1; gj=5  ->  addi r3,r3,1 ; li r0,5  ; stw r3,gi ; stw r0,gj
//     gi=5; gj=a+1  ->  addi r0,r3,1 ; li r3,5  ; stw r3,gi ; stw r0,gj   (5 reuses a's r3)
//     gi=a*b; gj=9  ->  mullw r3,..  ; li r0,9  ; stw r0,gj ; stw r3,gi   (high-lat: 9 first)
//
// The evaluation order is by weight (high-latency op > single-cycle op > constant), so the
// computation always precedes the constant; the store order is still by latency (a high-
// latency result is stored last). Two constants stay with the constant fill (it dedups a
// repeated value to one `li`); a register leaf goes through try_mixed_store_fill.
int gi, gj;
void low_then_const(int a)        { gi = a + 1; gj = 5; }
void const_then_low(int a)        { gi = 5; gj = a + 1; }   // the constant reuses a's register
void high_then_const(int a, int b){ gi = a * b; gj = 9; }   // high-latency: const stored first
void const_then_high(int a, int b){ gi = 9; gj = a * b; }
void shift_form(int a)            { gi = 100; gj = a << 2; }
