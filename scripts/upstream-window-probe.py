"""Render actual upstream pages with the Tauri renderer interfaces in Chromium.

The IPC adapter here is deliberately a test adapter. Native behavior is tested
by native-window-smoke.py; this probe exercises upstream DOM/React contracts.
"""

import argparse
import json
from pathlib import Path

from playwright.sync_api import sync_playwright


BOOTSTRAP = r"""
window.__NEKO_DESKTOP_PROFILE__=__PROFILE_DATA__;
window.__fixtureRequests=[];
window.__fixtureEvents=new Map();
let fixtureBounds={x:100,y:100,width:640,height:640};
window.__TAURI__={event:{listen:async(name,handler)=>{
 if(!window.__fixtureEvents.has(name))window.__fixtureEvents.set(name,new Set());
 window.__fixtureEvents.get(name).add(handler);
 return ()=>window.__fixtureEvents.get(name)?.delete(handler);
}},core:{invoke:async(name,args)=>{
 window.__fixtureRequests.push({name,args});
 if(name==='desktop_ready')return {document:1,epoch:1,parent:null,visible:true,
  surface:window.__NEKO_DESKTOP_PROFILE__.role==='full-chat'?'full':'compact',bounds:fixtureBounds,
  replay:[{kind:'config-result',data:{lanlan_name:'Window Test',model_type:'pngtuber',master_name:'Test'},epoch:1,generation:1,sequence:1},
   {kind:'session-ready',data:{url:'ws://127.0.0.1:48911/ws/Window%20Test'},epoch:1,generation:1,sequence:2}]};
 if(name==='desktop_publish')return;
 if(name==='desktop_window'){
  const r=args.request;
  if(r.action==='bounds')return fixtureBounds;
  if(r.action==='work-area')return {x:0,y:0,width:1280,height:1024};
  if(r.action==='cursor')return {x:-100,y:-100};
  if(r.action==='set-bounds'){fixtureBounds=r.bounds;return fixtureBounds;}
  if(r.action==='open-child')return {label:'child-fixture'};
  if(['input-region','show','hide','painted','focus','set-resizable','drag','resize','collapse','expand','select-surface'].includes(r.action))return true;
  throw new Error('Unsupported fixture action: '+r.action);
 }
 if(name==='set_click_through')return;
 if(name==='get_screen_capture_capabilities')return {provider:'fixture',canCaptureFrames:false};
 throw new Error('Unsupported fixture command: '+name);
}}};
window.__fixtureEmit=(kind,data,sequence=100)=>{
 for(const listener of window.__fixtureEvents.get('neko-desktop-message')||[])
  listener({payload:{kind,data,sequence,epoch:1,generation:1}});
};
"""


def run(output: Path, executable: str):
    root = Path(__file__).resolve().parents[1]
    bridges = root / "src-tauri/src/desktop/bridge"
    source = "\n".join((bridges / name).read_text() for name in
        ["transport.js", "websocket.js", "windows.js", "settings.js", "pet.js", "input.js", "chat.js", "compact-layout.js", "panels.js", "gestures.js"])
    output.mkdir(parents=True, exist_ok=True)
    report = {}
    with sync_playwright() as playwright:
        browser = playwright.chromium.launch(executable_path=executable, headless=True)
        for role, route in [("compact-chat", "/chat"), ("full-chat", "/chat_full")]:
            page = browser.new_page(viewport={"width": 640, "height": 640})
            errors = []
            page.on("pageerror", lambda error: errors.append(str(error)))
            profile = json.dumps({"role": role, "label": role, "parent": "main", "platform": "linux"})
            page.add_init_script(BOOTSTRAP.replace("__PROFILE_DATA__", profile) + "\n" + source)
            response = page.goto("http://127.0.0.1:48911" + route, wait_until="domcontentloaded")
            if not response or response.status != 200:
                raise RuntimeError(f"Upstream fixture returned {response.status if response else 'no response'} for {route}")
            try:
                page.wait_for_function("window.reactChatWindowHost?.getState?.().mounted", timeout=20000)
                page.wait_for_function("!!document.querySelector('#react-chat-window-root .chat-window')", timeout=10000)
                if role == "full-chat":
                    page.evaluate("""()=>window.__fixtureEmit('chat-snapshot',{
                      messages:[],composerAttachments:[],composerHidden:false,draft:'Draft handoff'
                    },500)""")
                    page.wait_for_function("document.querySelector('#react-chat-window-root textarea')?.value==='Draft handoff'", timeout=5000)
                    page.evaluate("""()=>window.__fixtureEmit('chat-snapshot',{
                      messages:[],composerAttachments:[],composerHidden:false,draft:''
                    },501)""")
                    page.wait_for_function("document.querySelector('#react-chat-window-root textarea')?.value===''", timeout=5000)
            except Exception as error:
                errors.append(str(error))
            report[role] = page.evaluate("""()=>({
              state:window.reactChatWindowHost?.getState?.(),
              shell:document.querySelector('#react-chat-window-shell')?.getBoundingClientRect().toJSON(),
              input:!!document.querySelector('#react-chat-window-root textarea'),
              geometry:window.__nekoGetCompactInteractionGeometry?.(),
              requests:(window.__fixtureRequests||[]).map(r=>r.name==='desktop_window'?r.args.request.action:r.name)
            })""")
            report[role]["errors"] = errors
            page.screenshot(path=str(output / (role + ".png")))
            page.close()
        browser.close()
    (output / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2))
    print(json.dumps({role: {"mounted": (data.get("state") or {}).get("mounted"), "shell": data.get("shell"),
        "errors": data["errors"]} for role, data in report.items()}, ensure_ascii=False))


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, default=Path(".artifacts/upstream-window-probe"))
    parser.add_argument("--browser", required=True)
    args = parser.parse_args()
    run(args.output, args.browser)
