// requires: rt/trap
// Linear memory: a byte slice, with its base pointer and length mirrored into fields.
// Effective addresses are computed in uint64 by callers (guest addr + offset can exceed 2^32), so every method takes a uint64 address and the bounds check is exact.
//
// The load and store units are shaped for Go's inliner: a function over 5000 AST nodes, which every large wasm function becomes, only inlines callees costing at most 20 (`inlineBigFunctionMaxCost`), and a call per memory access is otherwise the whole cost of such a function.
// Each unit therefore does its own check against `n`, reads or writes through `base` with one pointer dereference, and raises the trap as the prebuilt `oobTrap` (a call that builds one costs budget and allocates on a path that never returns).
// A unit that calls another unit does not fit, the `encoding/binary` byte-order helpers included (inside a large function such a nested call is a second inline decision under the same budget and stays a call), so the eight units are the raw widths in host byte order, and the emitter spells the sign extensions and float reinterpretations as casts at the site.
// The units lint asserts the budget.
type Memory struct {
    data     []byte
    base     unsafe.Pointer
    n        uint64
    maxPages uint32
}

var oobTrap = &rtTrap{"out of bounds memory access"}

// The dereference reads linear memory in host byte order, so a big-endian target must fail to compile rather than compute wrong values.
// `runtime.GOARCH` is a constant, and on exactly those targets the second key is also `true`, which Go rejects as a duplicate key in the map literal.
var _ = map[bool]struct{}{true: {}, runtime.GOARCH == "mips" || runtime.GOARCH == "mips64" || runtime.GOARCH == "ppc64" || runtime.GOARCH == "s390x": {}}

func newMemory(minPages, maxPages uint32) *Memory {
    m := &Memory{maxPages: maxPages}
    m.set(make([]byte, uint64(minPages)*65536))
    return m
}

// Replace the backing slice, keeping the mirrored fields in sync.
func (m *Memory) set(data []byte) {
    m.data = data
    m.base = unsafe.Pointer(unsafe.SliceData(data))
    m.n = uint64(len(data))
}

// The explicit check is load-bearing, not redundant with Go's own slice bounds check: it raises a *categorized* wasm trap before the access, whereas an out-of-range slice would panic with an uncategorized runtime.Error the spec harness's check_trap rejects.
// So it cannot be dropped in favor of letting the slice access fault.
func (m *Memory) check(addr, length uint64) {
    if addr+length > m.n {
        panic(oobTrap)
    }
}
