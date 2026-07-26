// requires: rt/trap, table/check_range
// `elem` is a list of table slot values, built at instantiation/table.init.
func (t *Table) init(dst uint32, elem []*funcref, src, length uint32) {
    if uint64(src)+uint64(length) > uint64(len(elem)) {
        Rt.trap("out of bounds table access")
    }
    t.check_range(dst, length)
    if length == 0 {
        return
    }
    copy(t.slots[dst:dst+length], elem[src:src+length])
}
