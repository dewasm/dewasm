# Identity object for a wasm exception tag: catch clauses compare tags with `is`, so an imported tag matches its origin by sharing the object, never by structure.
class Tag:
    wasm_kind = "tag"
