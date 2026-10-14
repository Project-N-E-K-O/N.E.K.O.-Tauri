"""Render actual upstream pages with the Tauri renderer interfaces in Chromium.

The IPC adapter here is deliberately a test adapter. Native behavior is tested
by native-window-smoke.py; this probe exercises upstream DOM/React contracts.
"""

import argparse
import json
import traceback
from pathlib import Path

from playwright.sync_api import sync_playwright


BOOTSTRAP = r"""
window.__NEKO_FIXTURE_CLIENT__='browser';
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
  if(['input-region','show','hide','painted','content-ready','focus','set-resizable','drag','resize','collapse','expand','select-surface'].includes(r.action))return true;
  throw new Error('Unsupported fixture action: '+r.action);
 }
 if(name==='set_click_through')return;
 if(name==='get_screen_capture_capabilities')return {provider:'fixture',canCaptureFrames:false};
 throw new Error('Unsupported fixture command: '+name);
}}};
window.__fixtureEmit=(kind,data,sequence=100)=>{
 if(kind==='chat-snapshot' && !data.responseState)data.responseState=window.appChat.captureDesktopState();
 for(const listener of window.__fixtureEvents.get('neko-desktop-message')||[])
  listener({payload:{kind,data,sequence,epoch:1,generation:1}});
};
"""


