def iwla(self, a, b):
    a = (a + b) & 0xFFFFFFFF
    self.check(a, 4)
    return int.from_bytes(self.data[a:a + 4], "little")
