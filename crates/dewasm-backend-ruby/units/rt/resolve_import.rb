# Resolve one imported function from the embedder's imports table.
# The value under a module name is either a Hash (name -> callable) or a provider object.
# A provider object responds to import(name).
# Providers may also define attach(instance).
# Generated code calls it once the instance is constructed.
def resolve_import(imports, mod, name)
  source = imports[mod]
  return nil if source.nil?
  source.respond_to?(:import) ? source.import(name) : source[name]
end
