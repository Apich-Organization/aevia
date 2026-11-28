'use strict';

const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const oniguruma = require('vscode-oniguruma');
const textmate = require('vscode-textmate');

let registry;
let grammar;
test.before(async () => {
  const wasm = fs.readFileSync(require.resolve('vscode-oniguruma/release/onig.wasm'));
  await oniguruma.loadWASM(wasm.buffer.slice(wasm.byteOffset, wasm.byteOffset + wasm.byteLength));
  const grammarPath = path.join(__dirname, '..', 'syntaxes', 'aevia.tmLanguage.json');
  registry = new textmate.Registry({
    onigLib: Promise.resolve(oniguruma),
    loadGrammar: async (scope) => scope === 'source.aevia'
      ? textmate.parseRawGrammar(fs.readFileSync(grammarPath, 'utf8'), grammarPath)
      : null,
  });
  grammar = await registry.loadGrammar('source.aevia');
  assert.ok(grammar);
});
test.after(() => registry?.dispose());

function tokenize(source) {
  let state = textmate.INITIAL;
  return source.split(/\r\n|\n|\r/).map((line) => {
    const result = grammar.tokenizeLine(line, state);
    state = result.ruleStack;
    return { line, tokens: result.tokens };
  });
}

function scopesAt({ line, tokens }, text) {
  const offset = line.indexOf(text);
  assert.notEqual(offset, -1, `missing ${JSON.stringify(text)} in ${JSON.stringify(line)}`);
  const token = tokens.find(({ startIndex, endIndex }) => startIndex <= offset && offset < endIndex);
  assert.ok(token, `no token at ${offset} in ${JSON.stringify(line)}`);
  return token.scopes;
}

test('ordinary strings preserve text, escapes and interpolation scopes', () => {
  const [line] = tokenize(String.raw`let label = "  ## text #- literal -# \0 \u{1F600} \{ {1 + 2} tail"`);
  assert.deepEqual(scopesAt(line, '## text'), ['source.aevia', 'string.quoted.double.aevia']);
  for (const escape of [String.raw`\0`, String.raw`\u{1F600}`, String.raw`\{`]) {
    assert.equal(scopesAt(line, escape).at(-1), 'constant.character.escape.aevia');
  }
  assert.equal(scopesAt(line, '{1 +').at(-1), 'punctuation.section.interpolation.begin.aevia');
  assert.equal(scopesAt(line, '1 +').at(-1), 'constant.numeric.aevia');
  assert.equal(scopesAt(line, '} tail').at(-1), 'punctuation.section.interpolation.end.aevia');
  assert.equal(scopesAt(line, 'tail').at(-1), 'string.quoted.double.aevia');
});

test('nested braces and each string delimiter close independently', () => {
  const [record, quoted, protectedLine, raw, nested, triple] = tokenize([
    'let record = "{ { first = { inner = 1 }, b = r"# raw {}" }.first.inner } tail"',
    'let quoted = "{show("one")} tail"',
    String.raw`let protected = "{show(\"one\") + show((\{ a = 1 \}).a)} tail"`,
    String.raw`let raw = "{format(r\"{} # text\", [5])} tail"`,
    String.raw`let nested = "{ "nested {[\"ok\", 2.75]}" } tail"`,
    String.raw`let triple = """before { [r"}\", "\{brace\} \"quote\"", """inside {1.25}"""] } tail"""`,
  ].join('\n'));
  assert.equal(scopesAt(record, '1 },').at(-1), 'constant.numeric.aevia');
  assert.equal(scopesAt(record, '}, b').at(-1), 'punctuation.section.group.end.aevia');
  assert.equal(scopesAt(record, '# raw').at(-1), 'string.quoted.raw.aevia');
  assert.equal(scopesAt(record, '}.first').at(-1), 'punctuation.section.group.end.aevia');
  assert.ok(scopesAt(record, '.first').includes('meta.interpolation.aevia'));
  assert.equal(scopesAt(record, '} tail').at(-1), 'punctuation.section.interpolation.end.aevia');
  assert.equal(scopesAt(quoted, 'one').at(-1), 'string.quoted.double.aevia');
  assert.equal(scopesAt(protectedLine, String.raw`\{ a`).at(-1), 'punctuation.section.group.begin.aevia');
  assert.equal(scopesAt(protectedLine, String.raw`\}).a`).at(-1), 'punctuation.section.group.end.aevia');
  assert.equal(scopesAt(protectedLine, 'one').at(-1), 'string.quoted.double.aevia');
  assert.equal(scopesAt(raw, '{} #').at(-1), 'string.quoted.raw.aevia');
  assert.equal(scopesAt(raw, '5').at(-1), 'constant.numeric.aevia');
  assert.equal(scopesAt(nested, '2.75').at(-1), 'constant.numeric.aevia');
  for (const line of [record, quoted, protectedLine, raw, nested]) {
    assert.deepEqual(scopesAt(line, 'tail'), ['source.aevia', 'string.quoted.double.aevia']);
  }
  assert.equal(scopesAt(triple, 'inside').at(-1), 'string.quoted.triple.aevia');
  assert.equal(scopesAt(triple, '1.25').at(-1), 'constant.numeric.aevia');
  assert.deepEqual(scopesAt(triple, 'tail'), ['source.aevia', 'string.quoted.triple.aevia']);
});

