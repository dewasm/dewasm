# requires: rt/trap
def check_range(self, offset, count):
    if offset + count > len(self._slots):
        Rt.trap("out of bounds table access")
