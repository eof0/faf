# Release process

## 1. Tag and push

Update the version in `Cargo.toml` and `Cargo.lock` (`cargo build --release`), commit, then:

```sh
git tag v3.0.0
git push origin v3.0.0
```

Tagging is the only step required to build and publish binaries.
Updating AUR and Homebrew formulas is still manual.
---

## 2. What the GitHub Action does

On every `v*` tag push, `.github/workflows/release.yml`:

1. Builds release binaries for three targets in parallel:
   - `x86_64-unknown-linux-gnu` (native, Ubuntu runner)
   - `aarch64-unknown-linux-gnu` (via `cross`, Ubuntu runner)
   - `aarch64-apple-darwin` (native, macOS 14 runner)
2. Packages each Linux/macOS binary as `faf-<platform>-<tag>.tar.gz` (e.g. `faf-linux-x86_64-v3.0.0.tar.gz`)
3. Creates a GitHub Release for the tag and uploads all archives

The release is available at:
`https://github.com/eof0/faf/releases/tag/v3.0.0`

Each tarball contains one `faf` binary.

---

## 3. Publish to AUR (`faf-bin`)

Full checklist: [`packaging/aur/README.md`](packaging/aur/README.md)

After the GitHub release assets are live:

```sh
cd packaging/aur
./update-checksums.sh   # requires makepkg / updpkgsums on Arch
makepkg -si             # optional local smoke test
```

Push `PKGBUILD` and `.SRCINFO` to `aur@aur.archlinux.org:faf-bin.git`:

```sh
git clone ssh://aur@aur.archlinux.org/faf-bin.git
cd faf-bin
cp /path/to/faf/packaging/aur/{PKGBUILD,.SRCINFO,LICENSE,REUSE.toml} .
git add PKGBUILD .SRCINFO LICENSE REUSE.toml
git commit -m "faf-bin 3.0.0"
git push
```

Linux release URLs used by the PKGBUILD:

| Arch | URL path |
|------|----------|
| x86_64 | `.../faf-linux-x86_64-v3.0.0.tar.gz` |
| aarch64 | `.../faf-linux-arm64-v3.0.0.tar.gz` |

---

## 4. Update Homebrew formula sha256

```sh
curl -sL https://github.com/eof0/faf/releases/download/v3.0.0/faf-macos-arm64-v3.0.0.tar.gz | sha256sum
```

Replace the hash or placeholder values in `faf.rb` in [eof0/homebrew-tap](https://github.com/eof0/homebrew-tap) and bump `version`.

---

## 5. Publish to crates.io

The crate is published as `fafind` because `faf` is taken on crates.io.
It installs only the `faf` binary.

```sh
cargo publish
```
