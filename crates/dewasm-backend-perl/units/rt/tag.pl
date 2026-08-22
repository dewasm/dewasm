# An identity object for a wasm exception tag: try_table catch clauses compare tags by reference identity (perl's `==` on a blessed hashref compares addresses), never by structure.
# Two `(tag)` definitions must stay distinct even when they share a type, while one tag imported twice must match itself; sharing the object through the provider protocol (`tag_export`, `wasm_import`, `check_import_kind` via `wasm_kind`) is the entire cross-instance story.
sub tag {
    return bless({ wasm_kind => 'tag' }, 'Rt::Tag');
}
