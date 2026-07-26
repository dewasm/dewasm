// wasm comparisons yield an i32 0/1; Go comparisons yield bool. This is the
// bridge, used at every relational operator (ADR-29).
func (rt) b2i(c bool) uint32 {
    if c {
        return 1
    }
    return 0
}
