func (m *Memory) f64_load(a uint64) float64 {
    m.check(a, 8)
    return math.Float64frombits(binary.LittleEndian.Uint64(m.data[a : a+8]))
}
