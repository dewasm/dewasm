# Resolve one import from the embedder's imports hash reference.
# The value under a module name is a plain hash reference (name -> value) or a provider object.
# A provider object responds to `wasm_import(name)`.
sub resolve_import {
    my ($imports, $mod, $name) = @_;
    my $source = $imports->{$mod};
    return undef unless defined $source;
    return $source->{$name} if ref($source) eq 'HASH';
    return $source->wasm_import($name);
}
