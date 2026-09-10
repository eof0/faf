# AUR: faf-bin

Pre-built binaries from [GitHub Releases](https://github.com/eof0/faf/releases). Installs:

- `/usr/bin/faf` - release binary

`faf-bin` replaces the old `fafind-bin` package.

## Maintainer checklist (v3.0.0+)

### 1. Publish upstream release

From the repo root:

```sh
git tag v3.0.0
git push origin v3.0.0
```

Wait for [release.yml](../../.github/workflows/release.yml) to finish. Confirm these assets exist:

| Architecture | Asset filename |
|--------------|----------------|
| x86_64 Linux | `faf-linux-x86_64-v3.0.0.tar.gz` |
| aarch64 Linux | `faf-linux-arm64-v3.0.0.tar.gz` |

Each tarball contains a single `faf` executable (stripped).

### 2. Refresh checksums

On Arch Linux (or an Arch container) in this directory:

```sh
cd packaging/aur
./update-checksums.sh
```

This runs `updpkgsums` and regenerates `.SRCINFO`. Commit both files.

If `updpkgsums` fails because the tag is not up yet, fix `pkgver` / URLs in `PKGBUILD` first, then retry.

### 3. Test the package

```sh
makepkg -si
faf --version
faf main /usr/share/doc  # quick smoke test
```

Or in a clean chroot:

```sh
extra-x86_64-build
```

### 4. Push to AUR

`faf-bin` is a new AUR package. Create it by pushing to its (empty) AUR repo:

```sh
git clone ssh://aur@aur.archlinux.org/faf-bin.git
cd faf-bin
cp /path/to/faf/packaging/aur/{PKGBUILD,.SRCINFO,LICENSE,REUSE.toml} .
git add PKGBUILD .SRCINFO LICENSE REUSE.toml
git commit -m "faf-bin 3.0.0"
git push
```

Then file a merge request on the `fafind-bin` AUR page asking to merge it into `faf-bin`.

### 5. Version bumps later

1. Bump `pkgver` / `pkgrel` in `PKGBUILD`
2. Run `./update-checksums.sh` after the matching GitHub release exists
3. Test with `makepkg -si`
4. Push to `aur@aur.archlinux.org:faf-bin.git`

## Files

| File | Purpose |
|------|---------|
| `PKGBUILD` | Package recipe |
| `.SRCINFO` | AUR metadata (must match `PKGBUILD`; regenerate, do not hand-edit for releases) |
| `update-checksums.sh` | `updpkgsums` + `makepkg --printsrcinfo` |
| `LICENSE` | 0BSD license for these packaging files (upstream MIT license is fetched as `faf-LICENSE` in `PKGBUILD`) |
