# requires: rt/trap
def call(self, i, type_key, *args):
    if i >= len(self._slots):
        Rt.trap("undefined element")
    slot = self._slots[i]
    if slot is None:
        Rt.trap("uninitialized element")
    if slot[0] != type_key:
        Rt.trap("indirect call type mismatch")
    return slot[1](*args)
