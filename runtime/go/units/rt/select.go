// wasm `select`: both operands are pre-evaluated (no short-circuit), so a
// plain value pick suffices. Generic over the value type since select works
// on any numeric type (ADR-29). Called as `rtSelect(cond, a, b)`.
func rtSelect[T any](c uint32, a, b T) T {
    if c != 0 {
        return a
    }
    return b
}
