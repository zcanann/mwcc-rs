// Removing an intrusive doubly-linked-list cell retains each tested neighbor
// in r5 across the corresponding link repair. This avoids reloading the member
// and leaves the incoming list head in r3 for the fallthrough return.
struct ExtractCell {
    struct ExtractCell* previous;
    struct ExtractCell* next;
};

struct ExtractCell* doubly_linked_list_extract(
    struct ExtractCell* list,
    struct ExtractCell* cell)
{
    if (cell->next) {
        cell->next->previous = cell->previous;
    }
    if (cell->previous == 0) {
        return cell->next;
    }
    cell->previous->next = cell->next;
    return list;
}
