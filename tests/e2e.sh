#!/bin/sh
# End-to-end: publish a master client, patch player clients from the site (file:// mirror).
# Needs a built `otp` and `otp-testkit` (both from this workspace: cargo build -p otp-cli -p otp-testkit).
# Usage: tests/e2e.sh [path/to/otp]   (otp-testkit is taken from the same folder)
set -eu
OTP=${1:-target/debug/otp}
OTP=$(cd "$(dirname "$OTP")" && pwd)/$(basename "$OTP")
TK=$(dirname "$OTP")/otp-testkit
[ -x "$TK" ] || { echo "missing $TK (cargo build -p otp-testkit)"; exit 1; }
W=$(mktemp -d)
trap '[ -n "${KEEP:-}" ] || rm -rf "$W"' EXIT
fail() { echo "FAIL: $*"; exit 1; }
ok() { echo "ok: $*"; }

for ENC in plain encrypted; do
  D="$W/$ENC"; M="$D/master"; P="$D/player"; S="$D/site"
  mkdir -p "$M/src/data/texture" "$P/oldfolder"
  PW=""; RP=""; CP=""; PUBPW=""; PATCHOPT=""
  if [ "$ENC" = encrypted ]; then
    PW="--password secret123 --encrypt"; RP="--password secret123"
    CP="--grf-password data.grf=secret123"; PUBPW="--grf-password secret123"
  fi
  i=1; while [ $i -le 40 ]; do echo "tex $i" > "$M/src/data/texture/t$i.txt"; i=$((i+1)); done
  echo "v1" > "$M/odin.cfg"; echo "dark" > "$M/odin-dark.grf"
  # shellcheck disable=SC2086
  "$TK" build $PW -o "$M/data.grf" "$M/src" >/dev/null
  "$TK" build -o "$M/odin.grf" "$M/src" >/dev/null

  "$OTP" keygen --out "$D/keys" >/dev/null
  cat > "$D/project.json" <<EOF
{
  "server": "TestRO", "client": "$M", "site": "$S", "base_urls": ["file://$S"],
  "thor": ["data.grf"], "sync": ["odin.grf", "odin.cfg", "odin-dark.grf"],
  "remove": ["old.dll", "oldfolder/*"], "signing_seed": "$D/keys/signing.seed", "full": true,
  "notice": "hello",
  "game": {"exe": "game.exe", "args": "-1rag1"},
  "data_ini": {"order": ["odin.grf", "@pack:comp", "data.grf"],
               "packs": [{"id": "comp", "file": "odin-dark.grf", "title": "Competitive", "default": true}]}
}
EOF
  pub() { "$OTP" publish --project "$D/project.json" $PUBPW; }
  pat() { "$OTP" patch --url "file://$S" --client "$P" --pub "$D/keys/patcher.pub" $CP; }

  "$OTP" publish --project "$D/project.json" --dry-run $PUBPW > "$D/dry.log" || { cat "$D/dry.log"; fail "$ENC dry run"; }
  [ ! -e "$S/patchlist.json" ] || fail "$ENC dry run wrote a patch list"
  ok "$ENC: publish --dry-run writes nothing"

  pub > "$D/pub1.log" || { cat "$D/pub1.log"; fail "$ENC publish 1"; }
  grep -q "baseline" "$D/pub1.log" || fail "$ENC no baseline"

  # a fresh client package = the baseline copy, plus things the server wants gone and a player's own GRF
  cp "$M/data.grf" "$P/data.grf"; echo old > "$P/old.dll"; echo x > "$P/oldfolder/a.txt"; echo keep > "$P/keep.txt"
  printf '[Data]\r\n0=data.grf\r\n1=myskin.grf\r\n' > "$P/DATA.INI"
  pat > "$D/p1.log" || { cat "$D/p1.log"; fail "$ENC patch 1"; }
  cmp "$M/odin.cfg" "$P/odin.cfg" || fail "$ENC synced file differs"
  grep -q "0 patch" "$D/p1.log" || fail "$ENC patch 1 should apply no thor patch"
  [ ! -e "$P/old.dll" ] && [ ! -e "$P/oldfolder" ] && [ -f "$P/keep.txt" ] || fail "$ENC removal"
  ok "$ENC: fresh client gets whole files, removals work"
  tr -d '\r' < "$P/DATA.INI" | tr '\n' ' ' | grep -q "0=odin.grf 1=odin-dark.grf 2=data.grf 3=myskin.grf" || { cat "$P/DATA.INI"; fail "$ENC DATA.INI"; }
  ok "$ENC: DATA.INI follows the server order, the player's own GRF stays"

  "$OTP" pack --url "file://$S" --client "$P" --id comp --off > /dev/null || fail "$ENC pack off"
  tr -d '\r' < "$P/DATA.INI" | tr '\n' ' ' | grep -q "0=odin.grf 1=data.grf 2=myskin.grf" || { cat "$P/DATA.INI"; fail "$ENC pack off ini"; }
  pat > /dev/null; tr -d '\r' < "$P/DATA.INI" | grep -q "odin-dark" && fail "$ENC pack came back after patching"
  "$OTP" pack --url "file://$S" --client "$P" --id comp --on > /dev/null || fail "$ENC pack on"
  grep -q "odin-dark.grf" "$P/DATA.INI" || fail "$ENC pack on ini"
  ok "$ENC: optional pack switch survives patching"

  # master changes: one changed, one new, one removed file in the GRF
  echo "tex changed" > "$M/src/data/texture/t1.txt"
  echo "brand new" > "$M/src/data/texture/new.txt"
  rm "$M/src/data/texture/t2.txt"
  echo "v2" > "$M/odin.cfg"
  rm "$M/data.grf"
  # shellcheck disable=SC2086
  "$TK" build $PW -o "$M/data.grf" "$M/src" >/dev/null
  pub > "$D/pub2.log" || { cat "$D/pub2.log"; fail "$ENC publish 2"; }
  grep -q "0001-data.thor: 1 new, 1 changed, 1 removed" "$D/pub2.log" || { cat "$D/pub2.log"; fail "$ENC patch content"; }
  pub > "$D/pub3.log"; grep -q "no changes" "$D/pub3.log" || fail "$ENC idempotent publish"
  ok "$ENC: publish writes only the diff, second publish is a no-op"

  pat > "$D/p2.log" || { cat "$D/p2.log"; fail "$ENC patch 2"; }
  grep -q "1 patch" "$D/p2.log" || fail "$ENC patch 2 count"
  # shellcheck disable=SC2086
  "$TK" extract $RP "$P/data.grf" "$D/xp" >/dev/null 2>&1 || fail "$ENC extract player"
  # shellcheck disable=SC2086
  "$TK" extract $RP "$M/data.grf" "$D/xm" >/dev/null 2>&1 || fail "$ENC extract master"
  diff -r "$D/xp" "$D/xm" >/dev/null || fail "$ENC patched GRF differs from master"
  [ ! -e "$P/data.grf.bak" ] || fail "$ENC old GRF was kept although --keep-backup was not given"
  ok "$ENC: patched GRF content == master (changed, new and removed entries), the old GRF is replaced completely (no .bak)"

  pat > "$D/p3.log"
  grep -q "0 file(s), 0 patch(es), 0 repaired, 0 removed" "$D/p3.log" || { cat "$D/p3.log"; fail "$ENC up to date run"; }
  ok "$ENC: second run is a no-op"

  cp "$S/patchlist.json" "$D/list.orig"
  sed 's/TestRO/Evil/' "$D/list.orig" > "$S/patchlist.json"
  if pat >/dev/null 2>&1; then fail "$ENC tampered list accepted"; fi
  cp "$D/list.orig" "$S/patchlist.json"
  ok "$ENC: tampered patch list rejected"

  # a download that arrives damaged is refused and the client stays as it was
  cp "$S/files/data.grf" "$D/full.orig"
  echo "garbage" >> "$S/files/data.grf"
  echo "junk" >> "$P/data.grf"; cp "$P/data.grf" "$D/damaged.grf"
  pat > "$D/p4.log" 2>&1 && fail "$ENC damaged download accepted"
  cmp "$P/data.grf" "$D/damaged.grf" || fail "$ENC client changed by a refused download"
  cp "$D/full.orig" "$S/files/data.grf"
  ok "$ENC: damaged download refused, client untouched"

  pat > "$D/p5.log" || { cat "$D/p5.log"; fail "$ENC repair"; }
  grep -q "1 repaired" "$D/p5.log" || fail "$ENC repair count"
  cmp "$M/data.grf" "$P/data.grf" || fail "$ENC repaired GRF != master"
  ok "$ENC: modified GRF restored from the full copy"
