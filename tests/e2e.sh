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

echo "ALL OK"
