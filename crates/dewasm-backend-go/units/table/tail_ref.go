// requires: rt/trap
// `call`'s checks, raised at the same point in execution order, but returning the slot's func value for the trampoline instead of handing back the plain entry.
// A slot's optional `body` is the split body of a tail-calling function; a slot without one completes in a single frame anyway, and the caller type-asserts whichever it got.
func (t *Table) tailRef(i uint32, typeKey string) any {
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
    if slot.body != nil {
        return slot.body
    }
    return slot.fn
}
