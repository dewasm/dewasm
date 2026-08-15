def i32_load8_uo(self, a, off):
    a = (a & 0xFFFFFFFF) + off
    self.check(a, 1)
    return self.data[a]
