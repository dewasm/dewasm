# A wasm exception in flight; the object itself is the exnref value.
# Deliberately unrelated to Trap, Exit, and LinkError: try_table's classification step tests for this class alone, so traps and the exit path structurally cannot be caught by catch_all.
sub wasm_exception {
    my ($tag, $values) = @_;
    die bless({ tag => $tag, values => $values }, 'Rt::WasmException');
}
