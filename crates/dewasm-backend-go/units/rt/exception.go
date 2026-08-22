// requires: rt/tag
// A wasm exception in flight, raised with panic; the object itself is the exnref value.
// Deliberately unrelated to rtTrap and rtExit: a try_table's handler matches this type alone, so traps and the exit path structurally cannot be caught by catch_all.
type rtException struct {
    tag    *rtTag
    values []any
}

func (e *rtException) Error() string { return "wasm exception" }
