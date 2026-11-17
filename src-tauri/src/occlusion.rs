// Explains why a widget Windows reports as visible may still not be seen: cloaked by the shell,
// no longer topmost, or under other windows.

#[cfg(any(windows, test))]
use serde_json::{json, Value};

#[cfg(any(windows, test))]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[cfg(any(windows, test))]
impl Rect {
    fn overlaps(&self, other: &Rect) -> bool {
        self.left.max(other.left) < self.right.min(other.right)
            && self.top.max(other.top) < self.bottom.min(other.bottom)
    }

    fn to_json(self) -> Value {
        json!([self.left, self.top, self.right, self.bottom])
    }
}

#[cfg(any(windows, test))]
#[derive(Debug, Clone, PartialEq)]
pub struct Above {
    pub rect: Rect,
    pub visible: bool,
    pub cloaked: bool,
    pub topmost: bool,
    pub layered: bool,
    pub pid: u32,
    pub class: String,
}

// Windows above the widget that are drawn over part of it.
#[cfg(any(windows, test))]
pub fn covering<'a>(widget: &Rect, above: &'a [Above]) -> Vec<&'a Above> {
    above
        .iter()
        .filter(|w| w.visible && !w.cloaked && w.rect.overlaps(widget))
        .collect()
}

#[cfg(any(windows, test))]
const MAX_REPORTED: usize = 8;

#[cfg(any(windows, test))]
pub fn report(
    widget: &Rect,
    topmost: bool,
    cloaked: u32,
    covers: &[&Above],
    process: impl Fn(u32) -> String,
) -> Value {
    let covered_by: Vec<Value> = covers
        .iter()
        .take(MAX_REPORTED)
        .map(|w| {
            json!({ "process": process(w.pid), "class": w.class, "topmost": w.topmost,
                    "layered": w.layered, "rect": w.rect.to_json() })
        })
        .collect();
    json!({ "rect": widget.to_json(), "topmost": topmost, "cloaked": cloaked,
            "coveredBy": covered_by, "coveredCount": covers.len() })
}

#[cfg(windows)]
mod platform {
    use super::{covering, report, Above, Rect};
    use serde_json::Value;
    use sysinfo::{Pid, ProcessesToUpdate, System};
    use windows_sys::Win32::{
        Foundation::{HWND, RECT},
        Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED},
        UI::WindowsAndMessaging::{
            GetClassNameW, GetWindow, GetWindowLongW, GetWindowRect, GetWindowThreadProcessId,
            IsWindowVisible, GWL_EXSTYLE, GW_HWNDPREV, WS_EX_LAYERED, WS_EX_TOPMOST,
        },
    };

    // Bounds the z-order walk in case windows are reordered while it runs.
    const MAX_WALK: usize = 2000;

    unsafe fn rect(window: HWND) -> Rect {
        let mut r = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        GetWindowRect(window, &mut r);
        Rect {
            left: r.left,
            top: r.top,
            right: r.right,
            bottom: r.bottom,
        }
    }

    unsafe fn cloaked(window: HWND) -> u32 {
        let mut value = 0u32;
        DwmGetWindowAttribute(
            window,
            DWMWA_CLOAKED as u32,
            &mut value as *mut u32 as *mut _,
            std::mem::size_of::<u32>() as u32,
        );
        value
    }

    unsafe fn ex_style(window: HWND) -> u32 {
        GetWindowLongW(window, GWL_EXSTYLE) as u32
    }

    unsafe fn class(window: HWND) -> String {
        let mut buffer = [0u16; 128];
        let length = GetClassNameW(window, buffer.as_mut_ptr(), buffer.len() as i32);
        String::from_utf16_lossy(&buffer[..length.max(0) as usize])
    }

    pub fn diagnose(widget: HWND) -> Value {
        let mut above = Vec::new();
        let (widget_rect, widget_topmost, widget_cloaked) = unsafe {
            let mut window = GetWindow(widget, GW_HWNDPREV);
            while !window.is_null() && above.len() < MAX_WALK {
                let style = ex_style(window);
                let mut pid = 0;
                GetWindowThreadProcessId(window, &mut pid);
                above.push(Above {
                    rect: rect(window),
                    visible: IsWindowVisible(window) != 0,
                    cloaked: cloaked(window) != 0,
                    topmost: style & WS_EX_TOPMOST != 0,
                    layered: style & WS_EX_LAYERED != 0,
                    pid,
                    class: class(window),
                });
                window = GetWindow(window, GW_HWNDPREV);
            }
            (
                rect(widget),
                ex_style(widget) & WS_EX_TOPMOST != 0,
                cloaked(widget),
            )
        };
        let covers = covering(&widget_rect, &above);
        let mut system = System::new().ok();
        if let Some(system) = system.as_mut() {
            let pids: Vec<Pid> = covers.iter().map(|w| Pid::from_u32(w.pid)).collect();
            system.refresh_processes(ProcessesToUpdate::Some(&pids), true);
        }
        report(
            &widget_rect,
            widget_topmost,
            widget_cloaked,
            &covers,
            |pid| {
                system
                    .as_ref()
                    .and_then(|s| s.process(Pid::from_u32(pid)))
                    .map_or_else(
                        || pid.to_string(),
                        |p| p.name().to_string_lossy().into_owned(),
                    )
            },
        )
    }
}

#[cfg(windows)]
pub fn diagnose(window: &tauri::WebviewWindow) -> Option<serde_json::Value> {
    window.hwnd().ok().map(|hwnd| platform::diagnose(hwnd.0))
}

#[cfg(not(windows))]
pub fn diagnose(_window: &tauri::WebviewWindow) -> Option<serde_json::Value> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> Rect {
        Rect {
            left,
            top,
            right,
            bottom,
        }
    }

    fn window(rect: Rect, visible: bool, cloaked: bool, class: &str) -> Above {
        Above {
            rect,
            visible,
            cloaked,
            topmost: true,
            layered: false,
            pid: 7,
            class: class.into(),
        }
    }

    #[test]
    fn only_shown_windows_over_the_widget_cover_it() {
        let widget = rect(100, 100, 380, 200);
        let above = [
            window(rect(0, 0, 1920, 1080), true, false, "Overlay"),
            window(rect(0, 0, 1920, 1080), false, false, "Hidden"),
            window(rect(0, 0, 1920, 1080), true, true, "Cloaked"),
            window(rect(380, 100, 500, 200), true, false, "Touching"),
            window(rect(300, 150, 400, 250), true, false, "Corner"),
        ];
        let classes: Vec<&str> = covering(&widget, &above)
            .iter()
            .map(|w| w.class.as_str())
            .collect();
        assert_eq!(classes, ["Overlay", "Corner"]);
    }

    #[test]
    fn report_names_processes_and_caps_the_list() {
        let widget = rect(0, 0, 10, 10);
        let above: Vec<Above> = (0..MAX_REPORTED + 3)
            .map(|i| window(rect(0, 0, 10, 10), true, false, &format!("W{i}")))
            .collect();
        let covers = covering(&widget, &above);
        let value = report(&widget, true, 0, &covers, |pid| format!("app{pid}.exe"));
        assert_eq!(value["coveredCount"], MAX_REPORTED + 3);
        assert_eq!(value["coveredBy"].as_array().unwrap().len(), MAX_REPORTED);
        assert_eq!(value["coveredBy"][0]["process"], "app7.exe");
        assert_eq!(value["coveredBy"][0]["class"], "W0");
        assert_eq!(value["topmost"], true);
        assert_eq!(value["rect"], json!([0, 0, 10, 10]));
    }
}
