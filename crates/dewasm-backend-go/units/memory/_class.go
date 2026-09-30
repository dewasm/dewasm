// requires: rt/trap
// Linear memory: a byte slice, with its base pointer and length mirrored into fields.
// Callers compute effective addresses in uint64 (guest address + offset can exceed 2^32).
// So every method takes a uint64 address, and the bounds check is exact.
//
// The load and store units are shaped for Go's inliner.
// Every large wasm function becomes a Go function over 5000 AST nodes.
// Such a function only inlines callees costing at most 20 (`inlineBigFunctionMaxCost`).
// A call per memory access is otherwise the whole cost of such a function.
// Each unit therefore does its own check against `n`.
// It reads or writes through `base` with one pointer dereference.
// It raises the trap as the prebuilt `oobTrap`.
// A call that builds a trap costs budget and allocates on a path that never returns.
// A unit that calls another unit does not fit, the `encoding/binary` byte-order helpers included.
// Inside a large function, such a nested call is a second inline decision under the same budget.
// That call stays a call.
// So the eight units are the raw widths in host byte order.
// The emitter spells the sign extensions and float reinterpretations as casts at the site.
// The units lint asserts the budget.
type Memory struct {
    data     []byte
    base     unsafe.Pointer
    n        uint64
    maxPages uint32
}

var oobTrap = &rtTrap{"out of bounds memory access"}

// The dereference reads linear memory in host byte order.
// So a big-endian target must fail to compile rather than compute wrong values.
// `runtime.GOARCH` is a constant, and on exactly those targets the second key is also `true`.
// Go rejects that as a duplicate key in the map literal.
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

// The explicit check is load-bearing: Go's own slice bounds check does not replace it.
// It raises a wasm trap of a known category before the access.
// An out-of-range slice would instead panic with a `runtime.Error` of no category.
// The specification harness's `check_trap` rejects that error.
// So it cannot be dropped in favor of letting the slice access fault.
func (m *Memory) check(addr, length uint64) {
    if addr+length > m.n {
        panic(oobTrap)
    }
}
