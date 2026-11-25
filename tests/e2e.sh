#!/bin/sh
# End-to-end: publish a master client twice, patch a player client from the site (file:// mirror).
# Needs `rgrf` (RustyGRF CLI) and a built `otp` (cargo build). Usage: tests/e2e.sh [path/to/otp]
set -eu
OTP=${1:-target/debug/otp}
OTP=$(cd "$(dirname "$OTP")" && pwd)/$(basename "$OTP")
W=$(mktemp -d)
trap "[ -n \"${KEEP:-}\" ] || rm -rf $W" EXIT
fail() { echo "FAIL: $*"; exit 1; }
ok() { echo "ok: $*"; }

for ENC in plain encrypted; do
  D="$W/$ENC"; M="$D/master"; P="$D/player"; S="$D/site"
  mkdir -p "$M/src/data/texture" "$P"
  PW=""; RP=""; CP=""; PUBPW=""
  if [ "$ENC" = encrypted ]; then
    PW="--password secret123 --encrypt"; RP="--password secret123"
    CP="--grf-password data.grf=secret123"; PUBPW="--grf-password secret123"
  fi
  i=1; while [ $i -le 40 ]; do echo "tex $i" > "$M/src/data/texture/t$i.txt"; i=$((i+1)); done
  echo "v1" > "$M/odin.cfg"
  # shellcheck disable=SC2086
  rgrf build -q $PW -o "$M/data.grf" "$M/src" >/dev/null
  rgrf build -q -o "$M/odin.grf" "$M/src" >/dev/null

  "$OTP" keygen --out "$D/keys" >/dev/null
  pub() {
    # shellcheck disable=SC2086
    "$OTP" publish --client "$M" --site "$S" --server TestRO --base-url "file://$S" \
      --thor data.grf --sync odin.grf --sync odin.cfg --seed "$D/keys/signing.seed" --full $PUBPW
  }
  pat() {
    # shellcheck disable=SC2086
    "$OTP" patch --url "file://$S" --client "$P" --pub "$D/keys/patcher.pub" $CP
  }
  pub > "$D/pub1.log" || { cat "$D/pub1.log"; fail "$ENC publish 1"; }
  grep -q "baseline" "$D/pub1.log" || fail "$ENC no baseline"

  # a fresh client package = the baseline copy
  cp "$M/data.grf" "$P/data.grf"
  pat > "$D/p1.log" || { cat "$D/p1.log"; fail "$ENC patch 1"; }
  cmp "$M/odin.cfg" "$P/odin.cfg" || fail "$ENC synced file differs"
  grep -q "0 patch" "$D/p1.log" || fail "$ENC patch 1 should apply no thor patch"
  ok "$ENC: fresh client gets whole files only"

  # master changes: one changed, one new, one removed file
  echo "tex changed" > "$M/src/data/texture/t1.txt"
  echo "brand new" > "$M/src/data/texture/new.txt"
  rm "$M/src/data/texture/t2.txt"
  echo "v2" > "$M/odin.cfg"
  rm "$M/data.grf"
  # shellcheck disable=SC2086
  rgrf build -q $PW -o "$M/data.grf" "$M/src" >/dev/null
  pub > "$D/pub2.log" || { cat "$D/pub2.log"; fail "$ENC publish 2"; }
  grep -q "0001-data.thor: 2 new/changed, 1 removed" "$D/pub2.log" || { cat "$D/pub2.log"; fail "$ENC patch content"; }
  pub > "$D/pub3.log"; grep -q "no changes" "$D/pub3.log" || fail "$ENC idempotent publish"
  ok "$ENC: publish writes only the diff, second publish is a no-op"

  pat > "$D/p2.log" || { cat "$D/p2.log"; fail "$ENC patch 2"; }
  grep -q "1 patch" "$D/p2.log" || fail "$ENC patch 2 count"
  # shellcheck disable=SC2086
  rgrf extract -q $RP "$P/data.grf" "$D/xp" >/dev/null 2>&1 || fail "$ENC extract player"
  # shellcheck disable=SC2086
  rgrf extract -q $RP "$M/data.grf" "$D/xm" >/dev/null 2>&1 || fail "$ENC extract master"
  diff -r "$D/xp" "$D/xm" >/dev/null || fail "$ENC patched GRF differs from master"
  [ -f "$P/data.grf.bak" ] || fail "$ENC no backup"
  ok "$ENC: patched GRF content == master, backup kept"

  pat > "$D/p3.log"
  grep -q "0 file(s), 0 patch(es), 0 repaired" "$D/p3.log" || { cat "$D/p3.log"; fail "$ENC up to date run"; }
  ok "$ENC: second run is a no-op"

  cp "$S/patchlist.json" "$D/list.orig"
  sed 's/TestRO/Evil/' "$D/list.orig" > "$S/patchlist.json"
  if pat >/dev/null 2>&1; then fail "$ENC tampered list accepted"; fi
  cp "$D/list.orig" "$S/patchlist.json"
  ok "$ENC: tampered patch list rejected"

  echo junk >> "$P/data.grf"
  pat > "$D/p4.log" || { cat "$D/p4.log"; fail "$ENC repair"; }
  grep -q "1 repaired" "$D/p4.log" || fail "$ENC repair count"
  cmp "$M/data.grf" "$P/data.grf" || fail "$ENC repaired GRF != master"
  ok "$ENC: damaged GRF restored from the full copy"
done
echo "ALL OK"
