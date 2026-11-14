# 01 — Lexical structure

This chapter defines how a source file is split into tokens. Syntax is chapter 02. The
semantics of unit literals is chapter 05; this chapter defines only how they are recognised.

## 1.1 Source files and encoding

- The extension is `.aev`; the encoding is UTF-8. A BOM is allowed and ignored.
- `\n`, `\r\n` and `\r` are all normalised to `\n` before tokenising.
- Identifiers (including keywords and unit names) are compared in Unicode NFC: a precomposed
  `é` and `e` plus a combining accent are the same name. Strings and comments are kept as
  written.
- The first line may be a shebang (`#!` to end of line); it is ignored.
- A tab MUST NOT appear in leading indentation (`E0101`). Tabs in strings and comments are
  unrestricted.

## 1.2 Tokens

```
token ::= IDENT | UIDENT | KEYWORD | INT | FLOAT | STRING
        | UNIT_NAME | SUPERSCRIPT | LT_APP | ELLIPSIS
        | OP | LPAREN | RPAREN | LBRACKET | RBRACKET | LBRACE | RBRACE
        | COMMA | SEMICOLON | COLON | DOT | DOTDOT | DOTDOTEQ
        | ARROW | FATARROW | LARROW | PIPE | EQUALS
        | NEWLINE | INDENT | DEDENT | EOF
```

Whitespace separates tokens, and newlines also take part in layout (§1.4). Apart from unit
literals there is no implicit multiplication by juxtaposition: `3 x` is not a product and
`f x` is not a call. Calls are written `f(x)` and products `3 * x`.

## 1.3 Comments

```aevia
# line comment
let x = 1  # trailing comment

#- block comment
   #- may nest -#
   still inside -#
```

- `#` starts a line comment. `#-` … `-#` is a nestable block comment; an unclosed one is
  `E0104`.
- `##` starts a doc comment (Markdown). It must begin a line (indentation allowed) and attaches
  to the next declaration.
- Comments produce no tokens.

## 1.4 Layout

Aevia uses the offside rule: blocks are delimited by indentation alone; there is no `end`.

### 1.4.1 INDENT / DEDENT

The lexer keeps an indentation stack, initially `[0]`. For each **logical line** (outside
brackets, not blank, not comment-only):

1. Count the leading spaces `n`.
2. `n >` top: push `n` and emit `INDENT`.
3. `n ==` top: emit nothing.
4. `n <` top: pop, emitting one `DEDENT` per pop, until top `== n`; if the stack empties
   first, report `E0102`.
5. Emit `NEWLINE` at the end of the line.

Blank and comment-only lines do not affect indentation and emit no `NEWLINE`.

### 1.4.2 Free layout inside brackets

Inside `(` `[` `{` no `NEWLINE`/`INDENT`/`DEDENT` is produced, and a trailing comma is
allowed:

```aevia
let A = [[1.0, 2.0],
         [3.0, 4.0]]

let B = [
    1.0, 2.0,
    3.0, 4.0,
]
```

### 1.4.3 Continuation

Outside brackets nothing suppresses the line end: every line ends with `NEWLINE`, and a
trailing `+` does not join the next line (`1 +` followed by a newline is `E0202`). A
multi-line expression MUST be parenthesised:

```aevia
let energy = (m * c^2
            + 1/2 * m * v^2
            - m * g * h)
```

When a line ends with a block-introducing token (`=`, `then`, `else`, `->`, `=>`, `do`,
`with`) and the next line is indented, the parser reads `NEWLINE INDENT expr DEDENT` as a
**layout group**. Layout groups are used for function and `let` bodies, `if` branches, `match`
arm bodies and `do` statement bodies. This keeps the lexer ignorant of which tokens are
operators.

### 1.4.4 Blocks

A block is also `NEWLINE INDENT … DEDENT`, containing a sequence of `let`/`fn` declarations and
a final expression; its value is the final expression's value, and a block of only `let`s is
`E0201`. After `NEWLINE INDENT` the parser chooses a block if the first token is `let` or `fn`,
and a layout group otherwise:

```aevia
fn mean<d>(xs: Tensor<1, Real[d]>): Real[d] =
    let n = length(xs)
    sum(xs) / n
```

A position that requires an indented block but has none is `E0103`. In a multi-line `if`,
`else` MUST be in the same column as `if`. The `|` arms of one `match` MUST share a column,
either the column of `match` or one level deeper (§2.3.4).

## 1.5 Identifiers and keywords

### 1.5.1 Identifiers

```
ident_start    ::= XID_Start | "_"
ident_continue ::= XID_Continue | "'" | "₀".."₉"
```

- Unicode letters (`Δt`, `λ`), primes (`f'`) and subscript digits (`x₁`) are allowed; names
  are case-sensitive.
