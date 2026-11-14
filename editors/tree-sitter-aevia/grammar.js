/**
 * A lexical tree-sitter grammar for Aevia.
 *
 * Aevia is layout-sensitive (offside rule, spec 1.4) and its unit literals
 * are context-free at the lexical level (spec 1.8.1: a number followed by
 * identifiers is a quantity). Editors need highlighting, brackets and
 * declaration heads, not a full parse tree; everything semantic comes from
 * `aevia-lsp` through semantic tokens. So this grammar recognises tokens and
 * the few declaration heads that benefit from a capture, and never fails.
 */
const KEYWORDS = [
  'as', 'class', 'derive', 'dimension', 'do', 'else', 'fn', 'for', 'if',
  'import', 'in', 'instance', 'let', 'match', 'module', 'open', 'private',
  'rec', 'test', 'then', 'type', 'unit', 'where', 'with',
];

const HEADS = ['fn', 'module', 'import', 'open', 'type', 'class', 'instance', 'dimension', 'unit'];

module.exports = grammar({
  name: 'aevia',

  extras: $ => [/\s+/, $.comment, $.doc_comment, $.block_comment],

  externals: $ => [$.block_comment],

  word: $ => $.identifier,

  rules: {
    source_file: $ => repeat($._item),

    _item: $ => choice(
      $.module_declaration,
      $.import_declaration,
      $.function_declaration,
      $.type_declaration,
      $.dimension_declaration,
      $.unit_declaration,
      $.attribute,
      $.quantity,
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
      $.bracket,
    ),

    module_declaration: $ => prec.right(seq('module', optional(field('path', $.module_path)))),
    import_declaration: $ => prec.right(seq(choice('import', 'open'), optional(field('path', $.module_path)))),

    module_path: $ => seq($.type_identifier, repeat(seq(token.immediate('.'), $.type_identifier))),

    function_declaration: $ => prec.right(seq('fn', optional(field('name', $.identifier)))),

    type_declaration: $ => prec.right(seq(choice('type', 'class', 'instance'), optional(field('name', $.type_identifier)))),
    dimension_declaration: $ => prec.right(seq('dimension', optional(field('name', $.type_identifier)))),
    unit_declaration: $ => prec.right(seq('unit', optional(field('name', $.unit_name)))),

    attribute: $ => seq('@', $.identifier),

    // spec 1.8.1: a number followed by identifier text is a unit literal.
    quantity: $ => prec.right(seq($.number, $.unit_expression)),
    unit_expression: $ => seq(
      $._unit_factor,
      repeat(seq(token.immediate(/[*·\/]/), $._unit_factor)),
    ),
    _unit_factor: $ => choice(
      seq($.unit_name, optional(choice(seq(token.immediate('^'), $.unit_exponent), $.superscript))),
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

    string: $ => choice(
      seq('"""', repeat(choice($.escape_sequence, $.interpolation, $.string_content)), '"""'),
      seq('"', repeat(choice($.escape_sequence, $.interpolation, $.string_content)), '"'),
    ),
    string_content: _ => token(prec(-1, /[^"\\{]+|"[^"]|""[^"]/)),
    raw_string: _ => token(seq('r"', /[^"]*/, '"')),
    escape_sequence: _ => token(/\\(u\{[0-9a-fA-F]+\}|[ntr\\"{}])/),
    interpolation: $ => seq('{', repeat($._inner), '}'),
    _inner: $ => choice(
      $.quantity, $.string, $.raw_string, $.number, $.boolean, $.keyword,
      $.type_identifier, $.identifier, $.operator, $.punctuation,
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
    bracket: _ => choice('(', ')', '[', ']', '{', '}'),

    comment: _ => token(prec(-1, /#[^\-#\n][^\n]*|#/)),
    doc_comment: _ => token(prec(1, seq('##', /[^\n]*/))),
  },
});
