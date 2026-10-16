"""Reference-browser oracle for HTML parser golden tests.

Serves tests/html/*.html to a browser page that parses each fixture with the
browser's own DOMParser, serializes the tree in html5lib-tests format, and
POSTs the result back. Results are written to tests/html/<name>.tree, and
document modes plus the browser's user agent go to tests/html/oracle.txt.

Usage:
    python scripts/html-oracle/server.py [port]
Then open http://127.0.0.1:<port>/ in the reference browser.

Binds to 127.0.0.1 only. Accepts results only for fixture names that exist.
"""

import json
import pathlib
import re
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ROOT = pathlib.Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "tests" / "html"
NAME_RE = re.compile(r"^[a-z0-9-]+$")

PAGE = """<!DOCTYPE html><meta charset=utf-8><title>RustLite HTML oracle</title>
<pre id=log>running…</pre>
<script>
function dump(doc) {
  const out = [];
  (function rec(node, depth) {
    for (const c of node.childNodes) {
      const ind = '  '.repeat(depth);
      if (c.nodeType === 10) {
        const ids = (c.publicId || c.systemId) ? ` "${c.publicId}" "${c.systemId}"` : '';
        out.push(`| ${ind}<!DOCTYPE ${c.name}${ids}>`);
      } else if (c.nodeType === 1) {
        out.push(`| ${ind}<${c.localName}>`);
        const attrs = [...c.attributes].map(a => [a.name, a.value])
          .sort((a, b) => a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0);
        for (const [n, v] of attrs) out.push(`| ${ind}  ${n}="${v}"`);
        rec(c.localName === 'template' ? c.content : c, depth + 1);
      } else if (c.nodeType === 3) {
        out.push(`| ${ind}"${c.data}"`);
      } else if (c.nodeType === 8) {
        out.push(`| ${ind}<!-- ${c.data} -->`);
      }
    }
  })(doc, 0);
  return out.join('\\n') + '\\n';
}
(async () => {
  const fixtures = await (await fetch('/fixtures')).json();
  const results = fixtures.map(f => {
    const doc = new DOMParser().parseFromString(f.source, 'text/html');
    return { name: f.name, tree: dump(doc), mode: doc.compatMode };
  });
  const r = await fetch('/results', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ userAgent: navigator.userAgent, results }),
  });
  document.getElementById('log').textContent = `${results.length} fixtures: ${await r.text()}`;
})();
</script>
"""


class Handler(BaseHTTPRequestHandler):
    def _send(self, status, body, content_type="text/plain; charset=utf-8"):
        data = body.encode("utf-8")
        self.send_response(status)
        self.send_header("content-type", content_type)
        self.send_header("content-length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path == "/":
            self._send(200, PAGE, "text/html; charset=utf-8")
        elif self.path == "/fixtures":
            fixtures = [
                {"name": p.stem, "source": p.read_text(encoding="utf-8")}
                for p in sorted(FIXTURES.glob("*.html"))
            ]
            self._send(200, json.dumps(fixtures), "application/json")
        else:
            self._send(404, "not found")

    def do_POST(self):
        if self.path != "/results":
            self._send(404, "not found")
            return
        length = int(self.headers.get("content-length", "0"))
        if length > 10_000_000:
            self._send(413, "too large")
            return
        payload = json.loads(self.rfile.read(length))
        modes = [f"# user-agent: {payload['userAgent']}"]
        written = 0
        for r in payload["results"]:
            name = r["name"]
            if not NAME_RE.match(name) or not (FIXTURES / f"{name}.html").exists():
                continue
            (FIXTURES / f"{name}.tree").write_text(r["tree"], encoding="utf-8", newline="\n")
            modes.append(f"{name} {r['mode']}")
            written += 1
        (FIXTURES / "oracle.txt").write_text("\n".join(modes) + "\n", encoding="utf-8", newline="\n")
        self._send(200, f"wrote {written} trees")
        print(f"wrote {written} trees", flush=True)

    def log_message(self, *args):
        pass


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8765
    print(f"serving on http://127.0.0.1:{port}/", flush=True)
    ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
