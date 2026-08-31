// requires: rt/trap
// `call`'s checks, raised at the same point in execution order, but handing back the whole slot so the caller can see whether it owns the tail entry.
Rt.Funcref tailSlot(int index, String ty) {
    if (index < 0 || index >= slots.length) {
        Rt.trap("undefined element");
    }
    Rt.Funcref f = slots[index];
    if (f == null) {
        Rt.trap("uninitialized element");
    }
    if (!f.ty.equals(ty)) {
        Rt.trap("indirect call type mismatch");
    }
    return f;
}
