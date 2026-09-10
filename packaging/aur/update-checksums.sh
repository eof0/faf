#!/usr/bin/env bash
# Refresh sha256sums and .SRCINFO after GitHub release assets are published.
set -euo pipefail

cd "$(dirname "$0")"

if ! command -v makepkg >/dev/null 2>&1; then
  echo "error: makepkg not found (run on Arch Linux or inside an Arch container)" >&2
  exit 1
fi

pkgver=$(grep '^pkgver=' PKGBUILD | sed 's/pkgver=//')
echo "Updating checksums for faf-bin ${pkgver}..."

updpkgsums
makepkg --printsrcinfo > .SRCINFO

echo "Done. Review PKGBUILD and .SRCINFO, then commit and push to AUR."
