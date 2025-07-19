use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "sand.pest"]
pub struct SandParser;

#[cfg(test)]
mod tests {
    use crate::parser::{Rule, SandParser};
    use pest::Parser as _;

    #[test]
    fn simple_parse() -> Result<(), Box<dyn std::error::Error>> {
        let parsed = SandParser::parse(
            Rule::doc,
            r#"
#(en, ja)

つまり何個でも伸ばせる。ただしこの定義は一つ。先頭にあるといい

#sentence## 文

markdownへの変換のみに関連するaliasつきのセクション

#s1[
	Thank you!
][
	ありがとう！
]

### セレクター

エイリアスなし
\#\# セレクター

#s2[
	I am sleepy.
][
	眠いです。
]

#.sentence.s1. に関連するんだけどさ〜(ここでKかgdで情報表示)。#.sentence.s1.ja 違う

トップレベルのja, enみたいなのは省略可能(すべてを指す)

#.
これで全ての文書
#.en
英文全体
#.ja
ここでは日本語文全体

#./s2. みたいにして相対

#[
	I got it.
][
	よし！
]

#./0.ja のようにもできる。./は最初の.のあとにだけ可能性がある。
セクションごとに0-indexedにふる。補完を出したい。

#{{ \n }}

みたいにすることで全体に適用できる。[]の中は改行もtrimするから、改行をいれるには\nがいる。

#{{ \n }}

は

#{all, { \n }}

に意味的に等しく、

#{[ja], { \n }}

ともできる
            "#,
        );

        match parsed {
            Ok(p) => {
                dbg!(&p);
            }
            Err(ref e) => {
                eprintln!("{e}");
                parsed?;
            }
        }
        Ok(())
    }
}
