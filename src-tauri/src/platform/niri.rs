//! Compositor-owned satellite placement. This is independent from the Pet
//! layer surface: ordinary native windows are addressed by title and process.
use serde_json::Value;
use std::{
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Geometry {
    pub(crate) x: f64,
    pub(crate) y: f64,
    pub(crate) width: f64,
    pub(crate) height: f64,
}

fn command(args: &[String]) -> Result<String, String> {
    let mut child = Command::new("niri")
        .arg("msg")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    let deadline = Instant::now() + Duration::from_millis(750);
    loop {
        if child
            .try_wait()
            .map_err(|error| error.to_string())?
            .is_some()
        {
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Niri operation timed out".into());
        }
        thread::sleep(Duration::from_millis(5));
    }
    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err("Niri rejected the window operation".into());
    }
    String::from_utf8(output.stdout).map_err(|error| error.to_string())
}

fn owned_window<'a>(windows: &'a Value, title: &str, pid: u32) -> Option<&'a Value> {
    windows.as_array()?.iter().find(|window| {
        window.get("title").and_then(Value::as_str) == Some(title)
            && window.get("pid").and_then(Value::as_u64) == Some(u64::from(pid))
    })
}

fn geometry(window: &Value) -> Option<Geometry> {
    let layout = window.get("layout")?;
    let position = layout.get("tile_pos_in_workspace_view")?.as_array()?;
    let size = layout.get("window_size")?.as_array()?;
    let value = Geometry {
        x: position.first()?.as_f64()?,
        y: position.get(1)?.as_f64()?,
        width: size.first()?.as_f64()?,
        height: size.get(1)?.as_f64()?,
    };
    ([value.x, value.y, value.width, value.height]
        .iter()
        .all(|value| value.is_finite())
        && value.width > 0.0
        && value.height > 0.0)
        .then_some(value)
}

fn query(title: &str) -> Result<(u64, Geometry, bool), String> {
    let windows: Value = serde_json::from_str(&command(&["--json".into(), "windows".into()])?)
        .map_err(|error| error.to_string())?;
    let window =
        owned_window(&windows, title, std::process::id()).ok_or("Niri window is not mapped yet")?;
    Ok((
        window
            .get("id")
            .and_then(Value::as_u64)
            .ok_or("Niri window has no identity")?,
        geometry(window).ok_or("Niri window geometry is unavailable")?,
        window
            .get("is_floating")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    ))
}

pub(crate) fn bounds(title: &str) -> Result<Geometry, String> {
    query(title).map(|(_, bounds, _)| bounds)
}

pub(crate) fn place(title: &str, desired: Geometry) -> Result<(), String> {
    let (id, before, floating) = query(title)?;
    let id = id.to_string();
    if !floating {
        command(&[
            "action".into(),
            "move-window-to-floating".into(),
            "--id".into(),
            id.clone(),
        ])?;
    }
    let result = (|| {
        command(&[
            "action".into(),
            "set-window-width".into(),
            "--id".into(),
            id.clone(),
            desired.width.round().to_string(),
        ])?;
        command(&[
            "action".into(),
            "set-window-height".into(),
            "--id".into(),
            id.clone(),
            desired.height.round().to_string(),
        ])?;
        let (_, current, _) = query(title)?;
        command(&[
            "action".into(),
            "move-floating-window".into(),
            "--id".into(),
            id.clone(),
            format!("--x={:+.0}", desired.x - current.x),
            format!("--y={:+.0}", desired.y - current.y),
        ])?;
        let (_, actual, _) = query(title)?;
        if [
            actual.x - desired.x,
            actual.y - desired.y,
            actual.width - desired.width,
            actual.height - desired.height,
        ]
        .iter()
        .any(|value| value.abs() > 1.0)
        {
            return Err("Niri placement was not confirmed".into());
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = command(&[
            "action".into(),
            "set-window-width".into(),
            "--id".into(),
            id.clone(),
            before.width.round().to_string(),
        ]);
        let _ = command(&[
            "action".into(),
            "set-window-height".into(),
            "--id".into(),
            id,
            before.height.round().to_string(),
        ]);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn compositor_identity_requires_both_title_and_process() {
        let windows = json!([{"id":1,"title":"N.E.K.O [chat-full]","pid":99},
            {"id":2,"title":"N.E.K.O [chat-full]","pid":42}]);
        assert_eq!(
            owned_window(&windows, "N.E.K.O [chat-full]", 42).unwrap()["id"],
            2
        );
        assert!(owned_window(&windows, "N.E.K.O [chat-full]", 77).is_none());
    }
    #[test]
    fn reads_compositor_logical_geometry_without_dpr_multiplication() {
        let window = json!({"layout":{"tile_pos_in_workspace_view":[-1280.0,24.0],"window_size":[440.0,100.0]}});
        assert_eq!(
            geometry(&window),
            Some(Geometry {
                x: -1280.0,
                y: 24.0,
                width: 440.0,
                height: 100.0
            })
        );
    }
}
