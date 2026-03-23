# Cfsurge CLI Release Commands

Use the commands for the user's OS only. These examples assume the binary was downloaded from GitHub Releases and run from the extracted directory.
Zip names differ by platform, but extracted executable names are unified: `cfsurge` on macOS/Linux and `cfsurge.exe` on Windows.

## macOS (Apple Silicon)

```sh
unzip ./cfsurge-darwin-arm64.zip
chmod +x ./cfsurge
./cfsurge --version
./cfsurge login
./cfsurge login --api-base https://api.example.com
./cfsurge init
./cfsurge publish
./cfsurge list
./cfsurge remove
```

## Linux x64

```sh
unzip ./cfsurge-linux-x64.zip
chmod +x ./cfsurge
./cfsurge --version
./cfsurge login
./cfsurge login --api-base https://api.example.com
./cfsurge init
./cfsurge publish
./cfsurge list
./cfsurge remove
```

## Windows x64 (PowerShell)

```powershell
Expand-Archive .\cfsurge-windows-x64.zip -DestinationPath .
.\cfsurge.exe --version
.\cfsurge.exe login
.\cfsurge.exe login --api-base https://api.example.com
.\cfsurge.exe init
.\cfsurge.exe publish
.\cfsurge.exe list
.\cfsurge.exe remove
```

## Notes

- First `login` prompts for `API base URL:` when `--api-base`, `CFSURGE_API_BASE`, and stored config are all absent.
- `init` writes `.cfsurge.json` in the current directory and stores `slug`, `publishDir`, and `visibility`.
- `init --visibility unlisted` can be used when the service supports obfuscated publish URLs.
- `publish` uses the positional directory argument first, then the `publishDir` field in `.cfsurge.json`.
- `publish` uses `--slug` first, then the `slug` field in `.cfsurge.json`.
- `publish` uses the `visibility` field in `.cfsurge.json` to choose `public` or `unlisted`.
- `remove` uses the positional slug first, then the `slug` field in `.cfsurge.json`.
- `list` prints TSV columns: `slug`, `visibility`, `servedUrl`, `activeDeploymentId`, `updatedAt`, `updatedBy`.
