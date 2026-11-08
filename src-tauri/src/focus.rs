// Brings the window a session runs in to the front: the terminal or editor that owns the Claude process.

#[cfg(any(windows, test))]
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub handle: isize,
    pub pid: u32,
    pub title: String,
}

// Nearest ancestor that owns a window wins; among its windows, the one whose title mentions the project.
#[cfg(any(windows, test))]
pub fn pick(candidates: &[Candidate], ancestors: &[u32], folder: &str) -> Option<isize> {
    let folder = folder.to_lowercase();
    ancestors.iter().find_map(|pid| {
        let own: Vec<&Candidate> = candidates.iter().filter(|c| c.pid == *pid).collect();
        own.iter()
            .find(|c| !folder.is_empty() && c.title.to_lowercase().contains(&folder))
            .or_else(|| own.first())
            .map(|c| c.handle)
    })
}

#[cfg(windows)]
mod platform {
    use super::{pick, Candidate};
    use sysinfo::{Pid, ProcessesToUpdate, System};
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM},
        UI::WindowsAndMessaging::{
            EnumWindows, GetWindow, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
            IsIconic, IsWindowVisible, SetForegroundWindow, ShowWindow, GW_OWNER, SW_RESTORE,
        },
    };

    fn ancestors(pid: u32) -> Vec<u32> {
        let Ok(mut system) = System::new() else {
            return Vec::new();
        };
        system.refresh_processes(ProcessesToUpdate::All, true);
        let mut chain = Vec::new();
        let mut current = Pid::from_u32(pid);
        while let Some(parent) = system.process(current).and_then(|p| p.parent()) {
            if chain.contains(&parent.as_u32()) || chain.len() >= 12 {
                break;
            }
            chain.push(parent.as_u32());
            current = parent;
        }
        chain
    }

    unsafe extern "system" fn collect(window: HWND, param: LPARAM) -> i32 {
        let found = &mut *(param as *mut Vec<Candidate>);
        if IsWindowVisible(window) == 0 || GetWindow(window, GW_OWNER) as isize != 0 {
            return 1;
        }
        let length = GetWindowTextLengthW(window);
        if length == 0 {
            return 1;
        }
        let mut buffer = vec![0u16; length as usize + 1];
        let copied = GetWindowTextW(window, buffer.as_mut_ptr(), buffer.len() as i32);
        let mut pid = 0;
        GetWindowThreadProcessId(window, &mut pid);
        found.push(Candidate {
            handle: window as isize,
            pid,
            title: String::from_utf16_lossy(&buffer[..copied as usize]),
        });
        1
    }

    pub fn focus(pid: u32, folder: &str) -> bool {
        let mut windows: Vec<Candidate> = Vec::new();
        unsafe {
            EnumWindows(Some(collect), &mut windows as *mut _ as LPARAM);
        }
        let Some(handle) = pick(&windows, &ancestors(pid), folder) else {
            return false;
        };
        let window = handle as HWND;
        unsafe {
            if IsIconic(window) != 0 {
                ShowWindow(window, SW_RESTORE);
            }
            SetForegroundWindow(window) != 0
        }
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn focus(_pid: u32, _folder: &str) -> bool {
        false
    }
}

pub fn focus(pid: u32, folder: &str) -> bool {
    platform::focus(pid, folder)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window(handle: isize, pid: u32, title: &str) -> Candidate {
        Candidate {
            handle,
            pid,
            title: title.into(),
        }
    }

    #[test]
    fn prefers_the_window_titled_with_the_project() {
        let windows = [
            window(1, 10, "other - Visual Studio Code"),
            window(2, 10, "my-repo - Visual Studio Code"),
        ];
        assert_eq!(pick(&windows, &[10], "My-Repo"), Some(2));
    }

    #[test]
    fn falls_back_to_the_first_window_of_the_nearest_ancestor() {
        let windows = [window(1, 20, "Terminal"), window(2, 10, "Editor")];
        assert_eq!(pick(&windows, &[5, 10, 20], "repo"), Some(2));
        assert_eq!(pick(&windows, &[5], "repo"), None);
    }
}
