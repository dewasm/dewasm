# requires: rt/trap
PAGE_SIZE = 65536

wasm_kind = "memory"

def __init__(self, min_pages, max_pages):
    self.data = bytearray(min_pages * 65536)
    self.max_pages = max_pages if (max_pages is not None and max_pages < 65536) else 65536

def check(self, addr, length):
    if addr + length > len(self.data):
        Rt.trap("out of bounds memory access")
