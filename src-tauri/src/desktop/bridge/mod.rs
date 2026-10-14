use crate::desktop::role::WindowRole;

pub(crate) fn initialization_script(role: WindowRole, label: &str, parent: Option<&str>) -> String {
    let profile = serde_json::json!({ "role": role, "label": label, "parent": parent, "platform": std::env::consts::OS });
    let scripts = [
        include_str!("transport.js"),
        include_str!("websocket.js"),
        include_str!("windows.js"),
        include_str!("settings.js"),
        include_str!("pet.js"),
        include_str!("input.js"),
        include_str!("composer.js"),
        include_str!("response-state.js"),
        include_str!("chat.js"),
        include_str!("compact-layout.js"),
        include_str!("panels.js"),
        include_str!("gestures.js"),
        include_str!("ball.js"),
        include_str!("../../screen_capture.js"),
    ];
    format!(
        "window.__NEKO_DESKTOP_PROFILE__={profile};\n{}",
        scripts.join("\n")
    )
}
