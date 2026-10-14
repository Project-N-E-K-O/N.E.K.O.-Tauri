"""Serve pinned upstream window pages without running AI or user-data services.

Pass --upstream pointing to an N.E.K.O checkout with its React bundle built.
The fixture is used by native and browser integration checks. Every static
asset and window template is read from that checkout, never vendored here.
"""

import argparse
import asyncio
import json
import uuid
from pathlib import Path

from aiohttp import web
from jinja2 import Environment, FileSystemLoader


CONFIG = {"success": True, "lanlan_name": "Window Test", "model_type": "pngtuber",
          "master_name": "Test", "master_display_name": "Test", "model_path": "",
          "autostart_csrf_token": "fixture", "privacy_mode_enabled": True}
PET = """<!doctype html><html><body><script>
window.lanlan_config=CONFIG;window.pageConfigReady=Promise.resolve(true);
window._agentTaskMap=new Map();
document.addEventListener('DOMContentLoaded',async()=>{
 await window.__NEKO_DESKTOP__.ready;
 window.fixtureSocket=new WebSocket('ws://127.0.0.1:48911/ws/Window%20Test');
});
</script></body></html>"""

DRIVER = r"""<script>
document.addEventListener('DOMContentLoaded',async()=>{
 if(window.__NEKO_FIXTURE_CLIENT__==='browser')return;
 const host=window.__NEKO_DESKTOP__;
 if(!host)return;
 const role=host.profile.role;
 const post=data=>fetch('/__fixture/report',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({[role]:data})});
 async function inspect(){
  const page=window.reactChatWindowHost;
  const state=page?.getState?.();
  const geometry=window.__nekoGetCompactInteractionGeometry?.();
  return {ready:true,mounted:!!state?.mounted,mode:state?.chatSurfaceMode,
   document:(await host.ready).document,updatedAt:Date.now(),
   messages:state?.messages,draft:document.querySelector('#react-chat-window-root textarea')?.value,
   pendingAttachments:window.appButtons?.getPendingComposerAttachments?.(),
   responseState:host.responseTransfer?.capture?.(),
   geometry,bounds:await host.request('bounds')};
 }
 await host.ready;
 setInterval(async()=>{
  try{
   const commands=await fetch('/__fixture/commands?role='+role).then(response=>response.json());
   for(const command of commands){
    let result;
    if(command.kind==='native')result=await host.request(command.action,command.options||{});
    else if(command.kind==='mode')result=await window.nekoDesktopWindows.selectChatSurface(command.mode);
    else if(command.kind==='input')result=window.reactChatWindowHost.setCompactChatState('input');
    else if(command.kind==='messages')result=window.reactChatWindowHost.setMessages(command.messages);
    else if(command.kind==='attachment'){
     const url=await window.appButtons.normalizeImageDataUrlForPendingList(command.url);
     window.appButtons.addScreenshotToList(url,command.position,{source:command.source,alt:command.alt});
     result=host.composerTransfer.capture();
    }
    else if(command.kind==='send')result=await window.appButtons.sendTextPayload(command.text,{source:'react-chat-window',requestId:command.requestId});
    else if(command.kind==='merge-setting')result=(window.mergeMessagesEnabled=command.enabled);
    else if(command.kind==='draft'){
     const input=document.querySelector('#react-chat-window-root textarea');
     Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype,'value').set.call(input,command.text);
     input.dispatchEvent(new Event('input',{bubbles:true}));
    }
    await fetch('/__fixture/result',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({id:command.id,result})});
   }
   await post(await inspect());
  }catch(error){await post({error:String(error.stack||error)});}
 },250);
});
</script>"""


