use serde::{Deserialize, Serialize};
use std::ffi::CString;
use std::os::raw::{c_char, c_void};

#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct HardwareProfile {
    pub tier: String,
    pub ram_gb: u32,
    pub chip: String,
    pub disk_free_gb: u32,
}

extern "C" {
    fn sysctlbyname(
        name: *const c_char,
        oldp: *mut c_void,
        oldlenp: *mut usize,
        newp: *const c_void,
        newlen: usize,
    ) -> i32;
}

fn sysctl_string(name: &str) -> Option<String> {
    let c_name = CString::new(name).ok()?;
    let mut len: usize = 0;
    unsafe {
        if sysctlbyname(
            c_name.as_ptr(),
            std::ptr::null_mut(),
            &mut len,
            std::ptr::null(),
            0,
        ) != 0
        {
            return None;
        }
        let mut buf = vec![0u8; len];
        if sysctlbyname(
            c_name.as_ptr(),
            buf.as_mut_ptr() as *mut c_void,
            &mut len,
            std::ptr::null(),
            0,
        ) != 0
        {
            return None;
        }
        buf.truncate(len.saturating_sub(1));
        String::from_utf8(buf).ok()
    }
}

fn sysctl_u64(name: &str) -> Option<u64> {
    let c_name = CString::new(name).ok()?;
    let mut value: u64 = 0;
    let mut len = std::mem::size_of::<u64>();
    unsafe {
        if sysctlbyname(
            c_name.as_ptr(),
            &mut value as *mut u64 as *mut c_void,
            &mut len,
            std::ptr::null(),
            0,
        ) != 0
        {
            return None;
        }
    }
    Some(value)
}

/// Detects hardware characteristics via `sysctl` (FR-001) — no elevated
/// privileges required, consistent with zero-config first run (Principle I).
pub fn detect() -> HardwareProfile {
    let chip = sysctl_string("machdep.cpu.brand_string").unwrap_or_else(|| "unknown".into());
    let ram_bytes = sysctl_u64("hw.memsize").unwrap_or(0);
    let ram_gb = (ram_bytes / (1024 * 1024 * 1024)) as u32;

    // TODO: real statvfs-based free-space calculation; stubbed for this pass.
    let disk_free_gb = 0u32;

    let tier = match ram_gb {
        0..=8 => "apple-silicon-8gb",
        9..=16 => "apple-silicon-16gb",
        17..=32 => "apple-silicon-32gb",
        _ => "apple-silicon-64gb-plus",
    }
    .to_string();

    HardwareProfile {
        tier,
        ram_gb,
        chip,
        disk_free_gb,
    }
}
