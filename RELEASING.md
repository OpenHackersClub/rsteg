# Releasing rsteg

rsteg ships on two channels:

- **crates.io** — `cargo install rsteg-cli` and `rsteg-core`/adapters as libraries.
- **Prebuilt binaries** — GitHub Releases. Download the `.tar.gz` (unix) or `.zip` (Windows) for your platform from the [Releases page](https://github.com/OpenHackersClub/rsteg/releases), extract it, and drop the `rsteg` binary somewhere on your `PATH`. Each release also ships a `SHA256SUMS` file for integrity verification.

All publishable crates share a single version in `[workspace.package]` and are released in lockstep. Path deps use `version = "=X.Y.Z"` (exact match), so we only ever publish matched sets.

## Publishable crates (in dep order)

1. `rsteg-core`
2. `rsteg-bmp`, `rsteg-wav`, `rsteg-png`, `rsteg-crypto-aead` (independent; any order)
3. `rsteg-cli` (must be last; depends on all of the above)

`rsteg-bench` and `rsteg-site-build` are marked `publish = false` and are skipped.

## Prerequisites

- `CARGO_REGISTRY_TOKEN` — crates.io API token with publish permission. Set it in your shell before step 5 (or pass `--token` on each `cargo publish`).
- GitHub write permission on `OpenHackersClub/rsteg` for pushing the tag.
- A clean working tree on `main` at the commit you want to release from.

## Release procedure

### 1. Bump the version

Edit `Cargo.toml` and bump `[workspace.package].version`:

```toml
[workspace.package]
version = "0.2.0"   # was 0.1.0
```

Then update the `=X.Y.Z` constraint on every path dep across the 6 publishable crates so they match. Search for the old version:

```sh
rg '"=0\.1\.0"' crates/
```

Rebuild to make sure the lockfile updates:

```sh
cargo build --workspace --locked
```

### 2. Dry-run publish locally

Run the publish dry-run for `rsteg-core` (this is what CI also runs on every PR):

```sh
cargo publish -p rsteg-core --dry-run --locked --no-verify
```

For the downstream crates, the full dry-run can't succeed until `rsteg-core` is actually on crates.io at the new version. As a pre-release sanity check, validate the manifests and file sets offline:

```sh
for c in rsteg-bmp rsteg-wav rsteg-png rsteg-crypto-aead rsteg-cli; do
  cargo package -p "$c" --list --no-verify --allow-dirty --offline
done
```

### 3. Commit and tag

```sh
git add -A
git commit -m "chore(release): vX.Y.Z"
git tag vX.Y.Z
git push origin main
git push origin vX.Y.Z
```

### 4. Watch the release workflow

Pushing `vX.Y.Z` triggers `.github/workflows/release.yml`. It:

- builds `rsteg-cli` for 5 targets (`{x86_64,aarch64}-apple-darwin`, `{x86_64,aarch64}-unknown-linux-gnu`, `x86_64-pc-windows-msvc`)
- produces `.tar.gz` / `.zip` archives with `rsteg` + `README.md` + `LICENSE*`
- generates an aggregated `SHA256SUMS` file
- uploads everything to a new GitHub Release named `vX.Y.Z` with auto-generated notes

Follow it with `gh run watch` or the Actions tab.

### 5. Publish to crates.io

Once the GitHub Release exists and the release workflow is green, publish to crates.io in dep order. **`rsteg-core` must be first; `rsteg-cli` must be last** so path+version deps resolve from the registry.

```sh
export CARGO_REGISTRY_TOKEN="crates-io-token..."

for c in rsteg-core rsteg-bmp rsteg-wav rsteg-png rsteg-crypto-aead rsteg-cli; do
  cargo publish -p "$c"
  # crates.io index propagation takes a few seconds — wait before publishing
  # the next crate so its dep resolves.
  sleep 20
done
```

If a downstream crate fails with "no matching package named …", wait longer for index propagation and re-run `cargo publish -p <crate>`.

### 6. Verify

```sh
# crates.io
cargo install rsteg-cli
rsteg --version

# GitHub Release prebuilt binary (replace TARGET with your triple)
curl -LO https://github.com/OpenHackersClub/rsteg/releases/download/vX.Y.Z/rsteg-cli-vX.Y.Z-TARGET.tar.gz
tar xzf rsteg-cli-vX.Y.Z-TARGET.tar.gz
./rsteg-cli-vX.Y.Z-TARGET/rsteg --version
```

## What is automated vs. manual

| Step                            | Automated by                                 |
| ------------------------------- | -------------------------------------------- |
| Manifest validation (per PR)    | `publish-check` job in `ci.yml`              |
| Build + test (per PR / push)    | `test` matrix job in `ci.yml`                |
| Prebuilt binaries on tag push   | `release.yml`                                |
| GitHub Release creation         | `release.yml`                                |
| crates.io publish               | **Manual** — step 5 above                    |

crates.io publishing is deliberately manual: it's irreversible (you can only yank, not unpublish), and chaining it behind a tag push makes bad releases harder to catch.

## Adjusting the release workflow

`.github/workflows/release.yml` is hand-rolled — edit it directly. To add or remove a target, add/remove an entry in the `matrix.include` list. To change the archive contents, edit the "Package (unix)" / "Package (windows)" steps.
