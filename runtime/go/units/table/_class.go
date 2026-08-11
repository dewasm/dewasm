// One table slot: a funcref (type key + the Go func value) or nil for a null slot. call_indirect compares type keys, so a table shared across modules stays consistent.
type funcref struct {
    ty string
    fn any
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
