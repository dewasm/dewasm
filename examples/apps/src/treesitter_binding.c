/*
 * treesitter_binding.c: our own committed C source.
 * First-party source is fine to commit; only third-party *artifacts* stay out of the tree.
 *
 * A reactor library that parses a source string with the tree-sitter runtime.
 * It uses the pre-generated `tree-sitter-json` grammar, then returns the S-expression of the tree.
 * `examples/apps/scripts/treesitter.sh` builds this into `cache/treesitter.wasm`.
 * It builds from the `tree-sitter` and `tree-sitter-json` releases at fixed versions.
 * It uses the same `wasi-sdk` reactor flags as the other C-API apps.
 */
#include <stdint.h>
#include <stdlib.h>

#include <tree_sitter/api.h>

/* Provided by the pre-generated `src/parser.c` of `tree-sitter-json`. */
const TSLanguage *tree_sitter_json(void);

/*
 * Parse `len` bytes of `src` as JSON and return the S-expression of the root node.
 * It is a new `malloc`'d, NUL-terminated C string in guest memory.
 * `ts_node_string` makes it, and that function calls `malloc()`.
 * The caller reads the string and frees it with `free()`.
 * Returns 0 only if the parser could not be created.
 */
char *parse_source(const char *src, uint32_t len) {
  TSParser *parser = ts_parser_new();
  if (parser == NULL) {
    return NULL;
  }
  ts_parser_set_language(parser, tree_sitter_json());
  TSTree *tree = ts_parser_parse_string(parser, NULL, src, len);
  TSNode root = ts_tree_root_node(tree);
  char *sexpr = ts_node_string(root);
  ts_tree_delete(tree);
  ts_parser_delete(parser);
  return sexpr;
}
