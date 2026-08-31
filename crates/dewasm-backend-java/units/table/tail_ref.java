// requires: rt/trap
// `call`'s checks, raised at the same point in execution order, but returning the slot's tail entry for the trampoline instead of the plain one.
// A slot's optional `body` is the split body of a tail-calling function; a slot without one completes in a single frame anyway.
Rt.Fn tailRef(int index, String ty) {
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
    return f.body != null ? f.body : f.fn;
}
