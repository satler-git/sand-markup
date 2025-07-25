; === 名前定義 ===
((name_definition) @keyword.define)
((identifier_list) @variable)
((identifier) @variable)

; === セクション ===
((section) @heading)
((hashes) @punctuation.special)
((one_line_str) @string)

; === 全体適用 (apply_all) ===
((apply_all) @keyword)
((apply_all alias: (identifier) @variable))
((apply_all content: (string) @string))

; === 文定義 (sentence_definition) ===
((sentence_definition) @constant.builtin)
((sentence_definition alias: (identifier) @variable))
((sentence_definition content: (string) @string))

; === セレクター ===
((selector) @operator)
((selector (identifier) @variable))

; === 文字列・エスケープ ===
((string) @string)
((non_escaped_string) @string)

