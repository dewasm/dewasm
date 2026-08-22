# Resolve one imported function from the embedder's imports table.
# The value under a module name is either a Hash (name -> callable) or a provider object responding to import(name); providers may also define attach(instance), which generated code calls once the instance is fully constructed.
def resolve_import(imports, mod, name)
  source = imports[mod]
  return nil if source.nil?
  source.respond_to?(:import) ? source.import(name) : source[name]
end
