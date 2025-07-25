/**
 * @file Sand grammar for tree-sitter
 * @author satler <satler@satler.dev>
 * @license MIT
 */

/// <reference types="tree-sitter-cli/dsl" />
// @ts-check

module.exports = grammar({
	name: "sand",

	rules: {
		source_file: ($) =>
			repeat(
				choice(
					$.name_definition,
					$.section,
					$.apply_all,
					$.sentence_definition,
					$.selector,
					$.non_escaped_string,
				),
			),

		// 名前定義: #(en, ja)
		name_definition: ($) =>
			seq(
				"#(",
				field("languages", $.identifier_list),
				")",
				/\s*/, // 行末の空白を許容
			),

		identifier_list: ($) => seq($.identifier, repeat(seq(",", $.identifier))),

		// セクション: #alias## title
		section: ($) =>
			seq(
				"#",
				optional(field("alias", $.identifier)),
				field("hashes", $.hashes),
				field("title", $.one_line_str),
				/\s*/, // 行末の空白を許容
			),

		hashes: ($) => /#+/,
		one_line_str: ($) => /[^\n]+/,

		// 全体適用: #{all, { content }}
		apply_all: ($) =>
			seq(
				"#",
				optional(field("alias", $.identifier)),
				"{",
				optional(seq(choice("all", seq("[", $.identifier_list, "]")), ",")),
				"{",
				field("content", $.string),
				"}",
				"}",
				/\s*/, // 行末の空白を許容
			),

		// 文定義: #alias[content1][content2]
		sentence_definition: ($) =>
			seq(
				"#",
				optional(field("alias", $.identifier)),
				repeat1(seq("[", field("content", $.string), "]")), // stringをrepeat1で囲む
				/\s*/, // 行末の空白を許容
			),

		// セレクター: #.path.to.element
		selector: ($) =>
			seq(
				"#.",
				optional("/"),
				optional(
					seq($.identifier, repeat(seq(".", $.identifier)), optional(".")),
				),
				/\s*/, // 行末の空白を許容
			),

		// 文字列 (角括弧やバックスラッシュを含む可能性のあるコンテンツ)
		string: ($) => repeat1($._char),
		_char: ($) =>
			choice(
				seq("\\", choice("]", "\\", "}", "n")),
				/[^\]}\\]/, // ]と}と\以外の文字
			),

		// エスケープされていない文字列
		non_escaped_string: ($) => prec.right(repeat1($._non_escaped_char)),
		_non_escaped_char: ($) =>
			choice(
				seq("\\", choice("#", "\\", "/", "n")),
				/[^#\\]/, // #と\以外の文字
			),

		identifier: ($) => /[a-zA-Z0-9_]+/,
	},
});
