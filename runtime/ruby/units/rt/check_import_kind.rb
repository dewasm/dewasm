# A present-but-wrong-kind import (e.g. a global where a func was
# declared) is a link error, distinct from a missing one (which still
# falls through to the caller's WASI/ENOSYS/raise fallback via `||`).
# Function values are bare Method/Proc objects; the runtime's own
# Global/Table/Memory wrappers self-report via `wasm_kind`.
def check_import_kind(value, kind, mod, name)
  return value if value.nil?
  ok =
    if kind == :func
      value.is_a?(Method) || value.is_a?(Proc)
    else
      value.respond_to?(:wasm_kind) && value.wasm_kind == kind
    end
  return value if ok
  raise(ArgumentError, "incompatible import type for #{mod}.#{name}")
end
