# requires: memory/size
def grow(self, delta):
    old = self.size()
    if old + delta > self.max_pages:
        return Rt.M32
    self.data.extend(b"\x00" * (delta * self.PAGE_SIZE))
    return old
