func (t *Table) slice(offset, length uint32) []*funcref {
    return t.slots[offset : offset+length]
}
