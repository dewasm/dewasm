# A wasm exception in flight; the object itself is the `exnref` value.
# Deliberately unrelated to Trap, Exit, and LinkError.
# The classification step of `try_table` tests for this class alone.
# So traps and the exit path structurally cannot be caught by `catch_all`.
sub wasm_exception {
    my ($tag, $values) = @_;
    die bless({ tag => $tag, values => $values }, 'Rt::WasmException');
}
