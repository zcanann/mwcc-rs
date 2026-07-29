// A free-list transfer expands the small intrusive-list extraction helper,
// preserves the heap descriptor in r31, then calls the larger insertion helper.
// The heap index is scaled by the descriptor stride before either list update.
struct TransferCell {
    struct TransferCell* previous;
    struct TransferCell* next;
    unsigned size;
};

struct TransferHeap {
    int size;
    struct TransferCell* free;
    struct TransferCell* allocated;
};

struct TransferHeap* transfer_heaps;

static struct TransferCell* transfer_extract(
    struct TransferCell* list,
    struct TransferCell* cell)
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

struct TransferCell* transfer_insert(
    struct TransferCell* list,
    struct TransferCell* cell);

void inlined_list_extract_then_insert(int heap, void* pointer)
{
    struct TransferHeap* descriptor;
    struct TransferCell* cell;

    cell = (void*)((unsigned)pointer - 32);
    descriptor = &transfer_heaps[heap];
    descriptor->allocated = transfer_extract(descriptor->allocated, cell);
    descriptor->free = transfer_insert(descriptor->free, cell);
}
