// requires: rt/trap
// call_indirect dispatch: bounds-check, null-check, structural-type-check, and
// return the Fn to invoke. The type string is a structural key so a table
// shared across modules stays consistent (ADR-30).
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