def create_app(upstream: Path):
    templates = Environment(loader=FileSystemLoader(upstream))
    app = web.Application()
    state = {"runId":uuid.uuid4().hex,"requests": [], "received": [], "connections": 0,
             "activeConnections":0,"report": {}, "commands": {}, "results": {}}
    sockets = set()

    async def socket(request):
        ws = web.WebSocketResponse()
        await ws.prepare(request)
        state["connections"] += 1
        sockets.add(ws)
        state["activeConnections"] = len(sockets)
        try:
            async for message in ws:
                if message.type != web.WSMsgType.TEXT:
                    continue
                data = json.loads(message.data)
                state["received"].append(data)
                del state["received"][:-256]
                if data.get("action") == "start_session":
                    await ws.send_json({"type": "session_started", "input_mode": data.get("input_type", "text"),
                                        "request_id":data.get("request_id")})
                elif data.get("action") == "stream_data" and data.get("input_type") == "text":
                    await ws.send_json({"type": "gemini_response", "isNewMessage": True, "text": "A shared response."})
                    await ws.send_json({"type": "system", "data": "turn end"})
                elif data.get("action") == "ping":
                    await ws.send_json({"type": "pong"})
        finally:
            sockets.discard(ws)
            state["activeConnections"] = len(sockets)
        return ws

    async def static(request):
        path = (upstream / request.path.lstrip("/")).resolve()
        if not path.is_relative_to(upstream.resolve()) or not path.is_file():
            raise web.HTTPNotFound()
        return web.FileResponse(path)

    async def endpoint(request):
        path = request.path
        state["requests"].append(path)
        del state["requests"][:-512]
        if path == "/__fixture/frame" and request.method == "POST":
            frame=await request.json()
            for ws in list(sockets):
                await ws.send_json(frame)
            return web.json_response({"receivers":len(sockets)})
        if path == "/health":
            return web.json_response({"app": "N.E.K.O", "service": "main"})
        if path == "/__fixture/report":
            if request.method == "POST":
                state["report"].update(await request.json())
            return web.json_response(state)
        if path == "/__fixture/commands":
            role = request.query.get("role", "")
            if request.method == "POST":
                state["commands"].setdefault(role, []).append(await request.json())
                return web.json_response({})
            return web.json_response(state["commands"].pop(role, []))
        if path == "/__fixture/result":
            data = await request.json()
            state["results"][data["id"]] = data.get("result")
            return web.json_response({})
        if path == "/":
            return web.Response(text=PET.replace("CONFIG", json.dumps(CONFIG)).replace("</body>", DRIVER + "</body>"), content_type="text/html")
        if path in {"/chat", "/chat_full", "/subtitle", "/agenthud"}:
            template = "chat.html" if path.startswith("/chat") else path[1:] + ".html"
            mode = "full" if path == "/chat_full" else "compact"
            html = templates.get_template("templates/" + template).render(vrm_defaults={},
                initial_chat_host_kind=mode, initial_chat_surface_mode=mode,
                static_asset_version="fixture", react_chat_asset_version="fixture")
            return web.Response(text=html.replace("</body>", DRIVER + "</body>"), content_type="text/html")
        if path == "/api/config/page_config":
            return web.json_response(CONFIG)
        if path == "/api/characters":
            return web.json_response({"success": True, "catgirls": {"Window Test": CONFIG}, "current_catgirl": "Window Test"})
        if path == "/api/storage-location/startup":
            return web.json_response({"success": True, "blocked": False, "needs_selection": False})
        if path == "/api/config/conversation-settings":
            return web.json_response({"success": True, "settings": {}})
        if path == "/api/agent/flags":
            return web.json_response({"success": True, "flags": {"agent_enabled": False}})
        if path == "/api/agent/tasks":
            return web.json_response({"success": True, "tasks": []})
        if path in {"/api/pending-notices", "/api/changelog"}:
            return web.json_response({"success": True, "notices": [], "entries": []})
        if path == "/api/survey":
            return web.json_response({"success": True, "survey": None})
        if path == "/api/game/route/active":
            return web.json_response({"active": False})
        if path.startswith("/api/") or path.startswith("/memory/"):
            return web.json_response({"success": True})
        raise web.HTTPNotFound()

    app.router.add_get("/ws/{character}", socket)
    app.router.add_get("/static/{path:.*}", static)
    app.router.add_route("*", "/{path:.*}", endpoint)
    return app


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("--upstream", type=Path, required=True)
    args = parser.parse_args()
    web.run_app(create_app(args.upstream), host="127.0.0.1", port=48911, print=None)
