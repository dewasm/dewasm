def i32_load8_u(self, a):
    self.check(a, 1)
    return self.data[a]
