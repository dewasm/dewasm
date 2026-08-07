// requires: rt/trap
// Linear memory: a byte slice plus a page cap. Effective addresses are
// computed in uint64 by callers (guest addr + offset can exceed 2^32), so
// every method takes a uint64 address and the bounds check is exact.
type Memory struct {
    data     []byte
    maxPages uint32
}

func newMemory(minPages, maxPages uint32) *Memory {
    return &Memory{data: make([]byte, uint64(minPages)*65536), maxPages: maxPages}
}

// The explicit check is load-bearing, not redundant with Go's own slice
// bounds check: it raises a *categorized* wasm trap before the access, whereas
// an out-of-range slice would panic with an uncategorized runtime.Error the
// spec harness's check_trap rejects. So it cannot be dropped in favor
// of letting the slice access fault.
func (m *Memory) check(addr, length uint64) {
    if addr+length > uint64(len(m.data)) {
        Rt.trap("out of bounds memory access")
    }
}
