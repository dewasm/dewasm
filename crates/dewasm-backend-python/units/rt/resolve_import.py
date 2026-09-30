# Resolve one import from the embedder's imports `dict`.
# The value under a module name is either a `dict` (name -> callable) or a provider object.
# A provider object responds to `wasm_import(name)`.
# Providers may also define attach(instance).
# Generated code calls it once the instance is constructed.
@staticmethod
def resolve_import(imports, mod, name):
    source = imports.get(mod)
    if source is None:
        return None
    if hasattr(source, "wasm_import"):
        return source.wasm_import(name)
    return source.get(name)