done

# ---- classic Thor Patcher server (plist.txt + .thor), merge and loose patches --------------------------------------
D="$W/legacy"; S="$D/site"; P="$D/player"; mkdir -p "$S" "$D/m/data/texture" "$D/l" "$P"
echo a > "$D/m/data/texture/a.txt"; echo b > "$D/m/data/texture/b.txt"
"$TK" build -o "$P/data.grf" "$D/m" >/dev/null
echo c > "$D/m/data/texture/c.txt"; rm "$D/m/data/texture/a.txt"
"$TK" thor --merge-into-grf --target data.grf --remove 'data\texture\a.txt' -o "$S/001 merge.thor" "$D/m" >/dev/null
echo loose > "$D/l/readme.txt"; mkdir -p "$D/l/sub"; echo s > "$D/l/sub/s.txt"
"$TK" thor -o "$S/002 loose.thor" "$D/l" >/dev/null
printf '// classic list\r\n1 001 merge.thor\r\n2 002 loose.thor\r\n' > "$S/plist.txt"
"$OTP" patch --url "file://$S" --client "$P" > "$D/p1.log" || { cat "$D/p1.log"; fail "legacy patch"; }
"$TK" extract "$P/data.grf" "$D/x" >/dev/null 2>&1 || fail "legacy extract"
[ -f "$D/x/data/texture/c.txt" ] && [ -f "$D/x/data/texture/b.txt" ] && [ ! -e "$D/x/data/texture/a.txt" ] || fail "legacy merge result"
[ "$(cat "$P/readme.txt")" = loose ] && [ "$(cat "$P/sub/s.txt")" = s ] || fail "legacy loose patch"
"$OTP" patch --url "file://$S" --client "$P" | grep -q "0 patch" || fail "legacy second run"
ok "classic plist.txt server: merge patch with a delete, loose patch, second run no-op"

# ---- cloud style: every file has its own full link, no mirror ------------------------------------------------------
D="$W/cloud"; M="$D/m"; S="$D/site"; P="$D/p"; U="$D/uploads"; mkdir -p "$M" "$P" "$U"
echo cloud1 > "$M/a.cfg"
cat > "$D/project.json" <<EOF
{"server":"CloudRO","client":"$M","site":"$S","base_urls":[],"sync":["a.cfg"],
 "url_map":{"files/a.cfg":"file://$U/share-a"}}
