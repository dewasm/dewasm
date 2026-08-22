@staticmethod
def i64_clz(x):
    return 64 if x == 0 else 64 - x.bit_length()
