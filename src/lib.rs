//! C ABI around adblock-rust for NEAT.
//!
//! Ownership rules:
//!  * `NeatEngine*` comes from `neat_engine_create` and is released with `neat_engine_free`.
//!  * Every `char*` returned by this library is released with `neat_string_free`, except the
//!    strings inside a `NeatCheckResult`, which are released with `neat_check_result_free`.
//!  * Input strings are UTF-8, NUL terminated, and only borrowed for the duration of the call.
//!
//! All functions are safe to call concurrently on the same engine. No function lets a Rust panic
//! cross the FFI boundary; a panic is reported as `NEAT_ERR_INTERNAL`.

use adblock::lists::{FilterSet, ParseOptions};
use adblock::request::Request;
use adblock::resources::{PermissionMask, Resource};
use adblock::Engine;
use serde_json::Value;
use std::collections::HashSet;
use std::ffi::{c_char, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::Path;
use std::ptr;

/// Bumped on any breaking change of the exported functions or structs.
pub const NEAT_ABI_VERSION: u32 = 1;

pub const NEAT_OK: i32 = 0;
pub const NEAT_ERR_ARGS: i32 = -1; // null pointer or invalid UTF-8 / JSON
pub const NEAT_ERR_URL: i32 = -2; // a URL could not be parsed
pub const NEAT_ERR_INTERNAL: i32 = -3; // panic caught at the boundary

pub const NEAT_ACTION_ALLOW: i32 = 0;
pub const NEAT_ACTION_BLOCK: i32 = 1;
pub const NEAT_ACTION_REDIRECT: i32 = 2;

/// Bit 0 = "trusted uBO list". Must match TRUSTED_PERMISSION in neat-filters/scripts/build.mjs.
const TRUSTED: PermissionMask = PermissionMask::from_bits(1);

pub struct NeatEngine {
    network: Engine,
    popup: Engine,
    /// Rules for a navigation to a page itself (`$document`, `$all`), see neat-filters' documents.txt.
    document: Engine,
}

#[repr(C)]
pub struct NeatCheckResult {
    /// NEAT_ACTION_ALLOW / BLOCK / REDIRECT.
    pub action: i32,
    /// For NEAT_ACTION_REDIRECT: the replacement resource (a data: URL) to serve instead.
    pub redirect: *mut c_char,
    /// Non-NULL when the URL should be rewritten (`$removeparam`); only meaningful on ALLOW.
    pub rewritten_url: *mut c_char,
}

// ---------------------------------------------------------------- helpers

fn guard<T>(on_panic: T, f: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(on_panic)
}

unsafe fn str_arg<'a>(p: *const c_char) -> Option<&'a str> {
    if p.is_null() {
        return None;
    }
    CStr::from_ptr(p).to_str().ok()
}

fn to_c(s: String) -> *mut c_char {
    CString::new(s.replace('\0', "")).map(CString::into_raw).unwrap_or(ptr::null_mut())
}

unsafe fn write_out(out: *mut *mut c_char, s: String) {
    if !out.is_null() {
        *out = to_c(s);
    }
}

unsafe fn clear_out(out: *mut *mut c_char) {
    if !out.is_null() {
        *out = ptr::null_mut();
    }
}

fn read_json(path: &Path) -> Result<Value, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
}

fn load_engines(dir: &Path) -> Result<NeatEngine, String> {
    let manifest = read_json(&dir.join("manifest.json"))?;
    let files = manifest["files"].as_array().ok_or("manifest.json: no files array")?;

    let mut network_set = FilterSet::new(false);
    let mut popup_set = FilterSet::new(false);
    let mut document_set = FilterSet::new(false);
    let mut resources: Vec<Resource> = Vec::new();

    for f in files {
        let name = f["name"].as_str().ok_or("manifest.json: file without name")?;
        let path = dir.join(name);
        match f["kind"].as_str() {
            Some("list") => {
                let text = std::fs::read_to_string(&path).map_err(|e| format!("{name}: {e}"))?;
                let role = f["role"].as_str().unwrap_or("network");
                let opts = ParseOptions {
                    permissions: if f["trusted"] == true { TRUSTED } else { PermissionMask::default() },
                    ..Default::default()
                };
                match role {
                    "popup" => &mut popup_set,
                    "document" => &mut document_set,
                    _ => &mut network_set,
                }
                .add_filter_list(text, opts);
            }
            Some("resources") => {
                let text = std::fs::read_to_string(&path).map_err(|e| format!("{name}: {e}"))?;
                let mut r: Vec<Resource> =
                    serde_json::from_str(&text).map_err(|e| format!("{name}: {e}"))?;
                resources.append(&mut r);
            }
            _ => {} // unknown kinds are ignored so newer manifests keep working
        }
    }

    let mut network = Engine::new_with_filter_set(network_set);
    network.use_resources(resources);
    let popup = Engine::new_with_filter_set(popup_set);
    let document = Engine::new_with_filter_set(document_set);
    Ok(NeatEngine { network, popup, document })
}