test('triple-quoted text and comments inside interpolation retain their scopes', () => {
  const [start, text, quotes, escaped, multilineText, , following, , code, close] = tokenize([
    'let label = """',
    '  # text',
    '  usual "quote" and ""doubled""',
    String.raw`  "{1} \0`,
    '  #- text -#',
    '  """',
    'let count = 2',
    'let message = "prefix #- literal -# {',
    '  1 #- block #- nested -# -# + 2 # code',
    '} suffix"',
  ].join('\n'));
  assert.equal(scopesAt(start, '"""').at(-1), 'string.quoted.triple.aevia');
  assert.equal(scopesAt(text, '# text').at(-1), 'string.quoted.triple.aevia');
  assert.equal(scopesAt(quotes, '""doubled').at(-1), 'string.quoted.triple.aevia');
  assert.equal(scopesAt(escaped, String.raw`\0`).at(-1), 'constant.character.escape.aevia');
  assert.equal(scopesAt(multilineText, '#- text').at(-1), 'string.quoted.triple.aevia');
  assert.equal(scopesAt(following, 'let').at(-1), 'keyword.control.aevia');
  assert.equal(scopesAt(code, '#- block').at(-1), 'comment.block.aevia');
  assert.equal(scopesAt(code, '# code').at(-1), 'comment.line.number-sign.aevia');
  assert.equal(scopesAt(close, '} suffix').at(-1), 'punctuation.section.interpolation.end.aevia');
  assert.equal(scopesAt(close, 'suffix').at(-1), 'string.quoted.double.aevia');
});

test('unknown escapes stay inside text and incomplete quotes recover on the next line', () => {
  const [invalid, ordinary, raw, following] = tokenize([
    String.raw`let invalid = "\q \u{Q} tail"`,
    'let ordinary = "never closed',
    String.raw`let raw = r"never closed \0 {x}`,
    'let after = 4',
  ].join('\r\n'));
  assert.equal(scopesAt(invalid, String.raw`\q`).at(-1), 'invalid.illegal.escape.aevia');
  assert.deepEqual(scopesAt(invalid, '{Q}'), ['source.aevia', 'string.quoted.double.aevia', 'invalid.illegal.escape.aevia']);
  assert.equal(scopesAt(invalid, 'tail').at(-1), 'string.quoted.double.aevia');
  assert.equal(scopesAt(ordinary, 'never').at(-1), 'string.quoted.double.aevia');
  assert.equal(scopesAt(raw, 'let').at(-1), 'keyword.control.aevia');
  assert.deepEqual(scopesAt(raw, String.raw`\0`), ['source.aevia', 'string.quoted.raw.aevia']);
  assert.deepEqual(scopesAt(raw, '{x}'), ['source.aevia', 'string.quoted.raw.aevia']);
  assert.equal(scopesAt(following, 'let').at(-1), 'keyword.control.aevia');
  assert.equal(scopesAt(following, '4').at(-1), 'constant.numeric.aevia');
});
