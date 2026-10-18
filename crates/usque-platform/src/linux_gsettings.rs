//! Narrow, in-process GSettings access. Values never become child arguments.
use libloading::Library;
use std::{
    ffi::{CStr, CString, c_char, c_int, c_void},
    io, ptr,
    sync::OnceLock,
};

pub fn read(schema: &str, key: &str, user_only: bool) -> io::Result<Option<String>> {
    validate_key(schema, key)?;
    let api = api()?;
    let settings = api.settings(schema)?;
    let key = text(key)?;
    // SAFETY: settings and key are live. Both getters return an owned variant or NULL.
    let value = unsafe {
        if user_only {
            (api.get_user)(settings.pointer, key.as_ptr())
        } else {
            (api.get)(settings.pointer, key.as_ptr())
        }
    };
    if value.is_null() {
        return Ok(None);
    }
    // SAFETY: the variant is owned until unref and g_variant_print returns an owned UTF-8 string.
    let printed = unsafe { (api.print)(value, 1) };
    let result = if printed.is_null() {
        Err(error())
    } else {
        // SAFETY: printed is the NUL-terminated UTF-8 text returned by GLib.
        unsafe { CStr::from_ptr(printed) }
            .to_str()
            .map(str::to_owned)
            .map(Some)
            .map_err(|_| error())
    };
    // SAFETY: these pointers are owned outputs from GLib; g_free accepts NULL.
    unsafe {
        (api.free)(printed.cast());
        (api.variant_unref)(value);
    }
    result
}

pub fn write(schema: &str, key: &str, value: Option<&str>) -> io::Result<()> {
    validate_key(schema, key)?;
    let api = api()?;
    let settings = api.settings(schema)?;
    let key = text(key)?;
    // SAFETY: settings and key are live; this is a read-only writability query.
    if unsafe { (api.writable)(settings.pointer, key.as_ptr()) } == 0 {
        return Err(error());
    }
    if let Some(value) = value {
        let value = text(value)?;
        let mut native_error = ptr::null_mut();
        // SAFETY: value is NUL-terminated; NULL type/limit/endptr permit a complete standalone variant.
        let variant = unsafe {
            (api.parse)(
                ptr::null(),
                value.as_ptr(),
                ptr::null(),
                ptr::null_mut(),
                &mut native_error,
            )
        };
        if !native_error.is_null() {
            // SAFETY: the error output transfers one allocated GError.
            unsafe { (api.error_free)(native_error) };
        }
        if variant.is_null() {
            return Err(error());
        }
        // SAFETY: take_ref sinks a floating variant without adding a second
        // reference to a non-floating variant, so set_value cannot consume ours.
        let variant = unsafe { (api.take_ref)(variant) };
        // SAFETY: settings, key and variant are live; setting retains its own value reference.
        let success = unsafe { (api.set)(settings.pointer, key.as_ptr(), variant) };
        // SAFETY: parse returned this owned variant reference, released once.
        unsafe { (api.variant_unref)(variant) };
        if success == 0 {
            return Err(error());
        }
    } else {
        // SAFETY: settings and key remain live for resetting the user override.
        unsafe { (api.reset)(settings.pointer, key.as_ptr()) };
    }
    // SAFETY: g_settings_sync has no arguments; synchronizes this process's writes.
    unsafe { (api.sync)() };
    Ok(())
}