EOF
"$OTP" publish --project "$D/project.json" > "$D/pub.log" || { cat "$D/pub.log"; fail "cloud publish"; }
cp "$S/files/a.cfg" "$U/share-a"
"$OTP" patch --url "file://$S" --client "$P" > /dev/null || fail "cloud patch"
cmp "$M/a.cfg" "$P/a.cfg" || fail "cloud file differs"
ok "cloud style: full links per file, no mirror"

# ---- private host: the site is served only with a token header (GitHub/GitLab private repositories work the same way) --
D="$W/auth"; M="$D/m"; S="$D/site"; P="$D/p"; mkdir -p "$M" "$P" "$S"
PORT=$((20000 + $$ % 20000))
echo secret-cfg > "$M/a.cfg"
cat > "$D/project.json" <<EOF
{"server":"PrivRO","client":"$M","site":"$S","base_urls":["http://127.0.0.1:$PORT"],"sync":["a.cfg"]}
EOF
"$OTP" publish --project "$D/project.json" > /dev/null || fail "auth publish"
cat > "$D/srv.py" <<'EOF'
import http.server, sys, os
os.chdir(sys.argv[2])
class H(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        if self.headers.get("Authorization") != "Bearer s3cret":
            self.send_response(401); self.end_headers(); return
        super().do_GET()
    def log_message(self, *a): pass
http.server.HTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
EOF
python3 "$D/srv.py" "$PORT" "$S" & SRV=$!
sleep 1
if "$OTP" patch --url "http://127.0.0.1:$PORT" --client "$P" > "$D/p0.log" 2>&1; then kill $SRV; fail "private host answered without a token"; fi
grep -q "access denied" "$D/p0.log" || { kill $SRV; cat "$D/p0.log"; fail "no clear message for a missing token"; }
"$OTP" patch --url "http://127.0.0.1:$PORT" --client "$P" --header "Authorization: Bearer s3cret" > "$D/p1.log" 2>&1 || { kill $SRV; cat "$D/p1.log"; fail "patch with token"; }
kill $SRV
cmp "$M/a.cfg" "$P/a.cfg" || fail "private host file differs"
ok "private host: refused without the token with a clear message, works with it"

# ---- git hosting: publish commits and pushes the site into a (bare) repository ---------------------------------------
D="$W/git"; M="$D/m"; S="$D/site"; mkdir -p "$M" "$D"
git init -q --bare "$D/remote.git" 2>/dev/null
git clone -q "$D/remote.git" "$S" 2>/dev/null
( cd "$S" && git config user.email t@example.org && git config user.name t && git checkout -q -b main 2>/dev/null; true )
echo g1 > "$M/a.cfg"
cat > "$D/project.json" <<EOF
{"server":"GitRO","client":"$M","site":"$S","base_urls":["gh:me/patches@main"],"sync":["a.cfg"],"git_push":true}
EOF
"$OTP" publish --project "$D/project.json" > "$D/pub.log" 2>&1 || { cat "$D/pub.log"; fail "git publish"; }
( cd "$S" && git push -q -u origin main 2>/dev/null; true )
echo g2 > "$M/a.cfg"; "$OTP" publish --project "$D/project.json" > "$D/pub2.log" 2>&1 || { cat "$D/pub2.log"; fail "git publish 2"; }
[ "$(git --git-dir="$D/remote.git" log --oneline main 2>/dev/null | wc -l)" -ge 2 ] || { cat "$D/pub2.log"; fail "git commits missing in the remote"; }
git --git-dir="$D/remote.git" show main:patchlist.json | grep -q GitRO || fail "patchlist not in the remote"
git --git-dir="$D/remote.git" ls-tree -r main --name-only | grep -q "^state/" && fail "state/ was pushed"
ok "git hosting: publish commits and pushes the site, state/ stays local"

# ---- several sources: the newest valid patch list wins, a stale or broken source changes nothing --------------------------
D="$W/mirror"; M="$D/m"; A="$D/siteA"; B="$D/siteB"; P="$D/p"; mkdir -p "$M" "$P"
echo m1 > "$M/a.cfg"
cat > "$D/project.json" <<EOF
{"server":"MirrorRO","client":"$M","site":"$A","base_urls":["file://$A","file://$B"],"sync":["a.cfg"]}
EOF
serial_of() { sed -n 's/.*"serial": *\([0-9]*\).*/\1/p' "$1/patchlist.json" | head -1; }
"$OTP" publish --project "$D/project.json" > /dev/null || fail "mirror publish 1"
[ "$(serial_of "$A")" = 1 ] || fail "first publish should have serial 1, has '$(serial_of "$A")'"
"$OTP" publish --project "$D/project.json" > /dev/null || fail "mirror publish 1b"
[ "$(serial_of "$A")" = 1 ] || fail "a publish without changes must keep the serial, has '$(serial_of "$A")'"
ok "serial: 1 for the first publish, unchanged by a publish without changes"
cp -R "$A" "$B"                                          # mirror B is in sync (serial 1)
echo m2 > "$M/a.cfg"
"$OTP" publish --project "$D/project.json" > /dev/null || fail "mirror publish 2"
[ "$(serial_of "$A")" = 2 ] || fail "second publish should have serial 2"
# A is current (serial 2), B is behind (serial 1)
"$OTP" patch --url "file://$B" --fallback-url "file://$A" --client "$P" > "$D/p1.log" 2>&1 || { cat "$D/p1.log"; fail "patch with a stale first source"; }
cmp "$M/a.cfg" "$P/a.cfg" || fail "the client did not get the master version although a newer source existed"
grep -q "list serial 2" "$D/p1.log" || { cat "$D/p1.log"; fail "the newest list (serial 2) was not used"; }
ok "stale first source, current fallback: the newest list wins, the client gets the master version"
# only the stale source reachable now: the client already applied serial 2, so serial 1 must be refused
echo "tampered by a stale mirror" > "$B/files/a.cfg"
if "$OTP" patch --url "file://$B" --client "$P" > "$D/p2.log" 2>&1; then fail "a list older than the applied one was accepted"; fi
grep -q "already applied serial 2" "$D/p2.log" || { cat "$D/p2.log"; fail "no clear message for a stale source"; }
cmp "$M/a.cfg" "$P/a.cfg" || fail "a stale source changed the client"
ok "rollback protection: a source that is behind is refused, the client stays as it is"
# primary gone completely
echo m3 > "$M/a.cfg"; "$OTP" publish --project "$D/project.json" > /dev/null || fail "mirror publish 3"
cp -R "$A/." "$B/"
P2="$D/p2"; mkdir -p "$P2"
"$OTP" patch --url "file://$D/does-not-exist" --fallback-url "file://$B" --client "$P2" > "$D/p3.log" 2>&1 || { cat "$D/p3.log"; fail "patch with the primary source gone"; }
cmp "$M/a.cfg" "$P2/a.cfg" || fail "fallback source did not deliver the master version"
ok "primary source gone: the fallback source serves everything"
# a source with a damaged file: SHA-256 rejects it, the next source delivers
echo m4 > "$M/a.cfg"; "$OTP" publish --project "$D/project.json" > /dev/null || fail "mirror publish 4"
cp -R "$A/." "$B/"
echo "damaged" > "$A/files/a.cfg"                          # base_urls lists A first, then B
P3="$D/p3"; mkdir -p "$P3"
"$OTP" patch --url "file://$A" --client "$P3" > "$D/p4.log" 2>&1 || { cat "$D/p4.log"; fail "patch with a damaged first source"; }
cmp "$M/a.cfg" "$P3/a.cfg" || fail "a damaged file was accepted"
grep -q "trying the next source" "$D/p4.log" || { cat "$D/p4.log"; fail "the fallback was not announced"; }
ok "damaged file on one source: rejected by SHA-256, the next source delivers the master version"

# ---- parallel downloads and the bandwidth limit -------------------------------------------------------------------------
D="$W/parallel"; M="$D/m"; S="$D/site"; P="$D/p"; P1="$D/p1"; mkdir -p "$M" "$P" "$P1"
i=1; while [ $i -le 12 ]; do head -c 65536 /dev/urandom > "$M/f$i.bin"; i=$((i+1)); done
SYNC=$(i=1; while [ $i -le 12 ]; do printf '"f%s.bin",' $i; i=$((i+1)); done | sed 's/,$//')
cat > "$D/project.json" <<EOF
{"server":"ParRO","client":"$M","site":"$S","base_urls":["file://$S"],"sync":[$SYNC]}
EOF
"$OTP" publish --project "$D/project.json" > /dev/null || fail "parallel publish"
"$OTP" patch --url "file://$S" --client "$P" --parallel 6 > "$D/p.log" 2>&1 || { cat "$D/p.log"; fail "parallel patch"; }
i=1; while [ $i -le 12 ]; do cmp "$M/f$i.bin" "$P/f$i.bin" || fail "parallel: f$i.bin differs"; i=$((i+1)); done
"$OTP" patch --url "file://$S" --client "$P1" --parallel 1 > /dev/null 2>&1 || fail "serial patch"
ok "parallel downloads: 12 files with 6 at a time, all equal to the master (also one at a time)"
PORT=$((20000 + ($$ + 7) % 20000))
python3 -m http.server "$PORT" --bind 127.0.0.1 --directory "$S" > /dev/null 2>&1 & SRV=$!
sleep 1
P2="$D/p2"; mkdir -p "$P2"
T0=$(date +%s)
"$OTP" patch --url "http://127.0.0.1:$PORT" --client "$P2" --parallel 4 --max-kbps 256 > "$D/p2.log" 2>&1 || { kill $SRV; cat "$D/p2.log"; fail "rate limited patch"; }
T1=$(date +%s); kill $SRV
cmp "$M/f12.bin" "$P2/f12.bin" || fail "rate limited download differs"
# 768 KiB at 256 KiB/s for all downloads together: at least ~2 s (the first second is the bucket)
[ $((T1 - T0)) -ge 2 ] || fail "bandwidth limit not applied ($((T1 - T0)) s for 768 KiB at 256 KiB/s)"
ok "bandwidth limit: 768 KiB over http at 256 KiB/s with 4 downloads took $((T1 - T0)) s"

# ---- MEGA file links (a local stand-in for MEGA's API and storage, AES-128-CTR as MEGA) ----------------------------------
D="$W/mega"; M="$D/m"; S="$D/site"; P="$D/p"; MS="$D/mega"; mkdir -p "$M" "$P" "$MS"
head -c 300000 /dev/urandom > "$M/big.bin"; echo "mega cfg" > "$M/a.cfg"
KEY="AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8"
KEY2="HxwdHh8QERITFBUWFxgZGhsAAQIDBAUGBwgJCgsMDQ4"
cat > "$D/project.json" <<EOF
{"server":"MegaRO","client":"$M","site":"$S","base_urls":[],"sync":["big.bin","a.cfg"],
 "url_map":{"files/big.bin":"https://mega.nz/file/bigfile#$KEY","files/a.cfg":"mega:cfgfile#$KEY2"}}
EOF
"$OTP" publish --project "$D/project.json" > /dev/null || fail "mega publish"
"$TK" mega-encrypt "https://mega.nz/file/bigfile#$KEY" "$M/big.bin" "$MS/bigfile.enc" || fail "mega encrypt"
"$TK" mega-encrypt "mega:cfgfile#$KEY2" "$M/a.cfg" "$MS/cfgfile.enc" || fail "mega encrypt 2"
cmp -s "$M/big.bin" "$MS/bigfile.enc" && fail "mega stand-in stores plain bytes"
PORT=$((20000 + ($$ + 11) % 20000))
python3 "$(dirname "$0")/mega_mock.py" "$PORT" "$MS" & MSRV=$!
sleep 1
OTP_MEGA_API="http://127.0.0.1:$PORT" "$OTP" patch --url "file://$S" --client "$P" > "$D/p.log" 2>&1 || { kill $MSRV; cat "$D/p.log"; fail "mega patch"; }
cmp "$M/big.bin" "$P/big.bin" || { kill $MSRV; fail "mega: decrypted file differs from the master"; }
cmp "$M/a.cfg" "$P/a.cfg" || { kill $MSRV; fail "mega: short link file differs"; }
ok "MEGA: file links (mega.nz/file/... and mega:...) resolved through the API, decrypted, equal to the master"
# resume: a partial download continues with MEGA's /start-end suffix
echo "changed" >> "$M/big.bin"
"$OTP" publish --project "$D/project.json" > /dev/null || fail "mega publish 2"
"$TK" mega-encrypt "https://mega.nz/file/bigfile#$KEY" "$M/big.bin" "$MS/bigfile.enc"
mkdir -p "$P/_patch/dl"; head -c 100000 "$M/big.bin" > "$P/_patch/dl/big.bin.part"
: > "$MS/hits.log"
OTP_MEGA_API="http://127.0.0.1:$PORT" "$OTP" patch --url "file://$S" --client "$P" > "$D/p2.log" 2>&1 || { kill $MSRV; cat "$D/p2.log"; fail "mega resume"; }
cmp "$M/big.bin" "$P/big.bin" || { kill $MSRV; fail "mega: resumed file differs"; }
grep -q "get bigfile 200008" "$MS/hits.log" || { kill $MSRV; cat "$MS/hits.log"; fail "mega: download did not resume at byte 100000"; }
ok "MEGA: an interrupted download resumes mid-file and decrypts correctly"
# quota: a clear message
sed -e 's#mega:cfgfile#mega:quota#' -e "s#\"site\":\"$S\"#\"site\":\"$D/sq\"#" "$D/project.json" > "$D/pq.json"
echo "q" >> "$M/a.cfg"
"$OTP" publish --project "$D/pq.json" > /dev/null || fail "mega quota publish"
if OTP_MEGA_API="http://127.0.0.1:$PORT" "$OTP" patch --url "file://$D/sq" --client "$D/pq" > "$D/p3.log" 2>&1; then kill $MSRV; fail "mega quota accepted"; fi
kill $MSRV
grep -q "transfer quota" "$D/p3.log" || { cat "$D/p3.log"; fail "no clear message for the MEGA quota"; }
ok "MEGA: an exhausted transfer quota is reported clearly"

# ---- self-update: the signed list offers a newer patcher, which replaces the program ----------------------------------
D="$W/selfupdate"; M="$D/m"; S="$D/site"; mkdir -p "$M" "$D/inst"
echo cfg > "$M/a.cfg"
printf 'new patcher build 0.9.0\n' > "$D/new-patcher"
"$OTP" keygen --out "$D/keys" > /dev/null
cat > "$D/project.json" <<EOF
{"server":"UpdRO","client":"$M","site":"$S","base_urls":["file://$S"],"sync":["a.cfg"],"signing_seed":"$D/keys/signing.seed",
 "patcher":{"version":"0.9.0","files":{"test-platform":"$D/new-patcher"},"notes":"faster downloads"}}
EOF
"$OTP" publish --project "$D/project.json" > "$D/pub.log" || { cat "$D/pub.log"; fail "self-update publish"; }
[ -f "$S/patcher/test-platform/new-patcher" ] || fail "self-update: program not in the site"
printf 'old patcher 0.1.0\n' > "$D/inst/openthor-patcher"
"$OTP" self-update --url "file://$S" --pub "$D/keys/patcher.pub" --exe "$D/inst/openthor-patcher" --current 0.1.0 --platform test-platform > "$D/u.log" 2>&1 || { cat "$D/u.log"; fail "self-update"; }
cmp "$D/new-patcher" "$D/inst/openthor-patcher" || fail "self-update: program not replaced"
grep -q "old patcher" "$D/inst/openthor-patcher.old" || fail "self-update: old program not kept as .old"
"$OTP" self-update --url "file://$S" --pub "$D/keys/patcher.pub" --exe "$D/inst/openthor-patcher" --current 0.9.0 --platform test-platform | grep -q "no newer patcher" || fail "self-update: same version updated again"
printf 'old patcher 0.1.0\n' > "$D/inst/openthor-patcher"
echo "evil" > "$S/patcher/test-platform/new-patcher"
if "$OTP" self-update --url "file://$S" --pub "$D/keys/patcher.pub" --exe "$D/inst/openthor-patcher" --current 0.1.0 --platform test-platform > "$D/u2.log" 2>&1; then fail "self-update accepted a tampered program"; fi
grep -q "old patcher" "$D/inst/openthor-patcher" || fail "self-update: a refused update changed the program"
ok "self-update: newer signed build replaces the program (old kept as .old), same version is left alone, a tampered build is refused"

# ---- MidgardCP: news from the panel's API into the signed list, status URL ----------------------------------------------
D="$W/midgard"; M="$D/m"; S="$D/site"; mkdir -p "$M" "$D/cp/api/v1"
echo cfg > "$M/a.cfg"
cat > "$D/cp/api/v1/news.json" <<'EOF'
{"total":1,"items":[{"id":7,"title":"WoE this Sunday","html":"<p>Castles: <strong>Prontera</strong> &amp; Payon</p><ul><li>19:00</li><li>20:00</li></ul>","realm":null,"pinned":true,"published_at":"2026-11-26T09:00:00Z"}]}
EOF
cat > "$D/project.json" <<EOF
{"server":"CpRO","client":"$M","site":"$S","base_urls":["file://$S"],"sync":["a.cfg"],
 "news_from":"file://$D/cp/api/v1/news.json","status_url":"https://cp.example/api/v1/status","status_realm":"hr",
 "news":[{"date":"2000-01-01","title":"old hand-written news"}]}
EOF
"$OTP" publish --project "$D/project.json" > "$D/pub.log" || { cat "$D/pub.log"; fail "midgard publish"; }
grep -q '"title": "WoE this Sunday"' "$S/patchlist.json" || fail "midgard: panel news not in the list"
grep -q 'Castles: \*\*Prontera\*\* & Payon' "$S/patchlist.json" || { grep -A3 WoE "$S/patchlist.json"; fail "midgard: html not turned into patcher markup"; }
grep -q '"status_realm": "hr"' "$S/patchlist.json" || fail "midgard: status realm missing"
grep -q "old hand-written" "$S/patchlist.json" && fail "midgard: hand-written news not replaced"
mv "$D/cp" "$D/cp-down"
"$OTP" publish --project "$D/project.json" > "$D/pub2.log" || { cat "$D/pub2.log"; fail "midgard publish with the panel down"; }
grep -q "the news of the last publish stay" "$D/pub2.log" || fail "midgard: no warning when the panel is down"
grep -q '"title": "WoE this Sunday"' "$S/patchlist.json" || fail "midgard: news lost while the panel was down"
"$OTP" news --from "file://$D/cp-down/api/v1/news.json" | grep -q "2026-11-26 WoE this Sunday" || fail "otp news preview"
ok "MidgardCP: news from the panel API (html → patcher markup), kept when the panel is down; status URL in the list"

# ---- external files (data.grf stays out of the site) and the client package for new players ----------------------------
D="$W/package"; M="$D/m"; S="$D/site"; mkdir -p "$M/src/data" "$M/csrc/data" "$M/BGM" "$D/store"
i=1; while [ $i -le 20 ]; do echo "base $i" > "$M/src/data/b$i.txt"; i=$((i+1)); done
"$TK" build -o "$M/data.grf" "$M/src" >/dev/null
echo "custom 1" > "$M/csrc/data/c1.txt"; "$TK" build -o "$M/cdata.grf" "$M/csrc" >/dev/null
echo "music" > "$M/BGM/01.mp3"; echo "v1" > "$M/odin.cfg"
mkdir -p "$M/skin/dark theme/img"; echo "btn" > "$M/skin/dark theme/img/btn.bmp"; echo "cfg" > "$M/skin/dark theme/skin.xml"
mkdir -p "$M/_backup_old"; echo "old" > "$M/_backup_old/x.dll"; echo "not managed" > "$M/notes.txt"
cp "$M/data.grf" "$D/store/data.grf"
"$OTP" keygen --out "$D/keys" > /dev/null
cat > "$D/project.json" <<EOF
{"server":"PkgRO","client":"$M","site":"$S","base_urls":["file://$S"],"signing_seed":"$D/keys/signing.seed",
 "thor":["cdata.grf"],"sync":["odin.cfg","BGM/01.mp3","skin/dark theme/*"],
 "external":{"data.grf":["file://$D/gone/data.grf","file://$D/store/data.grf"]}}
EOF
"$OTP" publish --project "$D/project.json" > "$D/pub1.log" || { cat "$D/pub1.log"; fail "package publish 1"; }
[ ! -e "$S/files/data.grf" ] || fail "external: data.grf was copied into the site"
grep -q '"mirrors"' "$S/patchlist.json" || fail "external: second link missing in the list"
echo "custom 2" > "$M/csrc/data/c2.txt"; rm "$M/cdata.grf"; "$TK" build -o "$M/cdata.grf" "$M/csrc" >/dev/null
"$OTP" publish --project "$D/project.json" > "$D/pub2.log" || { cat "$D/pub2.log"; fail "package publish 2"; }
grep -q "0001-cdata.thor" "$D/pub2.log" || { cat "$D/pub2.log"; fail "package: cdata patch missing"; }
ok "external: data.grf only as hash + links in the list, never copied into the site"
echo "fake patcher" > "$D/openthor-patcher.exe"; echo '{"name":"PkgRO","patch_url":"x"}' > "$D/patcher.json"
"$OTP" package --project "$D/project.json" --out "$D/pkg/PkgRO.zip" --patcher "$D/openthor-patcher.exe" --patcher-json "$D/patcher.json" > "$D/pkg.log" || { cat "$D/pkg.log"; fail "package"; }
grep -q "left out (the patcher downloads them): data.grf" "$D/pkg.log" || { cat "$D/pkg.log"; fail "package: data.grf not left out"; }
P="$D/player"; mkdir -p "$P"; (cd "$P" && unzip -q "$D/pkg/PkgRO.zip") || fail "package unzip"
[ -f "$P/openthor.exe" ] && [ ! -e "$P/openthor-patcher.exe" ] && [ -f "$P/patcher.json" ] && [ -f "$P/cdata.grf" ] && [ -f "$P/BGM/01.mp3" ] && [ ! -e "$P/data.grf" ] || fail "package content"
[ -f "$P/skin/dark theme/img/btn.bmp" ] || fail "package: folder sync entry (skin/dark theme/*) not packed"
[ ! -e "$P/_backup_old" ] && [ ! -e "$P/notes.txt" ] || fail "package: unmanaged files packed"
grep -q "notes.txt" "$D/pkg.log" || { cat "$D/pkg.log"; fail "package: no warning about unmanaged files"; }
"$OTP" patch --url "file://$S" --client "$P" --pub "$D/keys/patcher.pub" > "$D/p1.log" 2>&1 || { cat "$D/p1.log"; fail "patch from the package"; }
cmp "$M/data.grf" "$P/data.grf" || fail "external data.grf not downloaded"
grep -q "trying the next link" "$D/p1.log" || { cat "$D/p1.log"; fail "external: second link not used after the first failed"; }
grep -q "1 file(s), 0 patch(es), 0 repaired" "$D/p1.log" || { cat "$D/p1.log"; fail "a fresh package should need only the external file, no patch"; }
"$OTP" patch --url "file://$S" --client "$P" --pub "$D/keys/patcher.pub" | grep -q "0 file(s), 0 patch(es)" || fail "package: second run not a no-op"
echo "btn v2" > "$M/skin/dark theme/img/btn.bmp"
"$OTP" publish --project "$D/project.json" > /dev/null || fail "folder sync publish"
"$OTP" patch --url "file://$S" --client "$P" --pub "$D/keys/patcher.pub" > /dev/null 2>&1 || fail "folder sync patch"
cmp "$M/skin/dark theme/img/btn.bmp" "$P/skin/dark theme/img/btn.bmp" || fail "folder sync: changed file in skin/ not updated"
ok "package: ZIP without data.grf; a new player gets data.grf from the second link and needs no patch (current GRF version)"
echo "v2 unpublished" > "$M/odin.cfg"
if "$OTP" package --project "$D/project.json" --out "$D/pkg/stale.zip" > "$D/pkg2.log" 2>&1; then fail "package of an unpublished master accepted"; fi
grep -q "publish first" "$D/pkg2.log" || { cat "$D/pkg2.log"; fail "package: no clear message"; }
echo "v1" > "$M/odin.cfg"
"$OTP" package --project "$D/project.json" --out "$D/pkg/full.zip" --include-external > /dev/null || fail "package with external"
unzip -l "$D/pkg/full.zip" | grep -q " data.grf$" || fail "package --include-external without data.grf"
ok "package: refused when the master changed since the publish; --include-external packs data.grf too"

# ---- upload (rclone to two places, gh release for the package), patch list last --------------------------------------
if command -v rclone >/dev/null 2>&1; then
D="$W/upload"; M="$D/m"; S="$D/site"; mkdir -p "$M" "$D/fakebin"
echo a > "$M/a.cfg"; head -c 200000 /dev/urandom > "$M/big.bin"
cat > "$D/fakebin/gh" <<'EOF'
#!/bin/sh
echo "$*" >> "$(dirname "$0")/gh.log"
case "$1 $2" in "release view") exit 1;; esac
exit 0
EOF
chmod +x "$D/fakebin/gh"
cat > "$D/project.json" <<EOF
{"server":"UpRO","client":"$M","site":"$S","base_urls":["file://$D/dest1"],"sync":["a.cfg"],
 "external":{"big.bin":["file://$D/ext/big.bin"]},
 "upload":{"site":["$D/dest1","$D/dest2/patch"],"package":["$D/pkgdest","gh-release:me/client@v1"],"external":["$D/ext"],"gh":"$D/fakebin/gh"}}
