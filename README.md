# Sand

複数言語で表わされるテキストを推敲したりするためのDSLです。

構文や機能については `README.sand` を確認して下さい。

## Command

```shell
sand lsp # LSP serverを起動
sand out \#.ja --input README.sand # 日本語の文をプレーンテキストとして出力
sand out \#.en --markdown --input README.sand # 英語の文をマークダウンとして出力

sand parse README.sand # Debug用。パースしたASTを表示

source <(sand completions zsh) # Zsh向けの補完
```

