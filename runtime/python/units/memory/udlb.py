def udlb(self, a):
    a &= 0xFFFFFFFF
    self.check(a, 1)
    return self.data[a]