EOF
"$OTP" publish --project "$D/project.json" > /dev/null || fail "upload publish"
"$OTP" upload --project "$D/project.json" --site --external > "$D/up.log" 2>&1 || { cat "$D/up.log"; fail "upload site"; }
for d in "$D/dest1" "$D/dest2/patch"; do
  cmp "$S/patchlist.json" "$d/patchlist.json" && cmp "$S/files/a.cfg" "$d/files/a.cfg" || fail "upload: $d incomplete"
  [ ! -e "$d/state" ] || fail "upload: state/ was uploaded"
done
cmp "$M/big.bin" "$D/ext/big.bin" || fail "upload: external file missing"
grep -n "patch list last" "$D/up.log" | head -1 | grep -q . || fail "upload: patch list not uploaded last"
P="$D/p"; mkdir -p "$P"
"$OTP" patch --url "file://$D/dest2/patch" --client "$P" > /dev/null 2>&1 || fail "patch from the uploaded site"
cmp "$M/big.bin" "$P/big.bin" && cmp "$M/a.cfg" "$P/a.cfg" || fail "uploaded site does not give the master version"
echo z > "$D/x.zip"
"$OTP" upload --project "$D/project.json" --package "$D/x.zip" > "$D/up2.log" 2>&1 || { cat "$D/up2.log"; fail "upload package"; }
cmp "$D/x.zip" "$D/pkgdest/x.zip" || fail "upload: package not copied"
grep -q "release create v1 --repo me/client" "$D/fakebin/gh.log" && grep -q "release upload v1 --repo me/client --clobber" "$D/fakebin/gh.log" || { cat "$D/fakebin/gh.log"; fail "upload: gh release not used"; }
grep -q "https://github.com/me/client/releases/download/v1/x.zip" "$D/up2.log" || fail "upload: release link not reported"
ok "upload: site to two places (patch list last, state/ kept local, patchable), external file, package to a folder and a GitHub release"
MCP="$(dirname "$OTP")/otp-mcp"
if [ -x "$MCP" ]; then
  rm -rf "$D/ext"
  call() { printf '%s\n' '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"otp_upload\",\"arguments\":$1}}" | "$MCP"; }
  call "{\"project\":\"$D/project.json\",\"external\":true}" | grep -q "confirm: true" || fail "mcp otp_upload without confirm"
  [ ! -e "$D/ext/big.bin" ] || fail "mcp otp_upload wrote without confirm"
  call "{\"project\":\"$D/project.json\",\"external\":true,\"confirm\":true}" > "$D/mcp.log"
  cmp "$M/big.bin" "$D/ext/big.bin" && grep -q '\\"what\\"' "$D/mcp.log" || { cat "$D/mcp.log"; fail "mcp otp_upload"; }
  ok "mcp: otp_upload needs confirm, then uploads the external file"
fi
else echo "skip: rclone not installed (upload)"; fi

# OdinGuard hook: after a publish the master GRFs go into the OdinGuard profile and grf-manifest.json into the site.
# A stand-in `og` records its arguments (OG=/path/to/og runs the real program against a throw-away profile instead).
D="$W/odinguard"; M="$D/master"; S="$D/site"
mkdir -p "$M/src/data"
echo x > "$M/src/data/a.txt"
"$TK" build -o "$M/data.grf" "$M/src" >/dev/null
printf '[Data]\r\n0=data.grf\r\n' > "$M/DATA.INI"
if [ -n "${OG:-}" ]; then
  OGBIN=$OG
  "$OGBIN" new --id testro --operator TestRO "$D/og.profile.toml" > /dev/null
else
  OGBIN="$D/og"
  printf '%s\n' '#!/bin/sh' 'echo "$*" >> "$(dirname "$0")/og.args"' \
    'if [ "$1" = grf-manifest ]; then echo "{\"files\":[{\"name\":\"data.grf\",\"size\":1,\"tail_sha256\":\"00\",\"required\":true}]}" > "$4"; echo "1 GRF version(s) written to $4"; else echo "data.grf scanned"; fi' > "$OGBIN"
  chmod +x "$OGBIN"
  touch "$D/og.profile.toml"
fi
cat > "$D/project.json" <<EOF
{ "server": "TestRO", "client": "$M", "site": "$S", "base_urls": ["file://$S"], "thor": ["data.grf"],
  "odinguard": { "profile": "og.profile.toml", "og": "$OGBIN", "optional": ["odin-dark.grf"] } }
EOF
"$OTP" publish --project "$D/project.json" --dry-run > "$D/dry.log" || { cat "$D/dry.log"; fail "odinguard dry run"; }
grep -q "grf-manifest.json" "$D/dry.log" || { cat "$D/dry.log"; fail "the dry run does not show the OdinGuard step"; }
[ ! -e "$S/grf-manifest.json" ] || fail "the dry run wrote the GRF manifest"
"$OTP" publish --project "$D/project.json" > "$D/pub.log" 2>&1 || { cat "$D/pub.log"; fail "publish with the OdinGuard hook"; }
[ -s "$S/grf-manifest.json" ] || { cat "$D/pub.log"; fail "no grf-manifest.json in the site"; }
grep -q "before players patch" "$D/pub.log" || { cat "$D/pub.log"; fail "no hint to deploy the server config first"; }
if [ -z "${OG:-}" ]; then
  grep -q "^grf-scan --keep-previous --client $M --optional odin-dark.grf $D/og.profile.toml$" "$D/og.args" || { cat "$D/og.args"; fail "og grf-scan arguments"; }
  grep -q "^grf-manifest $D/og.profile.toml -o $S/grf-manifest.json$" "$D/og.args" || { cat "$D/og.args"; fail "og grf-manifest arguments"; }
else
  grep -q '"name": "data.grf"' "$S/grf-manifest.json" || { cat "$S/grf-manifest.json"; fail "real og: data.grf missing in the manifest"; }
fi
ok "OdinGuard hook: og grf-scan (keep previous versions) and grf-manifest.json in the site, not in a dry run"
P="$D/player"; mkdir -p "$P"; cp "$M/data.grf" "$P/data.grf"
"$OTP" patch --url "file://$S" --client "$P" > "$D/patch.log" 2>&1 || { cat "$D/patch.log"; fail "patch with a GRF manifest on the site"; }
if [ -z "${OG:-}" ]; then
  grep -q "data.grf is not a version the server's OdinGuard accepts" "$D/patch.log" || { cat "$D/patch.log"; fail "the patcher did not report a GRF the server does not accept"; }
  ok "patcher: a GRF that is not in the server's GRF manifest is reported after patching"
else
  grep -q "OdinGuard GRF check: 1 GRF(s) accepted" "$D/patch.log" || { cat "$D/patch.log"; fail "the patcher did not confirm the accepted GRF"; }
  ok "patcher: the patched GRF matches the GRF manifest written by the real og"
fi
sed "s#\"og\": \"[^\"]*\"#\"og\": \"$D/missing-og\"#" "$D/project.json" > "$D/p2.json"
if "$OTP" publish --project "$D/p2.json" > "$D/pub2.log" 2>&1; then fail "a missing og program was not reported"; fi
grep -q "could not be started" "$D/pub2.log" || { cat "$D/pub2.log"; fail "no clear message for a missing og"; }
ok "OdinGuard hook: a missing og program fails the publish with a clear message"

echo "ALL OK"
