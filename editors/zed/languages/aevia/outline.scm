(function_declaration
  "fn" @context
  name: (identifier) @name) @item

(type_declaration
  name: (type_identifier) @name) @item

(dimension_declaration
  "dimension" @context
  name: (type_identifier) @name) @item

(module_declaration
  "module" @context
  path: (module_path) @name) @item
