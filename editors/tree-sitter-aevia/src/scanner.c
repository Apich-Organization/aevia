// External scanner: nestable block comments `#- ... -#` (spec 1.3).
#include "tree_sitter/parser.h"

enum TokenType { BLOCK_COMMENT };

void *tree_sitter_aevia_external_scanner_create(void) { return NULL; }
void tree_sitter_aevia_external_scanner_destroy(void *p) { (void)p; }
unsigned tree_sitter_aevia_external_scanner_serialize(void *p, char *b) { (void)p; (void)b; return 0; }
void tree_sitter_aevia_external_scanner_deserialize(void *p, const char *b, unsigned n) { (void)p; (void)b; (void)n; }

bool tree_sitter_aevia_external_scanner_scan(void *p, TSLexer *lexer, const bool *valid) {
  (void)p;
  if (!valid[BLOCK_COMMENT]) return false;
  while (lexer->lookahead == ' ' || lexer->lookahead == '\t' || lexer->lookahead == '\n' || lexer->lookahead == '\r')
    lexer->advance(lexer, true);
  if (lexer->lookahead != '#') return false;
  lexer->advance(lexer, false);
  if (lexer->lookahead != '-') return false;
  lexer->advance(lexer, false);
  unsigned depth = 1;
  while (depth > 0) {
    if (lexer->eof(lexer)) { lexer->result_symbol = BLOCK_COMMENT; return true; } // unclosed: the LSP reports E0104
    int32_t c = lexer->lookahead;
    lexer->advance(lexer, false);
    if (c == '#' && lexer->lookahead == '-') { lexer->advance(lexer, false); depth++; }
    else if (c == '-' && lexer->lookahead == '#') { lexer->advance(lexer, false); depth--; }
  }
  lexer->result_symbol = BLOCK_COMMENT;
  return true;
}
