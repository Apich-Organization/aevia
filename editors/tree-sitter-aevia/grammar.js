/**
 * A lexical tree-sitter grammar for Aevia.
 *
 * Aevia is layout-sensitive (offside rule, spec 1.4). A number
 * may carry a unit suffix within a logical line (spec 1.8.1). This
 * grammar recognises tokens, bracket groups and declaration heads for
 * editors. The language server provides the semantic tokens.
 */
const KEYWORDS = [
  'as', 'class', 'derive', 'dimension', 'do', 'else', 'fn', 'for', 'if',
  'import', 'in', 'instance', 'let', 'match', 'module', 'open', 'private',
  'rec', 'test', 'then', 'type', 'unit', 'where', 'with',
];

const LINE_BREAK = /\r\n|[\r\n]/;

const HEADS = ['fn', 'module', 'import', 'open', 'type', 'class', 'instance', 'dimension', 'unit'];

module.exports = grammar({
  name: 'aevia',

  extras: $ => [/[ \t]+/, $.comment, $.doc_comment, $.block_comment],

  externals: $ => [$.block_comment, $.string_content, $._multiline_content, $._tight_unit_operator],

  word: $ => $.identifier,
  // Inside delimiters, a newline after a number may precede either a
  // unit suffix or the next token. Prefer the suffix if both parses fit.
  conflicts: $ => [[$._atom, $._delimited_quantity], [$._inner, $._delimited_quantity]],

  rules: {
    source_file: $ => repeat(choice($._common_item, $.quantity, $._line_break, $._unmatched_close)),

    _common_item: $ => choice(
      $.module_declaration,
      $.import_declaration,
      $.function_declaration,
      $.type_declaration,
      $.dimension_declaration,
      $.unit_declaration,
      $.attribute,
      $.parenthesized,
      $.bracketed,
      $.braced,
      $._atom,
    ),

    _atom: $ => choice(
      $.string,
      $.raw_string,
      $.number,
      $.boolean,
      $.keyword,
      $.type_identifier,
      $.identifier,
      $.operator,
      $.punctuation,
    ),

    // Keep the anonymous delimiter tokens beneath the named bracket leaves
    // used by highlighting and bracket queries.
    _lparen: _ => '(',
    _rparen: _ => ')',
    _lbracket: _ => '[',
    _rbracket: _ => ']',
    _lbrace: _ => '{',
    _rbrace: _ => '}',

    // Physical newlines separate tokens at layout depth zero. Delimited
    // expressions and string interpolations permit a suffix after a newline.
    _line_break: _ => LINE_BREAK,
    _delimited_item: $ => choice($._common_item, alias($._delimited_quantity, $.quantity), $._line_break),
    parenthesized: $ => seq(alias($._lparen, $.bracket), repeat($._delimited_item), alias($._rparen, $.bracket)),
    bracketed: $ => seq(alias($._lbracket, $.bracket), repeat($._delimited_item), alias($._rbracket, $.bracket)),
    braced: $ => seq(alias($._lbrace, $.bracket), repeat($._delimited_item), alias($._rbrace, $.bracket)),
    _unmatched_close: $ => alias(choice(')', ']', '}'), $.bracket),

    module_declaration: $ => prec.right(seq('module', optional(field('path', $.module_path)))),
    import_declaration: $ => prec.right(seq(choice('import', 'open'), optional(field('path', $.module_path)))),

    module_path: $ => seq($.type_identifier, repeat(seq(token.immediate('.'), $.type_identifier))),

    function_declaration: $ => prec.right(seq('fn', optional(field('name', $.identifier)))),

    type_declaration: $ => prec.right(seq(choice('type', 'class', 'instance'), optional(field('name', $.type_identifier)))),
    dimension_declaration: $ => prec.right(seq('dimension', optional(field('name', $.type_identifier)))),
    unit_declaration: $ => prec.right(seq('unit', optional(field('name', $.unit_name)))),

    attribute: $ => seq('@', $.identifier),

    quantity: $ => prec.right(1, seq($.number, $.unit_expression)),
    _delimited_quantity: $ => prec.dynamic(1, prec.right(seq($.number, repeat($._line_break), $.unit_expression))),
    unit_expression: $ => seq(
      choice($._unit_factor, $._unit_group),
      repeat(seq($._tight_unit_operator, $._unit_factor)),
    ),
    _unit_group: $ => seq(
      alias($._lparen, $.bracket),
      repeat($._line_break),
      $.unit_expression,
      repeat($._line_break),
      alias($._rparen, $.bracket),
      optional($._unit_power),
    ),
    _unit_power: $ => choice(seq(token.immediate('^'), $.unit_exponent), $.superscript),
    _unit_factor: $ => choice(
      seq($.unit_name, optional($._unit_power)),
      '%',
    ),
    unit_name: _ => token(/[\p{L}°µμΩÅ_][\p{L}\p{N}_°µμΩÅ]*(\.[\p{L}\p{N}_]+)*/),
    unit_exponent: _ => token.immediate(/-?[0-9]+|\(-?[0-9]+(\/[0-9]+)?\)/),
    superscript: _ => token.immediate(/[⁰¹²³⁴⁵⁶⁷⁸⁹⁻⁺]+/),

    number: _ => token(choice(
      /0[xX][0-9a-fA-F]+(_[0-9a-fA-F]+)*/,
      /0[oO][0-7]+(_[0-7]+)*/,
      /0[bB][01]+(_[01]+)*/,
      /[0-9]+(_[0-9]+)*(\.[0-9]+(_[0-9]+)*)?([eE][+-]?[0-9]+)?/,
    )),

    string: $ => choice($._ordinary_string, $._multiline_string),
    _ordinary_string: $ => seq(
      '"',
      repeat(choice($.escape_sequence, $.interpolation, $.string_content)),
      choice(token.immediate('"'), $.unterminated_string),
    ),
    _multiline_string: $ => seq(
      '"""',
      repeat(choice($.escape_sequence, $.interpolation, alias($._multiline_content, $.string_content))),
      token.immediate('"""'),
    ),
    // Keep error recovery at the end of an ordinary line. The compiler
    // supplies the missing-quote diagnostic through the language server.
    unterminated_string: _ => token.immediate(LINE_BREAK),
    raw_string: _ => token(seq('r"', /[^"\r\n]*/, '"')),
    escape_sequence: _ => token.immediate(/\\(u\{[0-9a-fA-F]+\}|[0ntr\\"{}])/),

    interpolation: $ => seq(
      field('open', token.immediate('{')),
      repeat1($._inner),
      field('close', '}'),
    ),
    interpolation_group: $ => seq(
      field('open', alias(choice('{', '\\{'), $.bracket)),
      repeat($._inner),
      field('close', alias(choice('}', '\\}'), $.bracket)),
    ),
    _protected_string: $ => seq(
      '\\"',
      repeat(choice(alias($._protected_escape, $.escape_sequence), $.interpolation, $.string_content)),
      choice(token.immediate('\\"'), $.unterminated_string),
    ),
    _protected_escape: _ => token.immediate(/\\(u\{[0-9a-fA-F]+\}|[0ntr\\{}])/),
    _protected_raw_string: _ => token(seq('r\\"', /([^\\\r\n]|\\[^"\r\n])*/, '\\"')),
    _inner: $ => choice(
      alias($._delimited_quantity, $.quantity), $._line_break,
      $.string, $.raw_string, $.number, $.boolean, $.keyword,
      $.type_identifier, $.identifier, $.operator, $.punctuation,
      $.interpolation_group,
      alias($._protected_string, $.string),
      alias($._protected_raw_string, $.raw_string),
      alias(choice('(', ')', '[', ']'), $.bracket),
    ),

    boolean: _ => choice('true', 'false'),

    keyword: _ => choice(...KEYWORDS.filter(k => !HEADS.includes(k))),

    type_identifier: _ => /[A-Z][\p{L}\p{N}_'₀-₉]*/,
    identifier: _ => /[\p{L}_][\p{L}\p{N}_'₀-₉]*/,

    operator: _ => token(choice(
      '|>', '<|', '->', '=>', '<-', '::', '...', '..=', '..', '==', '!=', '<=', '>=',
      '&&', '||', '≤', '≥', '≠', '√',
      /[+\-*\/^@!?=<>|·×÷−&:]/,
    )),

    punctuation: _ => choice(',', ';', '.', '_'),

    comment: _ => token(prec(-1, /#[^\-#\r\n][^\r\n]*|#/)),
    doc_comment: _ => token(prec(1, seq('##', /[^\r\n]*/))),
  },
});
