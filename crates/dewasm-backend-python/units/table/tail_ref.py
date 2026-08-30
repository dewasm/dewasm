# requires: rt/trap
# `call`'s checks, raised at the same point in execution order, but returning the slot's callable for the trampoline instead of invoking it.
# A slot's optional third element is the body method of a tail-calling function; a slot without one completes in a single frame anyway.
def tail_ref(self, i, type_key):
    if i >= len(self._slots):
        Rt.trap("undefined element")
    slot = self._slots[i]
    if slot is None:
        Rt.trap("uninitialized element")
    if slot[0] != type_key:
        Rt.trap("indirect call type mismatch")
    return slot[2] if len(slot) > 2 else slot[1]
