# Identity object for a wasm exception tag: catch clauses compare tags with `is`.
# So an imported tag matches its origin by sharing the object, never by structure.
class Tag:
    wasm_kind = "tag"
