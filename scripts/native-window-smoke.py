"""Deterministic HTTP fixture for real Tauri/WebKit native-window smoke tests.

This fixture validates the native host seam, not the complete upstream UI. Run
it on the normal backend port, start Tauri with isolated XDG directories under
Xvfb, and inspect /report after the host exits. It never starts AI services.
"""

import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer


REPORT = {"documents": {}, "checks": [], "errors": []}
LOCK = threading.Lock()

PAGE = r"""<!doctype html><html><head><meta charset="utf-8">
<style>html,body{margin:0;background:transparent}#react-chat-window-shell{width:400px;height:200px;background:#dceeff}#react-chat-window-root{height:100%}button{margin:16px}</style>
<script>window.lanlan_config={lanlan_name:'smoke',model_type:'pngtuber'};window.pageConfigReady=Promise.resolve();</script>
</head><body><section id="react-chat-window-shell"><div id="react-chat-window-root"><button>Native smoke</button></div></section>
<script>
const report = data => fetch('/report',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(data)});
const wait = delay => new Promise(resolve=>setTimeout(resolve,delay));
window.addEventListener('error',event=>report({error:event.message}));
window.addEventListener('unhandledrejection',event=>report({error:String(event.reason)}));
document.addEventListener('DOMContentLoaded', async()=>{
  try {
    const host=window.__NEKO_DESKTOP__;
    if(!host) throw new Error('Document-start desktop bridge is missing');
    const state=await host.ready;
    await report({document:host.profile.role,state});
    if(host.profile.role!=='pet')return;
    await wait(500);
    await host.request('select-surface',{surface:'full'});
    await wait(500);
    const documents=await fetch('/report').then(response=>response.json());
    if(!documents.documents['full-chat'])throw new Error('Full chat did not register its native document');
    await report({check:'full native window ready'});
    for(const role of ['subtitle','agent-hud']) {
      await host.request('show',{role});
      await wait(500);
      const state=await fetch('/report').then(response=>response.json());
      if(!state.documents[role])throw new Error(role+' did not register');
      await report({check:role+' native window ready'});
    }
    const child=await host.request('open-child',{url:'/settings',name:'smoke-settings'});
    await wait(500);
    const reused=await host.request('open-child',{url:'/settings',name:'smoke-settings'});
    if(child.label!==reused.label)throw new Error('Named child was not reused');
    await report({check:'named child reuse'});
    await host.request('post-message',{target:child.label,data:{smoke:true}});
    await wait(250);
    const deliveryReport=await fetch('/report').then(response=>response.json());
    if(deliveryReport.checks.filter(check=>check==='parent-child message delivered').length!==1)
      throw new Error('Parent-child event was broadcast outside its intended receiver');
    await host.request('close-child',{label:child.label});
    await report({check:'managed child close'});
    await host.request('select-surface',{surface:'compact'});
    await report({check:'switch back to compact'});
    await host.request('close');
  }catch(error){await report({error:String(error.stack||error)});}
});
window.addEventListener('message',event=>{if(event.data?.smoke)report({check:'parent-child message delivered'});});
</script></body></html>"""


class Fixture(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path == "/health":
            self.respond(200, "application/json", json.dumps({"app": "N.E.K.O", "service": "main"}).encode())
        elif self.path == "/report":
            with LOCK:
                data = json.dumps(REPORT).encode()
            self.respond(200, "application/json", data)
        else:
            self.respond(200, "text/html; charset=utf-8", PAGE.encode())

    def do_POST(self):
        if self.path != "/report":
            self.respond(404, "text/plain", b"not found")
            return
        data = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        with LOCK:
            if "document" in data:
                REPORT["documents"][data["document"]] = data["state"]
            if "check" in data:
                REPORT["checks"].append(data["check"])
            if "error" in data:
                REPORT["errors"].append(data["error"])
        self.respond(200, "application/json", b"{}")

    def respond(self, status, content_type, data):
        self.send_response(status)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *_args):
        pass


if __name__ == "__main__":
    ThreadingHTTPServer(("127.0.0.1", 48911), Fixture).serve_forever()
