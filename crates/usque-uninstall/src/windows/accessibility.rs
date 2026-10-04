//! MSAA name annotations for controls whose native provider guesses a label.
use std::{ffi::c_void, ptr};
use windows_sys::{
    Win32::{
        Foundation::HWND,
        System::Com::{
            CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
            CoUninitialize,
        },
        UI::{
            Accessibility::{CLSID_AccPropServices, PROPID_ACC_NAME},
            WindowsAndMessaging::OBJID_CLIENT,
        },
    },
    core::GUID,
};

const IID_ACC_PROP_SERVICES: GUID = GUID::from_u128(0x6e26e776_04f0_495d_80e4_3330352e3169);

pub(super) struct Apartment(bool);

impl Apartment {
    pub(super) fn enter() -> Self {
        // SAFETY: initialize only the calling UI thread; balance successful
        // initializations, including S_FALSE, when the native message pump ends.
        Self(unsafe { CoInitializeEx(ptr::null(), COINIT_APARTMENTTHREADED as u32) } >= 0)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            // SAFETY: this thread successfully entered COM through this guard.
            unsafe {
                CoUninitialize();
            }
        }
    }
}

// windows-sys exposes the class/property GUIDs but not this COM interface.
// This prefix follows IAccPropServicesVtbl in Windows SDK oleacc.h exactly:
// IUnknown, SetPropValue, SetPropServer, ClearProps, SetHwndProp, SetHwndPropStr.
#[repr(C)]
struct AccPropServices {
    vtable: *const AccPropServicesPrefix,
}

#[repr(C)]
struct AccPropServicesPrefix {
    _query_interface: *const c_void,
    _add_ref: *const c_void,
    release: unsafe extern "system" fn(*mut AccPropServices) -> u32,
    _preceding_methods: [*const c_void; 4],
    set_hwnd_prop_str:
        unsafe extern "system" fn(*mut AccPropServices, HWND, u32, u32, GUID, *const u16) -> i32,
}

pub(super) fn name(hwnd: HWND, caption: &str) {
    let mut object = ptr::null_mut::<c_void>();
    // SAFETY: the exact interface IID determines the vtable prefix above. This
    // uses Windows' in-process accessibility service on the initialized UI thread.
    let result = unsafe {
        CoCreateInstance(
            &CLSID_AccPropServices,
            ptr::null_mut(),
            CLSCTX_INPROC_SERVER,
            &IID_ACC_PROP_SERVICES,
            &mut object,
        )
    };
    if result < 0 || object.is_null() {
        return;
    }
    let service = object.cast::<AccPropServices>();
    let caption = super::wide(caption);
    // SAFETY: successful CoCreateInstance returns an owned interface with this
    // vtable. Caption stays alive during the call, GUID is passed by value as
    // required by MSAAPROPID, and Release consumes the acquired reference.
    unsafe {
        let methods = &*(*service).vtable;
        (methods.set_hwnd_prop_str)(
            service,
            hwnd,
            OBJID_CLIENT as u32,
            0,
            PROPID_ACC_NAME,
            caption.as_ptr(),
        );
        (methods.release)(service);
    }
}