- A name starting with an ASCII capital is a `UIDENT` (types, constructors, modules,
  dimensions); all others are `IDENT`. So `Δt` is a value name, not a constructor.
- A lone `_` is the wildcard. As a call argument it is a placeholder (§2.3.6). It is not a
  binding name.

### 1.5.2 Keywords

```
as       class     derive    dimension  do       else     false
fn       for       if        import     in       instance let
match    module    open      private    rec      test     then
true     type      unit      where      with
```

A keyword cannot be used as an identifier (`E0206`). `forall` is a contextual keyword: it is
a quantifier only at the start of a type, followed by type parameters and `.`, and it is also
a standard-library function name (§8.13). `Int`, `Real`, `Float`, `Bool` and the like are
treated specially only in type position; they are not keywords. `in` belongs to `let … in`,
`for` to `instance … for` and `derive … for`, and `rec` to `let rec`. `pure` is a Prelude
function, not a keyword (§9.7.3).

> **Status:** backtick identifiers (naming an external item that collides with a keyword) are
> not implemented; a backtick is `E0105`.

### 1.5.3 Naming conventions (informative)

Types, constructors, modules and dimensions use `UpperCamelCase`; functions, values and fields
use `snake_case`; type parameters are lower-case `a b`, dimension parameters `d e`; unit names
keep their international spelling (`m`, `kg`, `Pa`, `°C`).

## 1.6 Numeric literals

```
int_lit   ::= dec_int | "0x" hexdigit {["_"] hexdigit}
            | "0o" octdigit {["_"] octdigit} | "0b" bindigit {["_"] bindigit}
dec_int   ::= digit { ["_"] digit }
float_lit ::= dec_int "." dec_int [ exponent ] | dec_int exponent
exponent  ::= ("e" | "E") ["+" | "-"] dec_int
```

- Underscores only group digits; they cannot lead, trail or repeat (`E0106`). `1_000` is
  valid; `1__0` and `1_` are not.
- A decimal integer is an `Int`; a literal with a point or exponent is a `Float`. `0755` is
  decimal 755. There are no numeric suffixes, and `1.` and `.5` are invalid.
- `oo`, `-oo` and `nan` are predefined constants, not literals. `i` is the predefined imaginary
  unit; write a complex number as `3 + 4 * i`.
- `0` is not dimension-polymorphic: it defaults to `Int`. Write zero metres as `0 m` and a
  floating zero as `0.0` (§4.7).

## 1.7 String literals

```aevia
"hello"
"value = {x + 1}"
"line\nbreak"
r"C:\data\raw.csv"          # raw string: no escapes, no interpolation
"""
multi-line string
  keeps relative indentation
"""
```

- Strings are delimited by double quotes; a single quote is not a string delimiter (`E0105`).
- Escapes: `\n \t \r \\ \" \{ \} \u{1F600}`. An unknown escape is `E0105`.
- Interpolation `{expr}`: the expression's type MUST implement `Show`. An empty `{}` is
  `E0104`. A literal brace is written `\{`.
- A raw string `r"…"` processes neither backslashes nor braces and cannot contain `"`.
- A multi-line string `"""…"""` drops the leading newline and the common indentation of its
  lines; interior newlines are kept.
- A string is an immutable UTF-8 sequence; indexing counts grapheme clusters (§8.1.4).

## 1.8 Unit literals

```aevia
9.80665 m/s^2
3.0e8 m/s
1.5 kg*m^2/s^3
25 °C
50 %
2.5 µm
```

### 1.8.1 Recognition

**An identifier sequence immediately after a numeric literal is always a unit literal, never a
product.**

```aevia
3 m          # a quantity: 3 metres
3 * m        # the value 3 times the unit value m; display unit may differ
3 x          # a unit literal; E0501 if x is not a unit
```

The rule is context-free: neither lexer nor parser needs to know which names are units. Name
resolution then checks that `x` lives in the unit namespace, so unit declarations may come
later and the parser never backtracks.

```
quantity_lit ::= number unit_expr
unit_expr    ::= unit_factor { ("*" | "·" | "/") unit_factor }
unit_factor  ::= unit_atom [ "^" exponent | SUPERSCRIPT+ ]
unit_atom    ::= unit_name | "(" unit_expr ")" | "%"
unit_name    ::= UNIT_NAME | IDENT | qname
exponent     ::= ["-"] integer | "(" rational ")"
```

- `number` is an `INT` or `FLOAT` of §1.6. A unit literal starts when the numeric literal is
  immediately followed by an `IDENT`, a `UNIT_NAME` (`°C`), `%` or `(`.
- `*` and `·` are synonyms; `/` is left-associative: `kg*m^2/s^3` = `(kg*m^2)/s^3`.
- An exponent MUST be an integer literal or a parenthesised rational: `m^2`, `s^-1`,
  `m^(1/2)`. Variable exponents are not allowed (use `Dyn`, §4.9). Superscripts `m²`, `s⁻¹`,
  `m³·s⁻²` are equivalent to `^n` (`E0107`: invalid sequence).
