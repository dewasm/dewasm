// requires: rt/trap
func (t *Table) check_range(offset, count uint32) {
    if uint64(offset)+uint64(count) > uint64(len(t.slots)) {
        Rt.trap("out of bounds table access")
    }
}
