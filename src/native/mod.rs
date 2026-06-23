use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};

#[cfg(target_os = "android")]
extern "C" {
    fn arda_native_probe(out: *mut c_char, out_len: usize) -> c_int;
    fn arda_native_encoder_test(
        out_path: *const c_char,
        width: c_int,
        height: c_int,
        bit_rate: c_int,
        fps: c_int,
        seconds: c_int,
        report: *mut c_char,
        report_len: usize,
    ) -> c_int;
    fn arda_native_surface_encoder_test(
        out_path: *const c_char,
        width: c_int,
        height: c_int,
        bit_rate: c_int,
        fps: c_int,
        seconds: c_int,
        report: *mut c_char,
        report_len: usize,
    ) -> c_int;
    fn arda_native_display_probe(out: *mut c_char, out_len: usize) -> c_int;
    fn arda_native_display_active_probe(out: *mut c_char, out_len: usize) -> c_int;
}

#[cfg(target_os = "android")]
pub fn probe() -> Result<String, String> {
    let mut buf = vec![0u8; 16 * 1024];
    let rc = unsafe { arda_native_probe(buf.as_mut_ptr() as *mut c_char, buf.len()) };
    if rc != 0 {
        return Err(format!("arda_native_probe failed rc={rc}"));
    }
    let cstr = CStr::from_bytes_until_nul(&buf).map_err(|err| err.to_string())?;
    Ok(cstr.to_string_lossy().into_owned())
}

#[cfg(not(target_os = "android"))]
pub fn probe() -> Result<String, String> {
    Ok("native probe is only linked for Android targets".to_string())
}

#[cfg(target_os = "android")]
pub fn encoder_test(
    out_path: &str,
    width: i32,
    height: i32,
    bit_rate: i32,
    fps: i32,
    seconds: i32,
) -> Result<String, String> {
    let path = CString::new(out_path).map_err(|_| "output path contains NUL byte".to_string())?;
    let mut report = vec![0u8; 64 * 1024];
    let rc = unsafe {
        arda_native_encoder_test(
            path.as_ptr(),
            width,
            height,
            bit_rate,
            fps,
            seconds,
            report.as_mut_ptr() as *mut c_char,
            report.len(),
        )
    };
    let cstr = CStr::from_bytes_until_nul(&report).map_err(|err| err.to_string())?;
    let text = cstr.to_string_lossy().into_owned();
    if rc != 0 {
        return Err(format!("native encoder test failed rc={rc}\n{text}"));
    }
    Ok(text)
}

#[cfg(not(target_os = "android"))]
pub fn encoder_test(
    _out_path: &str,
    _width: i32,
    _height: i32,
    _bit_rate: i32,
    _fps: i32,
    _seconds: i32,
) -> Result<String, String> {
    Err("native encoder test is only linked for Android targets".to_string())
}

#[cfg(target_os = "android")]
pub fn surface_encoder_test(
    out_path: &str,
    width: i32,
    height: i32,
    bit_rate: i32,
    fps: i32,
    seconds: i32,
) -> Result<String, String> {
    let path = CString::new(out_path).map_err(|_| "output path contains NUL byte".to_string())?;
    let mut report = vec![0u8; 64 * 1024];
    let rc = unsafe {
        arda_native_surface_encoder_test(
            path.as_ptr(),
            width,
            height,
            bit_rate,
            fps,
            seconds,
            report.as_mut_ptr() as *mut c_char,
            report.len(),
        )
    };
    let cstr = CStr::from_bytes_until_nul(&report).map_err(|err| err.to_string())?;
    let text = cstr.to_string_lossy().into_owned();
    if rc != 0 {
        return Err(format!("native surface encoder test failed rc={rc}\n{text}"));
    }
    Ok(text)
}

#[cfg(not(target_os = "android"))]
pub fn surface_encoder_test(
    _out_path: &str,
    _width: i32,
    _height: i32,
    _bit_rate: i32,
    _fps: i32,
    _seconds: i32,
) -> Result<String, String> {
    Err("native surface encoder test is only linked for Android targets".to_string())
}


#[cfg(target_os = "android")]
pub fn display_probe() -> Result<String, String> {
    let mut buf = vec![0u8; 64 * 1024];
    let rc = unsafe { arda_native_display_probe(buf.as_mut_ptr() as *mut c_char, buf.len()) };
    let cstr = CStr::from_bytes_until_nul(&buf).map_err(|err| err.to_string())?;
    let text = cstr.to_string_lossy().into_owned();
    if rc != 0 {
        return Err(format!("arda_native_display_probe failed rc={rc}\n{text}"));
    }
    Ok(text)
}

#[cfg(not(target_os = "android"))]
pub fn display_probe() -> Result<String, String> {
    Ok("native display probe is only linked for Android targets".to_string())
}


#[cfg(target_os = "android")]
pub fn display_active_probe() -> Result<String, String> {
    let mut buf = vec![0u8; 64 * 1024];
    let rc = unsafe { arda_native_display_active_probe(buf.as_mut_ptr() as *mut c_char, buf.len()) };
    let cstr = CStr::from_bytes_until_nul(&buf).map_err(|err| err.to_string())?;
    let text = cstr.to_string_lossy().into_owned();
    if rc != 0 {
        return Err(format!("arda_native_display_active_probe failed rc={rc}\n{text}"));
    }
    Ok(text)
}

#[cfg(not(target_os = "android"))]
pub fn display_active_probe() -> Result<String, String> {
    Ok("native display active probe is only linked for Android targets".to_string())
}
