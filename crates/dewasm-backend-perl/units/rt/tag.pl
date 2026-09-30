# An identity object for a wasm exception tag.
# `try_table` catch clauses compare tags by reference identity, never by structure.
# Perl's `==` on a blessed hash reference compares addresses.
# Two `(tag)` definitions must stay distinct even when they share a type.
# One tag imported twice must match itself.
# Sharing the object through the provider protocol is the entire cross-instance story.
# The protocol is `tag_export`, `wasm_import`, and `check_import_kind` via `wasm_kind`.
sub tag {
    return bless({ wasm_kind => 'tag' }, 'Rt::Tag');
}
