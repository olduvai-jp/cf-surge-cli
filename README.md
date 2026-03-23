# cfsurge CLI

`cfsurge` は、静的サイトをアップロードして公開・運用するためのコマンドラインツールです。  
ローカルのビルド成果物をまとめて配信し、公開中プロジェクトの一覧確認や削除まで CLI で完結できます。

## できること

- 静的ファイルをデプロイして公開 URL を発行
- 公開中プロジェクトの一覧表示 (`list`)
- プロジェクト削除 (`remove`) とログアウト (`logout`)
- 公開形態の選択:
  - `public`: 通常公開
  - `unlisted`: 共有 URL を知っている人のみアクセス

## インストール

### GitHub Releases からインストール

Releases から OS に合った zip を取得してください。

- `cfsurge-darwin-arm64.zip`
- `cfsurge-linux-x64.zip`
- `cfsurge-windows-x64.zip`
- `SHA256SUMS`

展開後、`cfsurge` (Windows は `cfsurge.exe`) を `PATH` の通った場所に配置します。

### ローカルビルド

```bash
cargo build --release
./target/release/cfsurge --help
```

## クイックスタート

### 1) ログイン

```bash
cfsurge login
```

初回は次のように尋ねられます。

- `API base URL:`
- `Cloudflare API token:`

成功すると `logged in as ...` が表示されます。  
トークン未指定時は、作成用 URL も表示されます。

`--api-base` と `--token` で非対話実行も可能です。

```bash
cfsurge login --api-base https://api.example.com --token <TOKEN>
```

### 2) プロジェクト設定を作成

```bash
cfsurge init --slug my-site --publish-dir dist
```

成功すると `.cfsurge.json` が作成され、`saved .cfsurge.json` が表示されます。  
`public` なら `public URL preview: ...`、`unlisted` なら `unlisted URL preview: ...` が表示されます。

`--visibility` は `public` または `unlisted` を指定できます。

```bash
cfsurge init --slug my-site --publish-dir dist --visibility unlisted
```

### 3) 公開

`.cfsurge.json` を使う場合:

```bash
cfsurge publish
```

ディレクトリを直接指定する場合:

```bash
cfsurge publish dist --slug my-site
```

成功時は `published <slug> -> <url>` が表示されます。

## 公開形態

- `public`: 通常の公開 URL でアクセスできます。
- `unlisted`: 共有 URL 経由でアクセスします。

`unlisted` は、接続先サーバーが対応している場合にのみ利用できます。  
未対応の場合、`publish` は `unlisted publish is not supported by this server` で失敗します。

## コマンド一覧

```text
login [--api-base <url>] [--token <token>] [--token-storage <file|keychain>]
init [--api-base <url>] [--slug <slug>] [--publish-dir <dir>] [--visibility <public|unlisted>]
publish [dir] [--slug <slug>]
--version
list
remove [slug]
logout
```

`login` の既定保存先は `file` で、`--token-storage keychain` を明示した場合のみ macOS Keychain を利用します。

`list` は TSV 形式で 1 行ずつ出力します。

```text
<slug>\t<visibility>\t<served/public url>\t<activeDeploymentId>\t<updatedAt>\t<updatedBy>
```

## 設定ファイル

### グローバル設定

`~/.config/cfsurge/config.json`

保存される主なキー:

- `apiBase`
- `tokenStorage`
- `token` (`tokenStorage=file` のとき)

### プロジェクト設定

カレントディレクトリの `.cfsurge.json`

保存される主なキー:

- `slug`
- `publishDir`
- `visibility`

## 環境変数

- `CFSURGE_API_BASE`: API base URL を上書き
- `CFSURGE_TOKEN`: API token を上書き
- `CFSURGE_CLI_VERSION`: `--version` 表示値の注入用 (主にビルド/リリース用途)

## トラブルシュート

- `not logged in. Run cfsurge login.`  
  先に `cfsurge login` を実行してください。
- `invalid API base URL: expected absolute http(s) URL ...`  
  `https://api.example.com` の形式で指定してください。
- `invalid API base URL: do not include path, query, or fragment`  
  `/v1` や `?x=1` を付けず、オリジンだけを指定してください。
- `publish target has no files`  
  `publishDir` (または `publish` で指定したディレクトリ) に配信ファイルがあるか確認してください。
- `invalid visibility: expected public or unlisted`  
  `--visibility` は `public` か `unlisted` のみ指定できます。
