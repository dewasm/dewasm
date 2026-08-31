// requires: rt/trap
// `call`'s checks, raised at the same point in execution order, but resolving a tail call rather than an ordinary one.
// A tail entry is bound to the instance that built it and reads *that* instance's parked slots, so it is only returned to its owner; every other caller gets the ordinary func value and completes the call itself, which costs one frame per instance switch.
func (t *Table) tailRef(i uint32, typeKey string, owner any) (any, any) {
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
    if slot.body != nil && slot.owner == owner {
        return slot.body, nil
    }
    return nil, slot.fn
}
