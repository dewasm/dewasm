@staticmethod
def s32(x):
    return x - 0x100000000 if x >= 0x80000000 else x