fn validate_key(schema: &str, key: &str) -> io::Result<()> {
    if matches!(
        (schema, key),
        ("org.gnome.system.proxy", "mode")
            | (
                "org.gnome.system.proxy.http",
                "host" | "port" | "use-authentication"
            )
            | ("org.gnome.system.proxy.https", "host" | "port")
    ) {
        Ok(())
    } else {
        Err(error())
    }
}
fn text(value: &str) -> io::Result<CString> {
    CString::new(value).map_err(|_| error())
}
fn error() -> io::Error {
    io::Error::other("GNOME proxy settings are unavailable or read-only")
}
struct Settings<'a> {
    api: &'a Api,
    pointer: *mut c_void,
}
impl Drop for Settings<'_> {
    fn drop(&mut self) {
        // SAFETY: this owns the GSettings reference returned by new_full.
        unsafe { (self.api.object_unref)(self.pointer) };
    }
}
type Getter = unsafe extern "C" fn(*mut c_void, *const c_char) -> *mut c_void;
struct Api {
    _library: Library,
    default_source: unsafe extern "C" fn() -> *mut c_void,
    lookup_schema: unsafe extern "C" fn(*mut c_void, *const c_char, c_int) -> *mut c_void,
    schema_unref: unsafe extern "C" fn(*mut c_void),
    new_full: unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_char) -> *mut c_void,
    object_unref: unsafe extern "C" fn(*mut c_void),
    get: Getter,
    get_user: Getter,
    print: unsafe extern "C" fn(*mut c_void, c_int) -> *mut c_char,
    take_ref: unsafe extern "C" fn(*mut c_void) -> *mut c_void,
    variant_unref: unsafe extern "C" fn(*mut c_void),
    free: unsafe extern "C" fn(*mut c_void),
    writable: unsafe extern "C" fn(*mut c_void, *const c_char) -> c_int,
    parse: unsafe extern "C" fn(
        *const c_void,
        *const c_char,
        *const c_char,
        *mut *const c_char,
        *mut *mut c_void,
    ) -> *mut c_void,
    set: unsafe extern "C" fn(*mut c_void, *const c_char, *mut c_void) -> c_int,
    reset: unsafe extern "C" fn(*mut c_void, *const c_char),
    sync: unsafe extern "C" fn(),
    error_free: unsafe extern "C" fn(*mut c_void),
}
fn api() -> io::Result<&'static Api> {
    static API: OnceLock<Api> = OnceLock::new();
    if let Some(api) = API.get() {
        return Ok(api);
    }
    // SAFETY: the system GIO library's documented C ABI is used below. Once
    // loaded it stays live for all registered GObject types and native calls.
    let api = unsafe {
        let library = Library::new("libgio-2.0.so.0").map_err(|_| error())?;
        macro_rules! symbol {
            ($name:literal) => {
                *library
                    .get(concat!($name, "\0").as_bytes())
                    .map_err(|_| error())?
            };
        }
        Api {
            default_source: symbol!("g_settings_schema_source_get_default"),
            lookup_schema: symbol!("g_settings_schema_source_lookup"),
            schema_unref: symbol!("g_settings_schema_unref"),
            new_full: symbol!("g_settings_new_full"),
            object_unref: symbol!("g_object_unref"),
            get: symbol!("g_settings_get_value"),
            get_user: symbol!("g_settings_get_user_value"),
            take_ref: symbol!("g_variant_take_ref"),
            print: symbol!("g_variant_print"),
            variant_unref: symbol!("g_variant_unref"),
            free: symbol!("g_free"),
            writable: symbol!("g_settings_is_writable"),
            parse: symbol!("g_variant_parse"),
            set: symbol!("g_settings_set_value"),
            reset: symbol!("g_settings_reset"),
            sync: symbol!("g_settings_sync"),
            error_free: symbol!("g_error_free"),
            _library: library,
        }
    };
    let _ = API.set(api);
    API.get().ok_or_else(error)
}
impl Api {
    fn settings(&self, schema: &str) -> io::Result<Settings<'_>> {
        // Restrict this interface to the exact non-password schemas we own.
        if !matches!(
            schema,
            "org.gnome.system.proxy"
                | "org.gnome.system.proxy.http"
                | "org.gnome.system.proxy.https"
        ) {
            return Err(error());
        }
        let schema = text(schema)?;
        // SAFETY: source is borrowed globally; schema string is valid during lookup.
        let source = unsafe { (self.default_source)() };
        if source.is_null() {
            return Err(error());
        }
        // SAFETY: non-NULL schema source and terminated name satisfy lookup's contract.
        let schema = unsafe { (self.lookup_schema)(source, schema.as_ptr(), 1) };
        if schema.is_null() {
            return Err(error());
        }
        // SAFETY: existing fixed-path schema may construct settings without custom backend/path.
        let pointer = unsafe { (self.new_full)(schema, ptr::null_mut(), ptr::null()) };
        // SAFETY: lookup transferred one schema reference, now owned by settings too.
        unsafe { (self.schema_unref)(schema) };
        if pointer.is_null() {
            return Err(error());
        }
        Ok(Settings { api: self, pointer })
    }
}
