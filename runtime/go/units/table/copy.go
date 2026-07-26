// requires: rt/trap, table/check_range, table/slice
func (t *Table) copy(dst uint32, other *Table, src, length uint32) {
    if uint64(src)+uint64(length) > uint64(other.size()) {
        Rt.trap("out of bounds table access")
    }
    t.check_range(dst, length)
    if length == 0 {
        return
    }
    copy(t.slots[dst:dst+length], other.slice(src, length))
}
