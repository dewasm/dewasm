# Identity object for a wasm exception tag: catch clauses compare tags with `equal?`, so an imported tag matches its origin by sharing the object, never by structure.
class Tag
  def wasm_kind = :tag
end
