/* Linkage-first aggregate frames retain the complete incoming parameter table
 * below the first automatic object, even when those parameters are otherwise
 * dead. The aggregate's own alignment is applied after that table. */
typedef struct AggregateEntryImage {
    double words[2];
} AggregateEntryImage;

extern void consume_aggregate_entry(AggregateEntryImage* image);

void aggregate_entry_parameter_table(void* unused)
{
    AggregateEntryImage image;
    consume_aggregate_entry(&image);
}
