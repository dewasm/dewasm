// requires: rt/trap
// The type string is a structural key, not a module-local type index, so a table
// shared across modules stays consistent.
Rt.Fn call(int index, String ty) {
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
    return f.fn;
}