def run(output: Path, executable: str):
    root = Path(__file__).resolve().parents[1]
    bridges = root / "src-tauri/src/desktop/bridge"
    source = "\n".join((bridges / name).read_text() for name in
        ["transport.js", "websocket.js", "windows.js", "settings.js", "pet.js", "input.js", "composer.js", "response-state.js", "chat.js", "compact-layout.js", "panels.js", "gestures.js"])
    output.mkdir(parents=True, exist_ok=True)
    report = {}
    transferred_attachments = []
    streaming_snapshot = None
    rollback_snapshot = None
    rollback_request = None
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
                page.wait_for_function("window.__NEKO_DESKTOP__.composerTransfer.available()", timeout=10000)
                page.wait_for_function("window.appState?.socket?.readyState===1 && typeof window.appState.socket.onmessage==='function'", timeout=10000)
                if role == "compact-chat":
                    transferred_attachments = page.evaluate("""async()=>{
                      const url=await window.appButtons.normalizeImageDataUrlForPendingList(
                        'data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVQIHWP4z8DwHwAFgAI/ScLbtAAAAABJRU5ErkJggg==');
                      window.appButtons.addScreenshotToList(
                        url,
                        {modelPosition:{x:10,y:20}}, {source:'user-image',alt:'Transferred image'});
                      return window.__NEKO_DESKTOP__.composerTransfer.capture();
                    }""")
                    page.evaluate("""()=>{
                      window.mergeMessagesEnabled=true;
                      window.__fixtureEmit('socket-message',JSON.stringify({type:'gemini_response',isNewMessage:true,text:'First half',turn_id:'transfer-turn'}),600);
                    }""")
                    page.wait_for_function("window.reactChatWindowHost.getState().messages.some(message=>message.role==='assistant' && message.blocks.some(block=>block.text==='First half'))", timeout=5000)
                    streaming_snapshot = page.evaluate("""()=>({
                      messages:window.reactChatWindowHost.getState().messages,
                      responseState:window.__NEKO_DESKTOP__.responseTransfer.capture(),
                      throughSequence:window.__NEKO_DESKTOP__.characterSequence,
                      composerAttachments:[],composerHidden:false,draft:''
                    })""")
                    page.evaluate("""()=>{window.appState.isTextSessionActive=true;window.reactChatWindowHost.setCompactChatState('input');}""")
                    page.locator('#react-chat-window-root textarea').fill('Rejected draft survives switching')
                    page.locator('#react-chat-window-root textarea').press('Enter')
                    page.wait_for_function("window.__fixtureRequests.some(r=>r.name==='desktop_publish' && r.args.message.kind==='socket-send' && JSON.parse(r.args.message.data).data==='Rejected draft survives switching')", timeout=5000)
                    rollback_snapshot = page.evaluate("""()=>({
                      messages:window.reactChatWindowHost.getState().messages,
                      responseState:window.appChat.captureDesktopState(),
                      throughSequence:window.__NEKO_DESKTOP__.characterSequence,
                      composerAttachments:[],composerHidden:false,draft:''
                    })""")
                    rollback_request = page.evaluate("""()=>window.__fixtureRequests
                      .filter(r=>r.name==='desktop_publish' && r.args.message.kind==='socket-send')
                      .map(r=>JSON.parse(r.args.message.data)).find(frame=>frame.data==='Rejected draft survives switching').request_id""")
                if role == "full-chat":
                    page.evaluate("""()=>window.__fixtureEmit('chat-snapshot',{
                      messages:[],composerAttachments:[],composerHidden:false,draft:'Draft handoff'
                    },500)""")
                    page.wait_for_function("document.querySelector('#react-chat-window-root textarea')?.value==='Draft handoff'", timeout=5000)
                    page.evaluate("""()=>window.__fixtureEmit('chat-snapshot',{
                      messages:[],composerAttachments:[],composerHidden:false,draft:''
                    },501)""")
                    page.wait_for_function("document.querySelector('#react-chat-window-root textarea')?.value===''", timeout=5000)
                    page.evaluate("""attachments=>window.__fixtureEmit('chat-snapshot',{
                      messages:[],composerAttachments:attachments,composerHidden:false,draft:''
                    },502)""", transferred_attachments)
                    page.wait_for_function("window.appButtons.getPendingComposerAttachments().length===1", timeout=5000)
                    metadata = page.evaluate("""()=>{
                      const item=window.appState.dom.screenshotsList.firstElementChild;
                      return {id:item.dataset.attachmentId,source:item.dataset.source,position:JSON.parse(item.dataset.avatarPosition)};
                    }""")
                    assert metadata == {"id": transferred_attachments[0]["id"], "source": "user-image",
                        "position": {"modelPosition": {"x": 10, "y": 20}}}
                    # Exercise the original React removal callback, which reads
                    # the pending screenshot DOM rather than just preview props.
                    page.locator('#react-chat-window-root .composer-attachments button').click()
                    page.wait_for_function("window.appButtons.getPendingComposerAttachments().length===0", timeout=5000)
                    page.evaluate("""attachments=>window.__fixtureEmit('chat-snapshot',{
                      messages:[],composerAttachments:attachments,composerHidden:false,draft:''
                    },503)""", transferred_attachments)
                    page.wait_for_function("window.appButtons.getPendingComposerAttachments().length===1", timeout=5000)
                    accepted = page.evaluate("""async()=>{
                      window.appState.isTextSessionActive=true;
                      return window.appButtons.sendTextPayload('Attachment handoff', {source:'react-chat-window'});
                    }""")
                    assert accepted is True, "Upstream rejected the restored composer submission"
                    page.wait_for_function("window.__fixtureRequests.some(r=>r.name==='desktop_publish' && r.args.message.kind==='socket-send' && JSON.parse(r.args.message.data).input_type==='user_image')", timeout=5000)
                    image_frame = page.evaluate("""()=>window.__fixtureRequests
                      .filter(r=>r.name==='desktop_publish' && r.args.message.kind==='socket-send')
                      .map(r=>JSON.parse(r.args.message.data)).find(frame=>frame.input_type==='user_image')""")
                    assert image_frame["data"].startswith("data:image/")
                    assert image_frame["avatar_position"] == {"modelPosition": {"x": 10, "y": 20}}
                    page.evaluate("""snapshot=>{
                      window.mergeMessagesEnabled=true;
                      window.__fixtureEmit('chat-snapshot',snapshot,700);
                      window.__fixtureEmit('socket-message',JSON.stringify({type:'gemini_response',text:' + second half',turn_id:'transfer-turn'}),601);
                    }""", streaming_snapshot)
                    page.wait_for_function("window.reactChatWindowHost.getState().messages.some(message=>message.role==='assistant' && message.blocks.some(block=>block.text==='First half + second half'))", timeout=5000)
                    messages = page.evaluate("window.reactChatWindowHost.getState().messages")
                    assert len(messages) == 1, "Continuation created a duplicate bubble"
                    assert messages[0]["id"] == streaming_snapshot["messages"][0]["id"]
                    assert messages[0]["blocks"][0]["text"] == "First half + second half"
                    page.evaluate("""data=>{
                      window.__fixtureEmit('chat-snapshot',data.snapshot,801);
                      window.__fixtureEmit('socket-message',JSON.stringify({
                        type:'response_discarded',message:{code:'RESPONSE_TOO_LONG'},request_id:data.request
                      }),802);
                    }""", {"snapshot":rollback_snapshot,"request":rollback_request})
                    page.wait_for_function("document.querySelector('#react-chat-window-root textarea')?.value==='Rejected draft survives switching'", timeout=5000)
            except Exception as error:
                errors.append(traceback.format_exc())
            report[role] = page.evaluate("""()=>({
              state:window.reactChatWindowHost?.getState?.(),
              shell:document.querySelector('#react-chat-window-shell')?.getBoundingClientRect().toJSON(),
              input:!!document.querySelector('#react-chat-window-root textarea'),
              geometry:window.__nekoGetCompactInteractionGeometry?.(),
              requests:(window.__fixtureRequests||[]).map(r=>r.name==='desktop_window'?r.args.request.action:r.name)
            })""")
            report[role]["errors"] = errors
            report[role]["attachmentHandoffPassed"] = role == "full-chat" and not errors
            page.screenshot(path=str(output / (role + ".png")))
            page.close()
        browser.close()
    (output / "report.json").write_text(json.dumps(report, ensure_ascii=False, indent=2))
    print(json.dumps({role: {"mounted": (data.get("state") or {}).get("mounted"), "shell": data.get("shell"),
        "attachmentHandoffPassed": data["attachmentHandoffPassed"], "errors": data["errors"]} for role, data in report.items()}, ensure_ascii=False))
    if any(data["errors"] for data in report.values()):
        raise SystemExit(1)


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--output", type=Path, default=Path(".artifacts/upstream-window-probe"))
    parser.add_argument("--browser", required=True)
    args = parser.parse_args()
    run(args.output, args.browser)
