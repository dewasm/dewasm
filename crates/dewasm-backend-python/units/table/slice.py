def slice(self, offset, length):
    return self._slots[offset:offset + length]
