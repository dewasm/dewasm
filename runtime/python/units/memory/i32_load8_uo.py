def i32_load8_uo(self, a, off):
    a += off
    self.check(a, 1)
    return self.data[a]
