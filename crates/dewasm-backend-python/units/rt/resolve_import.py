# Resolve one import from the embedder's imports dict.
# The value under a module name is either a dict (name -> callable) or a provider object responding to wasm_import(name); providers may also define attach(instance), which generated code calls once the instance is fully constructed.
@staticmethod
def resolve_import(imports, mod, name):
    source = imports.get(mod)
    if source is None:
        return None
    if hasattr(source, "wasm_import"):
        return source.wasm_import(name)
    return source.get(name)
