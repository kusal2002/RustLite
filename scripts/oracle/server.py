"""Reference-browser oracle for RustLite golden tests.

Serves fixtures to a browser page that computes the expected results with the
browser's own engine and POSTs them back:

* /html  - tests/html/*.html parsed with DOMParser, serialized in
           html5lib-tests tree format -> tests/html/<name>.tree,
           document modes -> tests/html/oracle.txt
* /style - tests/style/*.html rendered in an 800x600 iframe; getComputedStyle
           values for every element -> tests/style/<name>.styles,
           metadata (user agent, devicePixelRatio) -> tests/style/oracle.txt

Usage:
    python scripts/oracle/server.py [port]
Then open http://127.0.0.1:<port>/html and/or /style in the reference browser.

Binds to 127.0.0.1 only and accepts results only for fixtures that exist.
"""

import json
import pathlib
import re
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

ROOT = pathlib.Path(__file__).resolve().parents[2]
SUITES = {
    "html": (ROOT / "tests" / "html", ".tree"),
    "style": (ROOT / "tests" / "style", ".styles"),
}
NAME_RE = re.compile(r"^[a-z0-9-]+$")

COMMON_JS = """
async function run(suite, compute) {
  const fixtures = await (await fetch(`/${suite}/fixtures`)).json();
  const results = [];
  for (const f of fixtures) results.push({ name: f.name, ...(await compute(f)) });
  const r = await fetch(`/${suite}/results`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ userAgent: navigator.userAgent, dpr: devicePixelRatio, results }),
  });
  document.getElementById('log').textContent = `${results.length} fixtures: ${await r.text()}`;
}
"""

HTML_PAGE = """<!DOCTYPE html><meta charset=utf-8><title>RustLite HTML oracle</title>
<pre id=log>running…</pre>
<script>%s
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
run('html', async f => {
  const doc = new DOMParser().parseFromString(f.source, 'text/html');
  return { output: dump(doc), mode: doc.compatMode };
});
</script>
""" % COMMON_JS

STYLE_PAGE = """<!DOCTYPE html><meta charset=utf-8><title>RustLite style oracle</title>
<pre id=log>running…</pre>
<script>%s
const DEFAULT_PROPS = %s;
function propsFor(source) {
  const m = source.match(/<!--\\s*compare:([^>]*?)-->/);
  return m ? m[1].trim().split(/\\s+/) : DEFAULT_PROPS;
}
run('style', f => new Promise(resolve => {
  const frame = document.createElement('iframe');
  frame.style.cssText = 'width:800px;height:600px;border:0;display:block';
  frame.onload = () => {
    const doc = frame.contentDocument;
    const props = propsFor(f.source);
    const lines = [];
    for (const el of doc.querySelectorAll('*')) {
      lines.push(`<${el.localName}${el.id ? '#' + el.id : ''}>`);
      const cs = frame.contentWindow.getComputedStyle(el);
      for (const p of props) lines.push(`  ${p}: ${cs.getPropertyValue(p)}`);
    }
    const mode = doc.compatMode;
    frame.remove();
    resolve({ output: lines.join('\\n') + '\\n', mode });
  };
  frame.srcdoc = f.source;
  document.body.appendChild(frame);
}));
</script>
"""

DEFAULT_STYLE_PROPS = [
    "display", "color", "background-color", "font-size", "font-weight", "font-style",
    "line-height", "text-align", "overflow-x", "overflow-y",
    "margin-top", "margin-right", "margin-bottom", "margin-left",
    "padding-top", "padding-right", "padding-bottom", "padding-left",
    "border-top-width", "border-right-width", "border-bottom-width", "border-left-width",
    "border-top-style", "border-right-style", "border-bottom-style", "border-left-style",
    "border-top-color", "border-right-color", "border-bottom-color", "border-left-color",
]


class Handler(BaseHTTPRequestHandler):
    def _send(self, status, body, content_type="text/plain; charset=utf-8"):
        data = body.encode("utf-8")
        self.send_response(status)
        self.send_header("content-type", content_type)
        self.send_header("content-length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def do_GET(self):
        if self.path == "/html":
            self._send(200, HTML_PAGE, "text/html; charset=utf-8")
        elif self.path == "/style":
            page = STYLE_PAGE % (COMMON_JS, json.dumps(DEFAULT_STYLE_PROPS))
            self._send(200, page, "text/html; charset=utf-8")
        elif self.path.endswith("/fixtures") and self.path.strip("/").split("/")[0] in SUITES:
            directory, _ = SUITES[self.path.strip("/").split("/")[0]]
            fixtures = [
                {"name": p.stem, "source": p.read_text(encoding="utf-8")}
                for p in sorted(directory.glob("*.html"))
            ]
            self._send(200, json.dumps(fixtures), "application/json")
        else:
            self._send(404, "not found")

    def do_POST(self):
        suite = self.path.strip("/").split("/")[0]
        if suite not in SUITES or not self.path.endswith("/results"):
            self._send(404, "not found")
            return
        directory, extension = SUITES[suite]
        length = int(self.headers.get("content-length", "0"))
        if length > 20_000_000:
            self._send(413, "too large")
            return
        payload = json.loads(self.rfile.read(length))
        meta = [f"# user-agent: {payload['userAgent']}", f"# device-pixel-ratio: {payload['dpr']}"]
        written = 0
        for r in payload["results"]:
            name = r["name"]
            if not NAME_RE.match(name) or not (directory / f"{name}.html").exists():
                continue
            (directory / f"{name}{extension}").write_text(r["output"], encoding="utf-8", newline="\n")
            meta.append(f"{name} {r['mode']}")
            written += 1
        (directory / "oracle.txt").write_text("\n".join(meta) + "\n", encoding="utf-8", newline="\n")
        self._send(200, f"wrote {written} {suite} results")
        print(f"wrote {written} {suite} results", flush=True)

    def log_message(self, *args):
        pass


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8765
    print(f"serving on http://127.0.0.1:{port}/html and /style", flush=True)
    ThreadingHTTPServer(("127.0.0.1", port), Handler).serve_forever()
