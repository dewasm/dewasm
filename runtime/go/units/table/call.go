// requires: rt/trap
// Bounds/null/type-check the slot and return its Go func value; the caller
// type-asserts it to the exact signature of the call site.
func (t *Table) call(i uint32, typeKey string) any {
    if i >= uint32(len(t.slots)) {
        Rt.trap("undefined element")
    }
    slot := t.slots[i]
    if slot == nil {
        Rt.trap("uninitialized element")
    }
    if slot.ty != typeKey {
        Rt.trap("indirect call type mismatch")
    }
    return slot.fn
}
