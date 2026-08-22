# requires: rt/trap, table/check_range, table/slice
def copy(self, dst, other, src, length):
    if src + length > other.size():
        Rt.trap("out of bounds table access")
    self.check_range(dst, length)
    if length == 0:
        return
    self._slots[dst:dst + length] = other.slice(src, length)
