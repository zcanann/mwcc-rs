struct Entry { unsigned char header[8]; char name[32]; };
int compare32(struct Entry *entry, const char *input) {
 char *cursor; char a; char b; int remaining;
 cursor=entry->name; remaining=32;
 while(0<=--remaining) {
  if((a=*cursor++)!=(b=*input++)) return 0;
  else if(b==0) return 1;
 }
 if(*input==0) return 1;
 return 0;
}
int compare7(const char *left, const char *input) {
 char *cursor; char a; char b; int remaining;
 cursor=(char*)left; remaining=7;
 while(0<=--remaining) {
  if((a=*cursor++)!=(b=*input++)) return 0;
  else if(b==0) return 1;
 }
 if(*input==0) return 1;
 return 0;
}
int compare0(const char *left, const char *input) {
 char *cursor; char a; char b; int remaining;
 cursor=(char*)left; remaining=0;
 while(0<=--remaining) {
  if((a=*cursor++)!=(b=*input++)) return 0;
  else if(b==0) return 1;
 }
 if(*input==0) return 1;
 return 0;
}
int compare_dynamic(const char *left, const char *input, int limit) {
 char *cursor; char a; char b; int remaining;
 cursor=(char*)left; remaining=limit;
 while(0<=--remaining) {
  if((a=*cursor++)!=(b=*input++)) return 0;
  else if(b==0) return 1;
 }
 if(*input==0) return 1;
 return 0;
}
int compare_unsigned(const unsigned char *left, const unsigned char *input) {
 unsigned char *cursor; unsigned char a; unsigned char b; int remaining;
 cursor=(unsigned char*)left; remaining=32;
 while(0<=--remaining) {
  if((a=*cursor++)!=(b=*input++)) return 0;
  else if(b==0) return 1;
 }
 if(*input==0) return 1;
 return 0;
}
int guard_count(const unsigned char *input, int limit) {
 int count; count=0;
 while((count=count+1)<limit) { if(*input++==0) return count; }
 if(count==limit) return 7;
 return 9;
}
int guard_byte(const unsigned char *input) {
 unsigned char byte; int count; count=0;
 while((byte=*input++)!=0) { count+=byte; }
 if(count>255) return -1;
 return count;
}
int guard_cursor(const char *input, int limit) {
 const char *cursor; int count; cursor=input; count=0;
 while(count<limit) { if(*cursor++==0) return count; count++; }
 if(*cursor==0) return count;
 return -1;
}
