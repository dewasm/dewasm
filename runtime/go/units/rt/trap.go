// A wasm trap: a runtime fault (out-of-bounds, integer overflow, ...) raised
// with panic and recovered at the public boundary, mirroring Ruby/Python's
// Rt.Trap.
type rtTrap struct{ msg string }

func (e *rtTrap) Error() string { return e.msg }

func (rt) trap(msg string) {
    panic(&rtTrap{msg})
}
