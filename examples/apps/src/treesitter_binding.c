/*
 * treesitter_binding.c — our own committed C source (ADR-9 permits committing
 * first-party source; only third-party *artifacts* stay out of the tree).
 *
 * A reactor library that parses a source string with the tree-sitter runtime
 * and the pre-generated tree-sitter-json grammar, then returns the parse
 * tree's S-expression. Built into cache/treesitter.wasm by
 * examples/apps/fetch.sh from the pinned tree-sitter + tree-sitter-json
 * releases, with the same zig reactor flags as the other C-API apps (ADR-22).
 */
#include <stdint.h>
#include <stdlib.h>

#include <tree_sitter/api.h>

/* Provided by tree-sitter-json's pre-generated src/parser.c. */
const TSLanguage *tree_sitter_json(void);

/*
 * Parse `len` bytes of `src` as JSON and return the root node's S-expression
 * as a freshly malloc'd, NUL-terminated C string in guest memory (via
 * ts_node_string, which malloc()s). The caller reads the string and frees it
 * with free(). Returns 0 only if the parser could not be created.
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
