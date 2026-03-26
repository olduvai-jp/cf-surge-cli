# cfsurge CLI

`cfsurge` は、静的サイトをアップロードして公開・運用するためのコマンドラインツールです。  
ローカルのビルド成果物をまとめて配信し、公開中プロジェクトの一覧確認や削除まで CLI で完結できます。

## できること

- 静的ファイルをデプロイして公開 URL を発行
- 公開中プロジェクトの一覧表示 (`list`)
- プロジェクト削除 (`remove`) とログアウト (`logout`)
- サービスユーザーのパスワード変更 (`passwd`)
- 管理者向けユーザー管理 (`admin users ...`)
- 公開形態の選択:
  - `public`: 通常公開
  - `basic`: 同じ公開 URL で HTTP Basic 認証を要求

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

既定では `service-session` モードでログインします。初回は次のように尋ねられます。

- `API base URL:`
- `Username:`
- `Password:`

成功すると `logged in as ...` が表示されます。

```bash
cfsurge login --api-base https://api.example.com --username <USERNAME> --password <PASSWORD>
```

パスワード変更必須 (`mustChangePassword`) のアカウントでは、`login` がそのまま変更フローを完了できます。
非対話実行では `--new-password` を指定してください。

```bash
cfsurge login --api-base https://api.example.com --username <USERNAME> --password <TEMP_PASSWORD> --new-password <NEW_PASSWORD>
```

Cloudflare API token を指定すると、`cloudflare-admin` モードが自動選択されます。
明示する場合は次のように実行できます。

```bash
cfsurge login --api-base https://api.example.com --auth cloudflare-admin --token <TOKEN>
```

トークン未指定の `cloudflare-admin` ログインでは、トークン作成用 URL と `Cloudflare API token:` プロンプトが表示されます。

### 2) プロジェクト設定を作成

```bash
cfsurge init --slug my-site --publish-dir dist
```

成功すると `.cfsurge.json` が作成され、`saved .cfsurge.json` が表示されます。  
`public URL preview: ...` が表示されます。

`--access` は `public` または `basic` を指定できます。

```bash
cfsurge init --slug my-site --publish-dir dist --access basic
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

`publish` の進捗は `stderr` に出力されます。TTY ではスピナー付きで 1 行更新、非 TTY では改行付きログとして出力されます。  
成功時の最終行 `published <slug> -> <url>` は `stdout` に出力されます。

`access=basic` の場合は `publish` 実行時に毎回、次の環境変数が必須です。

- `CFSURGE_BASIC_AUTH_USERNAME`
- `CFSURGE_BASIC_AUTH_PASSWORD`

## 公開形態

- `public`: 通常の公開 URL でアクセスできます。
- `basic`: 通常の公開 URL で HTTP Basic 認証を要求します。

## コマンド一覧

```text
login [--api-base <url>] [--auth <service-session|cloudflare-admin>] [--username <username>] [--password <password>] [--new-password <password>] [--token <token>] [--token-storage <file|keychain>]
init [--api-base <url>] [--slug <slug>] [--publish-dir <dir>] [--access <public|basic>]
publish [dir] [--slug <slug>]
--version
list
remove [slug]
passwd [--current-password <password>] [--new-password <password>]
admin users list
admin users create --username <username> [--role <user|admin>] [--temporary-password <password>]
admin users reset-password <username>
admin users disable <username>
admin users enable <username>
logout
```

`login` の既定保存先は `file` で、`--token-storage keychain` を明示した場合のみ macOS Keychain を利用します。  
`passwd` は `service-session` ログイン時のみ利用でき、成功時は自動再ログインされます。

`list` は TSV 形式で 1 行ずつ出力します。

```text
<slug>\t<access>\t<served/public url>\t<activeDeploymentId>\t<updatedAt>\t<updatedBy>
```

## 設定ファイル

### グローバル設定

`~/.config/cfsurge/config.json`

保存される主なキー:

- `apiBase`
- `auth.type` (`service-session` または `cloudflare-admin`)
- `auth.tokenStorage`
- `auth.accessToken` (`tokenStorage=file` のとき)
- `auth.actor`
- `auth.username`
- `auth.role`
- `auth.mustChangePassword`
- `tokenStorage` と `token` (旧形式。`cloudflare-admin` 互換のため併存)

### プロジェクト設定

カレントディレクトリの `.cfsurge.json`

保存される主なキー:

- `slug`
- `publishDir`
- `access`

## 環境変数

- `CFSURGE_API_BASE`: API base URL を上書き
- `CFSURGE_TOKEN`: API token を上書き
- `CFSURGE_USERNAME`: `service-session` ログインの username を上書き
- `CFSURGE_PASSWORD`: `service-session` ログインの password を上書き
- `CFSURGE_BASIC_AUTH_USERNAME`: `access=basic` publish の Basic 認証 username
- `CFSURGE_BASIC_AUTH_PASSWORD`: `access=basic` publish の Basic 認証 password
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
- `invalid access: expected public or basic`  
  `--access` は `public` か `basic` のみ指定できます。
- `--visibility is no longer supported. Use --access <public|basic>.`  
  `--visibility` は廃止されました。`--access` を使ってください。
- `basic publish requires CFSURGE_BASIC_AUTH_USERNAME and CFSURGE_BASIC_AUTH_PASSWORD`  
  `access=basic` の publish では 2 つの環境変数が必須です。
- `invalid basic auth username: expected non-empty printable ASCII without ':'`  
  username は空文字不可・表示可能 ASCII のみ・`:` を含められません。
- `invalid basic auth password: expected non-empty printable ASCII`  
  password は空文字不可・表示可能 ASCII のみ指定できます。
- `password change required. Run cfsurge passwd.`  
  `service-session` ログイン後に初回変更が必須な状態です。`cfsurge passwd` を実行してください。
- `password change required for this account. Re-run cfsurge login with --new-password <password>.`  
  非対話モードの `login` でパスワード変更必須アカウントを処理する場合は `--new-password` を指定してください。
- `--new-password is only available with service-session login`  
  `--new-password` は `service-session` ログイン時のみ利用できます。
- `password updated, but automatic re-login failed. Run cfsurge login with your new password.`  
  `passwd` 後の自動再ログインに失敗しています。新しいパスワードで `cfsurge login` を実行してください。
- `token-based login requires --auth cloudflare-admin`  
  `--token` または `CFSURGE_TOKEN` を使うと、通常は `cloudflare-admin` が自動選択されます。`--auth service-session` を明示しつつ token を指定した場合にこのエラーになります。
