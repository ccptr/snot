#!/usr/bin/env bash
# Points the AUR packages at a release and refreshes their checksums and
# .SRCINFO files, ready to commit here and push to the AUR:
#
#     packaging/aur/update.sh 1.2.3                  # all of them
#     packaging/aur/update.sh 1.2.3 snot-notes-bin   # just one
#
# The version is the one in Cargo.toml, without the tag's v. The release must
# be published already, as the checksums are of its source and Linux tarball;
# a source file already next to a PKGBUILD is summed instead of downloaded.
# snot-notes-git takes no version and only has its .SRCINFO refreshed. Needs
# makepkg and updpkgsums (pacman-contrib).
set -euo pipefail

version=${1:?usage: update.sh VERSION [PACKAGE...]}
shift
cd "$(dirname "$0")"
[ $# -gt 0 ] || set -- snot-notes snot-notes-bin snot-notes-git

for pkg; do
  (
    cd "$pkg"
    if [ "$pkg" != snot-notes-git ]; then
      sed -i -e "s/^_version=.*/_version=$version/" -e 's/^pkgrel=.*/pkgrel=1/' PKGBUILD
      updpkgsums
    fi
    makepkg --printsrcinfo > .SRCINFO
  )
done
