# requires: rt/trap
def call(self, i, type_key, *args):
    if i >= len(self._slots):
        Rt.trap("undefined element")
    slot = self._slots[i]
    if slot is None:
        Rt.trap("uninitialized element")
    ty, func = slot
    if ty != type_key:
        Rt.trap("indirect call type mismatch")
    return func(*args)
