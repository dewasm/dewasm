// wasm `select`: both operands are pre-evaluated (neither is skipped).
// So a plain value pick is enough.
func rtSelect[T any](c uint32, a, b T) T {
    if c != 0 {
        return a
    }
    return b
}
