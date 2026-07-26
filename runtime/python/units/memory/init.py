# requires: rt/trap
# Also used to initialize active data segments at instantiation time.
def init(self, dst, data, src, length):
    if src + length > len(data):
        Rt.trap("out of bounds memory access")
    self.check(dst, length)
    if length == 0:
        return
    self.data[dst:dst + length] = data[src:src + length]