// ---------------------------------------------------------------- exported API

#[no_mangle]
pub extern "C" fn neat_abi_version() -> u32 {
    NEAT_ABI_VERSION
}

/// Builds the engines from a directory holding a neat-filters release (`manifest.json` plus the
/// files it lists). The caller is expected to have verified the files' hashes.
///
/// Returns NULL on failure and, if `error_out` is not NULL, stores an error message in it
/// (release with `neat_string_free`). Building takes a while; call it off the UI thread.
#[no_mangle]
pub unsafe extern "C" fn neat_engine_create(dir: *const c_char, error_out: *mut *mut c_char) -> *mut NeatEngine {
    clear_out(error_out);
    let Some(dir) = str_arg(dir) else {
        write_out(error_out, "invalid directory argument".into());
        return ptr::null_mut();
    };
    match catch_unwind(AssertUnwindSafe(|| load_engines(Path::new(dir)))) {
        Ok(Ok(engine)) => Box::into_raw(Box::new(engine)),
        Ok(Err(e)) => {
            write_out(error_out, e);
            ptr::null_mut()
        }
        Err(_) => {
            write_out(error_out, "internal error while building the engine".into());
            ptr::null_mut()
        }
    }
}

#[no_mangle]
pub unsafe extern "C" fn neat_engine_free(engine: *mut NeatEngine) {
    if !engine.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| drop(Box::from_raw(engine))));
    }
}

#[no_mangle]
pub unsafe extern "C" fn neat_string_free(s: *mut c_char) {
    if !s.is_null() {
        drop(CString::from_raw(s));
    }
}

#[no_mangle]
pub unsafe extern "C" fn neat_check_result_free(r: *mut NeatCheckResult) {
    if r.is_null() {
        return;
    }
    neat_string_free((*r).redirect);
    neat_string_free((*r).rewritten_url);
    (*r).redirect = ptr::null_mut();
    (*r).rewritten_url = ptr::null_mut();
    (*r).action = NEAT_ACTION_ALLOW;
}

/// Checks one network request. `request_type` is one of: document, subdocument, stylesheet,
/// image, media, font, script, xmlhttprequest, ping, websocket, object, other. `method` may be
/// NULL. `result` is always initialised on return and must be released with
/// `neat_check_result_free`.
#[no_mangle]
pub unsafe extern "C" fn neat_check_request(
    engine: *const NeatEngine,
    url: *const c_char,
    source_url: *const c_char,
    request_type: *const c_char,
    method: *const c_char,
    result: *mut NeatCheckResult,
) -> i32 {
    if result.is_null() {
        return NEAT_ERR_ARGS;
    }
    *result = NeatCheckResult { action: NEAT_ACTION_ALLOW, redirect: ptr::null_mut(), rewritten_url: ptr::null_mut() };
    let (Some(engine), Some(url), Some(source), Some(ty)) =
        (engine.as_ref(), str_arg(url), str_arg(source_url), str_arg(request_type))
    else {
        return NEAT_ERR_ARGS;
    };
    let method = str_arg(method).unwrap_or("");

    guard(NEAT_ERR_INTERNAL, || {
        let Ok(request) = Request::new(url, source, ty, method) else { return NEAT_ERR_URL };
        let r = engine.network.check_network_request(&request);
        let blocked = r.should_block();
        let action = match (blocked, r.redirect.is_some()) {
            (true, true) => NEAT_ACTION_REDIRECT,
            (true, false) => NEAT_ACTION_BLOCK,
            (false, _) => NEAT_ACTION_ALLOW,
        };
        (*result).action = action;
        if action == NEAT_ACTION_REDIRECT {
            (*result).redirect = to_c(r.redirect.unwrap_or_default());
        }
        if action == NEAT_ACTION_ALLOW {
            if let Some(u) = r.rewritten_url {
                (*result).rewritten_url = to_c(u);
            }
        }
        NEAT_OK
    })
}

/// Popup check for WebView2's NewWindowRequested: 1 = block the new window, 0 = allow,
/// negative = error.
#[no_mangle]
pub unsafe extern "C" fn neat_check_popup(
    engine: *const NeatEngine,
    target_url: *const c_char,
    opener_url: *const c_char,
) -> i32 {
    let (Some(engine), Some(target), Some(opener)) = (engine.as_ref(), str_arg(target_url), str_arg(opener_url)) else {
        return NEAT_ERR_ARGS;
    };
    guard(NEAT_ERR_INTERNAL, || {
        let Ok(request) = Request::new(target, opener, "document", "get") else { return NEAT_ERR_URL };
        engine.popup.check_network_request(&request).should_block() as i32
    })
}

