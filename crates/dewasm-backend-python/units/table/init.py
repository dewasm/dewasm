# requires: rt/trap, table/check_range
# `elem` is a list of table slot values ([type_key, func] pairs, or None for a ref.null item), built once at instantiation/table.init time.
def init(self, dst, elem, src, length):
    if src + length > len(elem):
        Rt.trap("out of bounds table access")
    self.check_range(dst, length)
    if length == 0:
        return
    self._slots[dst:dst + length] = elem[src:src + length]
