// wasm `select`: both operands are pre-evaluated (no short-circuit), so a plain value pick suffices.
func rtSelect[T any](c uint32, a, b T) T {
    if c != 0 {
        return a
    }
    return b
}
