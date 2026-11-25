#!/bin/sh
# Refreshes vendor/rgrf-core from a RustyGRF checkout, so that this project builds and runs without RustyGRF:
# no git dependency, no `rgrf` program, builds offline.
#
#   tools/vendor-rgrf-core.sh /path/to/RustyGRF-checkout
#
# Copied: crates/rgrf-core/src (without clientcrypt.rs, which embeds GRF Editor's cps.dll and is not needed here)
# and the licences. Not copied: tests and fixtures (third-party test data) and the assets folder.
set -eu
SRC="${1:?usage: $0 <RustyGRF checkout>}"
HERE="$(cd "$(dirname "$0")/.." && pwd)"
DST="$HERE/vendor/rgrf-core"
[ -d "$SRC/crates/rgrf-core/src" ] || { echo "not a RustyGRF checkout: $SRC" >&2; exit 1; }
REV="$(git -C "$SRC" rev-parse HEAD)"
VER="$(sed -n 's/^version = "\(.*\)"/\1/p' "$SRC/Cargo.toml" | head -1)"

rm -rf "$DST"
mkdir -p "$DST"
cp -R "$SRC/crates/rgrf-core/src" "$DST/src"
rm -f "$DST/src/clientcrypt.rs"
grep -v '^pub mod clientcrypt;' "$DST/src/lib.rs" > "$DST/src/lib.rs.new" && mv "$DST/src/lib.rs.new" "$DST/src/lib.rs"
cp "$SRC/LICENSE-APACHE" "$SRC/LICENSE-MIT" "$DST/"

# Dependencies exactly as upstream (same versions), without the workspace inheritance.
awk '/^\[dependencies\]/{p=1} p' "$SRC/crates/rgrf-core/Cargo.toml" > "$DST/deps.tmp"
{
  echo '[package]'
  echo 'name = "rgrf-core"'
  echo 'description = "RustyGRF core library (vendored copy, see VENDORED.md)"'
  echo "version = \"$VER\""
  echo 'edition = "2024"'
  echo 'license = "MIT OR Apache-2.0"'
  echo 'publish = false'
  echo
  echo '# the upstream unit tests need test data that is not copied here'
  echo '[lib]'
  echo 'test = false'
  echo
  cat "$DST/deps.tmp"
} > "$DST/Cargo.toml"
rm "$DST/deps.tmp"

cat > "$DST/VENDORED.md" <<EOF
# rgrf-core (vendored)

A copy of the \`rgrf-core\` crate of [RustyGRF](https://github.com/kxdrsrt/RustyGRF) (MIT OR Apache-2.0, same author),
revision \`$REV\`, version $VER. It is here so that this project does not depend on RustyGRF: no git dependency, no
\`rgrf\` program, no network access to build.

- Changed: \`Cargo.toml\` (no workspace inheritance, lib tests off), \`clientcrypt\` left out (it embeds a third-party DLL).
- Not changed: everything in \`src/\`. Do not edit it here; fix it in RustyGRF and run \`tools/vendor-rgrf-core.sh\` again.
- The upstream tests (and the test data they need) stay in RustyGRF.
EOF
echo "vendored rgrf-core $VER at $REV into $DST"
