def i32_load8_u(self, a):
    a &= 0xFFFFFFFF
    self.check(a, 1)
    return self.data[a]
