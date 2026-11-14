(comment) @comment
(block_comment) @comment
(doc_comment) @comment.doc

(string) @string
(raw_string) @string
(string_content) @string
(escape_sequence) @string.escape
(interpolation "{" @punctuation.special "}" @punctuation.special)

(number) @number
(boolean) @boolean
(keyword) @keyword
["fn" "module" "import" "open" "type" "class" "instance" "dimension" "unit"] @keyword

(unit_name) @constant
(unit_exponent) @number
(superscript) @number

(module_path (type_identifier) @type)
(function_declaration name: (identifier) @function)
(type_declaration name: (type_identifier) @type)
(dimension_declaration name: (type_identifier) @type)
(attribute "@" @attribute (identifier) @attribute)

(type_identifier) @type
(identifier) @variable
(operator) @operator
(punctuation) @punctuation.delimiter
(bracket) @punctuation.bracket
