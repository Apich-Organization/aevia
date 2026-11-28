// External scanner for nested block comments, string text and tight unit operators.
#include "tree_sitter/parser.h"

enum TokenType { BLOCK_COMMENT, STRING_CONTENT, MULTILINE_CONTENT, TIGHT_UNIT_OPERATOR };

void *tree_sitter_aevia_external_scanner_create(void) { return NULL; }
void tree_sitter_aevia_external_scanner_destroy(void *p) { (void)p; }
unsigned tree_sitter_aevia_external_scanner_serialize(void *p, char *b) { (void)p; (void)b; return 0; }
void tree_sitter_aevia_external_scanner_deserialize(void *p, const char *b, unsigned n) { (void)p; (void)b; (void)n; }

static bool scan_string_content(TSLexer *lexer, bool multiline) {
  bool consumed = false;
  lexer->mark_end(lexer);
  while (!lexer->eof(lexer)) {
    int32_t c = lexer->lookahead;
    if (c == '\\' || c == '{' || (!multiline && (c == '"' || c == '\r' || c == '\n')))
      break;
    lexer->advance(lexer, false);
    if (c == '"' && lexer->lookahead == '"') {
      lexer->advance(lexer, false);
      if (lexer->lookahead == '"') break; // leave all three quotes for the delimiter
    }
    lexer->mark_end(lexer);
    consumed = true;
  }
  lexer->result_symbol = multiline ? MULTILINE_CONTENT : STRING_CONTENT;
  return consumed;
}

static bool scan_block_comment(TSLexer *lexer) {
  while (lexer->lookahead == ' ' || lexer->lookahead == '\t')
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

static bool is_unit_operator(int32_t c) {
  return c == '*' || c == '/' || c == 0xb7 || c == 0xd7 || c == 0xf7; // · × ÷
}

// Called at the first byte after a factor, before any extras are skipped.
// The right operand must also start immediately. Unicode names are checked by
// the grammar, with the Unicode unary operator spellings excluded here.
static bool scan_tight_unit_operator(TSLexer *lexer) {
  lexer->advance(lexer, false);
  int32_t c = lexer->lookahead;
  if (!((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || c == '_' ||
        c == '%' || (c >= 0x80 && c != 0x2212 && c != 0x221a))) // − √
    return false;

  lexer->result_symbol = TIGHT_UNIT_OPERATOR;
  return true;
}

bool tree_sitter_aevia_external_scanner_scan(void *p, TSLexer *lexer, const bool *valid) {
  (void)p;
  // In error recovery Tree-sitter enables all external symbols. Neither
  // string body can start there without an opening delimiter.
  if (valid[STRING_CONTENT] && valid[MULTILINE_CONTENT]) return false;
  if (valid[STRING_CONTENT] || valid[MULTILINE_CONTENT])
    return scan_string_content(lexer, valid[MULTILINE_CONTENT]);
  if (valid[TIGHT_UNIT_OPERATOR] && is_unit_operator(lexer->lookahead))
    return scan_tight_unit_operator(lexer);
  return valid[BLOCK_COMMENT] && scan_block_comment(lexer);
}
