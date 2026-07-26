func (m *Memory) f64_store(a uint64, v float64) {
    m.check(a, 8)
    binary.LittleEndian.PutUint64(m.data[a:a+8], math.Float64bits(v))
}
