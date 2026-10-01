# requires: rt/link_error
# A present-but-wrong-kind import is a link error, distinct from a missing one.
# An example is a global where a function was declared.
# A missing one still falls through to the caller's WASI/ENOSYS/raise fallback via `||`.
# Function values are plain `Method`/`Proc` objects.
# The runtime's own Global/Table/Memory/Tag wrappers self-report via `wasm_kind`.
def check_import_kind(value, kind, mod, name)
  return value if value.nil?
  ok =
    if kind == :func
      value.is_a?(Method) || value.is_a?(Proc)
    else
      value.respond_to?(:wasm_kind) && value.wasm_kind == kind
    end
  return value if ok
  raise(LinkError, "incompatible import type for #{mod}.#{name}")
end
