use super::{windows, DesktopHost, WindowRole};
use std::time::Duration;
use tauri::Manager;

/// Detect topology changes outside renderer documents. All native access is
/// marshalled onto the UI thread; no desktop lock is held across native calls.
pub(crate) fn start(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mut fingerprint = String::new();
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let Some(host) = app.try_state::<DesktopHost>() else {
                return;
            };
            if host.lock().map(|state| state.exiting).unwrap_or(true) {
                return;
            }
            let handle = app.clone();
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            let previous = fingerprint.clone();
            if app
                .run_on_main_thread(move || {
                    let result = (|| {
                        let main = handle
                            .get_webview_window("main")
                            .ok_or("Pet window is unavailable")?;
                        let displays = windows::displays(&main)?;
                        let current =
                            serde_json::to_string(&displays).map_err(|error| error.to_string())?;
                        if current != previous && !previous.is_empty() {
                            let areas: Vec<_> =
                                displays.iter().map(|display| display.work_area).collect();
                            let labels: Vec<_> = handle
                                .state::<DesktopHost>()
                                .lock()?
                                .windows
                                .iter()
                                .filter(|(_, record)| {
                                    record.role != WindowRole::Pet && record.ready
                                })
                                .map(|(label, _)| label.clone())
                                .collect();
                            for label in labels {
                                if let Some(window) = handle.get_webview_window(&label) {
                                    if let Some(fitted) = windows::bounds(&window)?.restore(&areas)
                                    {
                                        windows::set_bounds(&window, fitted)?;
                                    }
                                }
                            }
                        }
                        Ok::<_, String>(current)
                    })();
                    let _ = sender.send(result);
                })
                .is_err()
            {
                return;
            }
            if let Ok(Ok(current)) = receiver.recv_timeout(Duration::from_secs(5)) {
                fingerprint = current;
            }
        }
    });
}
