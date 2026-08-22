func (m *Memory) read_string(ptr, length uint64) []byte {
    m.check(ptr, length)
    b := make([]byte, length)
    copy(b, m.data[ptr:ptr+length])
    return b
}
