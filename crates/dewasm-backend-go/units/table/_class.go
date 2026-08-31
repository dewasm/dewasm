// One table slot: a funcref (type key + the Go func value) or nil for a null slot. call_indirect compares type keys, so a table shared across modules stays consistent.
// `body` is set only for a tail-calling function, whose split body `table/tail_ref` hands to the trampoline so a chain through the table stays flat.
type funcref struct {
    ty   string
    fn   any
    body any
}

type Table struct {
    slots []*funcref
}

func newTable(size uint32) *Table {
    return &Table{slots: make([]*funcref, size)}
}

func (t *Table) size() uint32 {
    return uint32(len(t.slots))
}
