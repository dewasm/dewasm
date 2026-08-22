// Identity object for a wasm exception tag: catch clauses compare tags with `==`, so an imported tag matches its origin by sharing the object, never by structure.
static final class Tag {
}
