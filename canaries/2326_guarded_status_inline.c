struct Item { unsigned bits; const struct Item *identity; };
extern unsigned status_mask;
extern struct Item empty_item;
extern int sample(const struct Item *);
extern int check(const struct Item *, int);
int classify(const struct Item *p) {
 const struct Item *identity=p->identity;
 int result;
 unsigned bits;
 result=sample(p);
 if(result<0) {
  bits=p->bits & status_mask;
  if((bits&1) && check(p,1)) return 0;
  if((bits&2) && check(identity,2)) return 0;
 }
 return result;
}
int direct(const struct Item *p) {
 int result=classify(p);
 if(result==-10 && (p->bits&4)) return 0;
 return result;
}
int assigned(const struct Item *p) {
 int result;
 result=classify(p);
 if(result==-10 && (p->bits&4)) return 0;
 return result;
}
int continuation(const struct Item *p) {
 int before=sample(p);
 int result=classify(p);
 int after=check(p,3);
 return before+result+after;
}
int shared_address(const struct Item *p, const struct Item *q) {
 const struct Item *identity=p->identity;
 if(q->bits==255) return -4;
 if(identity==&empty_item || (check(q,1)==0 && check(identity,2)==0)) return 0;
 return -10;
}
