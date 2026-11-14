(comment) @comment
(block_comment) @comment.block
(doc_comment) @comment.line.documentation

(string) @string
(raw_string) @string
(string_content) @string
(escape_sequence) @constant.character.escape
(interpolation "{" @punctuation.special "}" @punctuation.special)

(number) @constant.numeric
(boolean) @constant.builtin.boolean
(keyword) @keyword
["fn" "module" "import" "open" "type" "class" "instance" "dimension" "unit"] @keyword

(unit_name) @type.builtin
(unit_exponent) @constant.numeric
(superscript) @constant.numeric

(module_path (type_identifier) @namespace)
(function_declaration name: (identifier) @function)
(type_declaration name: (type_identifier) @type)
(dimension_declaration name: (type_identifier) @type)
(attribute "@" @attribute (identifier) @attribute)

(type_identifier) @type
(identifier) @variable
(operator) @operator
(punctuation) @punctuation.delimiter
(bracket) @punctuation.bracket
