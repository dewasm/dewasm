@staticmethod
def s64(x):
    return x - 0x10000000000000000 if x >= 0x8000000000000000 else x
