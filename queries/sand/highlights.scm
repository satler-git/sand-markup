; -*- scheme -*-
; Tree-sitter ハイライト定義 for Sand

; === 識別子 ===
((identifier) @variable)

; === 名前定義: `#(`, `)` ===
((name_definition "#("      ) @punctuation.special)
((name_definition ")"      ) @punctuation.bracket)
((identifier_list)          @variable)

; === セクション: `#alias## title` ===
((section "#"               ) @punctuation.special)
((section alias: (identifier) @variable))
((hashes)                   @punctuation.special)
; ((one_line_str)             @string)

; === 全体適用 (apply_all): `#{alias,{all,{…}}}` ===
((apply_all "#"             ) @punctuation.special)
((apply_all alias: (identifier) @variable))
((apply_all "{"             ) @punctuation.bracket)
((apply_all ","             ) @punctuation.separator)
((apply_all "}"             ) @punctuation.bracket)
((apply_all content: (string)     @string))

; === 文定義 (sentence_definition): `#alias[…][…]` ===
((sentence_definition "#"         ) @punctuation.special)
((sentence_definition alias: (identifier) @variable))
((sentence_definition "["         ) @punctuation.bracket)
((sentence_definition "]"         ) @punctuation.bracket)
((sentence_definition content: (string)    @string))

; === セレクター: `#.` `/` `.` ===
((selector "#."             ) @punctuation.special)
((selector "/"              ) @punctuation.special)
((selector "."              ) @punctuation.special)
((selector (identifier)     @variable))

; === 文字列・エスケープ ===
((string)           @string)
