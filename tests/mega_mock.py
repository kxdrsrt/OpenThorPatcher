"""Stand-in for MEGA in tests/e2e.sh: the API (`POST /cs`, `[{"a":"g","p":<id>}]`) answers with a temporary address on this
server, which serves the encrypted bytes with MEGA's `/<start>-<end>` resume suffix. Files: <dir>/<id>.enc.
Special ids: `quota` answers -17 (transfer quota), `gone` answers -9 (link removed)."""
import http.server, json, os, sys

PORT, ROOT = int(sys.argv[1]), sys.argv[2]
HITS = os.path.join(ROOT, "hits.log")


class H(http.server.BaseHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def do_POST(self):
        body = json.loads(self.rfile.read(int(self.headers.get("Content-Length", "0"))) or b"[]")
        fid = body[0].get("p", "")
        with open(HITS, "a") as f:
            f.write(f"api {fid}\n")
        if fid == "quota":
            out = [-17]
        elif fid == "gone" or not os.path.isfile(os.path.join(ROOT, fid + ".enc")):
            out = [-9]
        else:
            size = os.path.getsize(os.path.join(ROOT, fid + ".enc"))
            out = [{"s": size, "at": "x", "g": f"http://127.0.0.1:{PORT}/dl/{fid}"}]
        data = json.dumps(out).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        parts = self.path.split("/")  # /dl/<id>[/<start>-<end>]
        if len(parts) < 3 or parts[1] != "dl":
            self.send_response(404); self.end_headers(); return
        data = open(os.path.join(ROOT, parts[2] + ".enc"), "rb").read()
        if len(parts) > 3:
            a, b = parts[3].split("-")
            data = data[int(a): int(b) + 1]
        with open(HITS, "a") as f:
            f.write(f"get {parts[2]} {len(data)}\n")
        self.send_response(200)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)


class S(http.server.ThreadingHTTPServer):
    def server_bind(self):  # no getfqdn (slow on macOS)
        import socketserver
        socketserver.TCPServer.server_bind(self)
        self.server_name, self.server_port = "127.0.0.1", PORT


S(("127.0.0.1", PORT), H).serve_forever()