/// Page check for a top-level navigation: 1 = the page itself is blocked (a `$document` or `$all`
/// rule matched and no exception did), 0 = allow, negative = error. A plain rule such as
/// `||ads.example^` does not block a navigation, as in uBlock Origin. An older release without
/// `documents.txt` always answers 0.
#[no_mangle]
pub unsafe extern "C" fn neat_check_document(engine: *const NeatEngine, url: *const c_char) -> i32 {
    let (Some(engine), Some(url)) = (engine.as_ref(), str_arg(url)) else {
        return NEAT_ERR_ARGS;
    };
    guard(NEAT_ERR_INTERNAL, || {
        let Ok(request) = Request::new(url, url, "document", "get") else { return NEAT_ERR_URL };
        engine.document.check_network_request(&request).should_block() as i32
    })
}

/// Extra CSP directives to add to the response of a document or subdocument request.
/// Returns 1 and stores the directives in `csp_out` (release with `neat_string_free`), 0 when
/// there are none, negative on error.
#[no_mangle]
pub unsafe extern "C" fn neat_get_csp(
    engine: *const NeatEngine,
    url: *const c_char,
    source_url: *const c_char,
    request_type: *const c_char,
    csp_out: *mut *mut c_char,
) -> i32 {
    clear_out(csp_out);
    let (Some(engine), Some(url), Some(source), Some(ty)) =
        (engine.as_ref(), str_arg(url), str_arg(source_url), str_arg(request_type))
    else {
        return NEAT_ERR_ARGS;
    };
    guard(NEAT_ERR_INTERNAL, || {
        let Ok(request) = Request::new(url, source, ty, "") else { return NEAT_ERR_URL };
        match engine.network.get_csp_directives(&request) {
            Some(csp) => {
                write_out(csp_out, csp);
                1
            }
            None => 0,
        }
    })
}

/// Cosmetic resources for a page URL, as JSON:
/// `{"hide_selectors":[..],"procedural_actions":[..],"exceptions":[..],"injected_script":"..","generichide":bool}`.
/// Release the string with `neat_string_free`.
#[no_mangle]
pub unsafe extern "C" fn neat_cosmetic_resources(
    engine: *const NeatEngine,
    url: *const c_char,
    json_out: *mut *mut c_char,
) -> i32 {
    clear_out(json_out);
    let (Some(engine), Some(url)) = (engine.as_ref(), str_arg(url)) else { return NEAT_ERR_ARGS };
    guard(NEAT_ERR_INTERNAL, || {
        let resources = engine.network.url_cosmetic_resources(url);
        match serde_json::to_string(&resources) {
            Ok(s) => {
                write_out(json_out, s);
                NEAT_OK
            }
            Err(_) => NEAT_ERR_INTERNAL,
        }
    })
}

/// Generic hide selectors for the classes and ids found on a page. All three inputs are JSON
/// arrays of strings; `exceptions_json` is the `exceptions` array from `neat_cosmetic_resources`.
/// The result is a JSON array of CSS selectors. Release with `neat_string_free`.
#[no_mangle]
pub unsafe extern "C" fn neat_hidden_class_id_selectors(
    engine: *const NeatEngine,
    classes_json: *const c_char,
    ids_json: *const c_char,
    exceptions_json: *const c_char,
    json_out: *mut *mut c_char,
) -> i32 {
    clear_out(json_out);
    let (Some(engine), Some(classes), Some(ids), Some(exceptions)) =
        (engine.as_ref(), str_arg(classes_json), str_arg(ids_json), str_arg(exceptions_json))
    else {
        return NEAT_ERR_ARGS;
    };
    guard(NEAT_ERR_INTERNAL, || {
        let (Ok(classes), Ok(ids), Ok(exceptions)) = (
            serde_json::from_str::<Vec<String>>(classes),
            serde_json::from_str::<Vec<String>>(ids),
            serde_json::from_str::<HashSet<String>>(exceptions),
        ) else {
            return NEAT_ERR_ARGS;
        };
        let selectors = engine.network.hidden_class_id_selectors(classes, ids, &exceptions);
        match serde_json::to_string(&selectors) {
            Ok(s) => {
                write_out(json_out, s);
                NEAT_OK
            }
            Err(_) => NEAT_ERR_INTERNAL,
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn engine_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<NeatEngine>();
    }
}
