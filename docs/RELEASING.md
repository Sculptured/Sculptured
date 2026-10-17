# Releasing

**What a release is:** the `sculptured` runner builds that `install.sh` and `install.ps1` download, plus the macOS desktop app. People get them from the hosted gateway's Computers page, which serves the builds at `/releases/` (`install.sh` fetches `sculptured-<target>.tar.gz` and its `.sha256`, checks the checksum, then installs).

There is no separate release step: each deploy of the server and each push to `main` produce the builds.

## Where each build comes from

| Build | Made by | When |
|---|---|---|
| Linux x86_64 runner | The gateway image (`sculptured-cloud/deploy/Gateway.Dockerfile`, Rust on Debian bullseye, so glibc 2.31 or newer), packaged with `deploy/release/package.sh` | Every server deploy |
| Windows x64 runner (`sculptured.exe`, `sculptured-background.exe`) | The same image, cross-compiled with MinGW (`x86_64-pc-windows-gnu`), for `install.ps1` | Every server deploy |
| macOS runners (Apple silicon and Intel) and `Sculptured-mac.dmg` (universal) | `.github/workflows/mac.yml` on GitHub's Apple silicon Macs, into the `mac-latest` prerelease | Every push to `main` |
| Linux arm64 runner | Not built. `install.sh` asks for it, and the gateway answers that there is no build for that system yet | – |

The server's update timer (`sculptured-cloud/deploy/aws/update.sh`) takes the macOS files from `mac-latest` once their `.commit` matches the commit it runs, checks their checksums, and serves them from `/data/releases`. Each deploy (and `deploy.sh --release-status`) warns when an uploaded build is from another commit.

The macOS app is signed ad hoc, so macOS asks once before it opens; a Developer ID and notarization would remove that.

## Cost

`mac.yml` runs on GitHub's macOS runners. They are free for public repositories; for a private one, macOS minutes count 10× against the monthly allowance. If the account would be billed, build on a Mac instead (below).

## Building by hand

A Linux build anywhere:

```sh
cargo build --release -p sculptured
sh deploy/release/package.sh x86_64-unknown-linux-gnu target/release/sculptured dist
```

The macOS builds, when GitHub can't make them:

```sh
git fetch && git checkout origin/main
deploy/release/build-mac.sh dist                                   # on the Mac
deploy/aws/deploy.sh --upload-release <folder with dist/*>         # in sculptured-cloud
```

`GATEWAY_RELEASES_URL` can point a gateway at any folder serving these files.

**The desktop app's installers** for Linux and Windows are built on each OS with `cd desktop && npm ci && npx electron-builder` (see `desktop/README.md`).

## Before a release goes to people

1. The suites pass: `cargo test --workspace`, `npm --prefix web test`, `npm --prefix docs-site test` (see [testing](internals/testing.md)).
2. `CHANGELOG.md` and `docs-site/changelog.mdx` describe what changed.
3. After the deploy, the Computers page offers each system's build, and `deploy.sh --release-status` reports no build from another commit.
4. [Release verification](RELEASE-VERIFICATION.md) records what was checked, and what was not.

## Deferred

- Linux arm64 builds;
- Windows on Arm;
- the desktop installers for Linux and Windows in CI;
- code signing (Apple Developer ID and notarization; a Windows certificate);
- tests on every push (only `mac.yml` runs on push).