- Juxtaposition inside a unit expression is not allowed: `kg m^2` is not `kg*m^2`.
- **Inside a unit expression, `*` `·` `/` MUST touch both neighbours.** A `*` or `/` with
  whitespace on either side ends the unit literal and is ordinary arithmetic:
  `9.81 m/s^2 * mass` = `(9.81 m/s^2) * mass`; `70 kg * v^2 / 2` = `((70 kg) * v^2) / 2`;
  `3 m * x` = `(3 m) * x`. The tight `3 m*x` is still a unit expression and is `E0501` when `x`
  is not a unit. The formatter always prints units tight and arithmetic operators spaced, so
  formatting never changes meaning.
- Numbers are not allowed inside a unit expression: `3 m/2 s` is `(3 m) / (2 s)`; `1/2 m` is
  `1 / (2 m)` with type `Real[1/L]`; write "half a metre" as `(1/2) m`.
- Parentheses are for units only: `3 (m/s)` is valid; `3 (2)` is `E0108`; a malformed unit in
  parentheses such as `3 ()` is `E0209`.

### 1.8.2 Unit names and qualification

- Unit names are resolved in the **unit namespace**, separate from values: `let s = 5` does not
  affect `3 s`; to multiply by the variable write `3 * s`.
- A name may carry a prefix (`km`, `MHz`, `µs`), matched longest-first (§5.3), and is
  case-sensitive (`Pa` ≠ `pa`).
- A name may be qualified: `3 Units.SI.km` is valid; `3 p.x` is not (`p` is not a unit).
- `%` is a unit, not a modulo operator: `50%` and `50 %` are both the `Real[1]` value 0.5;
  `50 % 3` is a syntax error. Use `mod(a, b)` or `rem(a, b)` for remainders.

### 1.8.3 Superscripts and special characters

The superscript characters `⁰¹²³⁴⁵⁶⁷⁸⁹ ⁻ ⁺` may only follow a unit name; `x²` is not an
expression — write `x^2`.

| Written | Meaning |
|---|---|
| `°C` `°F` | Celsius, Fahrenheit (affine, §5.6) |
| `°` alone | the angle unit `deg` |
| `Ω` / `Å` | `ohm` / `angstrom` |
| `µ` (U+00B5), `μ` (U+03BC) | both are the micro prefix, the same prefix |

### 1.8.4 Type and value (preview)

| Literal | Default type | Value |
|---|---|---|
| `3 m` | `Real[L]` (instantiates to `Float[L]` where one is expected) | 3 (coherent SI) |
| `3.0 m` | `Float[L]` (may be exact where `Real[L]` is expected) | 3.0 |
| `25 °C` | `Point[Θ]` | 298.15 K (§5.6) |
| `1.5 kg*m^2/s^3` | `Float[M*L^2/T^3]` | 1.5 |
| `50 %` | `Real[1]` (`Float[1]` where a `Float` is expected) | 0.5 |

The display unit (`m`, `km`, `°C`) is not part of the type. It is metadata on the value, and
`to(unit, x)` changes it (§5.7).

## 1.9 Operators and punctuation

Precedence and associativity are in §2.6.

| Token | Meaning |
|---|---|
| `+ - * /` `@` `^` | arithmetic, matrix product, power |
| `== != < <= > >=` | comparison |
| `&&` `\|\|` `!` | logical and, or, not; postfix `!` is the factorial (§2.7.5) |
| `\|>` `<\|` | pipe, reverse application |
| `::` | list cons |
| `..` `..=` `...` | half-open and closed ranges (index position only), record row variable |
| `->` `=>` `<-` `=` `:` | function type / match arm, lambda, `do` bind, definition, annotation |
| `?` `√` | postfix error propagation, prefix square root |

Synonyms are normalised to ASCII tokens in the lexer, keeping the source position for
diagnostics and formatting: `·` `×` = `*`, `÷` = `/`, `−` = `-`, `≤` `≥` `≠` = `<=` `>=`
`!=`. `%` appears only inside unit literals.

## 1.10 Lexical errors

| Code | Situation |
|---|---|
| `E0101` | tab in indentation |
| `E0102` | inconsistent indentation (no matching level for a DEDENT) |
| `E0103` | indented block required but absent |
| `E0104` | unclosed string, block comment or bracket; empty `{}` interpolation |
| `E0105` | invalid character or unknown escape |
| `E0106` | invalid numeric literal (underscore placement, no digits) |
| `E0107` | invalid superscript sequence |
| `E0108` | missing unit after a numeric literal |

A lexical error MUST report an exact line and column range and SHOULD suggest a fix.
