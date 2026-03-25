# Cfsurge CLI Release Commands

Use the commands for the user's OS only. These examples assume the binary was downloaded from GitHub Releases and run from the extracted directory.
Zip names differ by platform, but extracted executable names are unified: `cfsurge` on macOS/Linux and `cfsurge.exe` on Windows.

## macOS (Apple Silicon)

```sh
unzip ./cfsurge-darwin-arm64.zip
chmod +x ./cfsurge
./cfsurge --version
./cfsurge login
./cfsurge login --api-base https://api.example.com --username alice --password <PASSWORD>
./cfsurge login --api-base https://api.example.com --auth cloudflare-admin --token <TOKEN>
./cfsurge init
./cfsurge publish
./cfsurge list
./cfsurge remove
./cfsurge passwd --current-password <OLD> --new-password <NEW>
./cfsurge admin users list
```

## Linux x64

```sh
unzip ./cfsurge-linux-x64.zip
chmod +x ./cfsurge
./cfsurge --version
./cfsurge login
./cfsurge login --api-base https://api.example.com --username alice --password <PASSWORD>
./cfsurge login --api-base https://api.example.com --auth cloudflare-admin --token <TOKEN>
./cfsurge init
./cfsurge publish
./cfsurge list
./cfsurge remove
./cfsurge passwd --current-password <OLD> --new-password <NEW>
./cfsurge admin users list
```

## Windows x64 (PowerShell)

```powershell
Expand-Archive .\cfsurge-windows-x64.zip -DestinationPath .
.\cfsurge.exe --version
.\cfsurge.exe login
.\cfsurge.exe login --api-base https://api.example.com --username alice --password <PASSWORD>
.\cfsurge.exe login --api-base https://api.example.com --auth cloudflare-admin --token <TOKEN>
.\cfsurge.exe init
.\cfsurge.exe publish
.\cfsurge.exe list
.\cfsurge.exe remove
.\cfsurge.exe passwd --current-password <OLD> --new-password <NEW>
.\cfsurge.exe admin users list
```

## Notes

- First `login` prompts for `API base URL:` when `--api-base`, `CFSURGE_API_BASE`, and stored config are all absent.
- Default `login` mode is `service-session` and prompts `Username:` / `Password:` unless provided by flags or `CFSURGE_USERNAME`/`CFSURGE_PASSWORD`.
- When `--token` or `CFSURGE_TOKEN` is present, login defaults to `cloudflare-admin`.
- If `--auth service-session` is explicitly combined with token input, login fails with `token-based login requires --auth cloudflare-admin`.
- `init` writes `.cfsurge.json` in the current directory and stores `slug`, `publishDir`, and `visibility`.
- `init --visibility unlisted` can be used when the service supports obfuscated publish URLs.
- `publish` uses the positional directory argument first, then the `publishDir` field in `.cfsurge.json`.
- `publish` uses `--slug` first, then the `slug` field in `.cfsurge.json`.
- `publish` uses the `visibility` field in `.cfsurge.json` to choose `public` or `unlisted`.
- `remove` uses the positional slug first, then the `slug` field in `.cfsurge.json`.
- `list` prints TSV columns: `slug`, `visibility`, `servedUrl`, `activeDeploymentId`, `updatedAt`, `updatedBy`.
- `passwd` is available only for `service-session` logins.
- `admin users` supports `list`, `create`, `reset-password`, `disable`, and `enable`.
