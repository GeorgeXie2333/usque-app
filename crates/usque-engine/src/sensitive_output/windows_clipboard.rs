use std::{io, ptr};

use windows_sys::Win32::Foundation::{GlobalFree, HGLOBAL, HWND};
use windows_sys::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows_sys::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, HWND_MESSAGE};
use zeroize::{Zeroize, Zeroizing};

pub(super) fn copy(value: &[u8]) -> io::Result<()> {
    let text = std::str::from_utf8(value)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "secret is not UTF-8"))?;
    let wide = Zeroizing::new(text.encode_utf16().chain([0]).collect::<Vec<u16>>());
    write_clipboard(&mut NativeClipboard, &wide)
}

// This synchronous boundary keeps the owner window, open clipboard and movable
// allocation on one OS thread. Tests exercise failures without using a clipboard.
trait ClipboardApi {
    type Owner: Copy;
    type Buffer: Copy;
    fn allocate(&mut self, wide: &[u16]) -> io::Result<Self::Buffer>;
    fn create_owner(&mut self) -> io::Result<Self::Owner>;
    fn open(&mut self, owner: Self::Owner) -> io::Result<()>;
    fn empty(&mut self) -> io::Result<()>;
    fn set(&mut self, buffer: Self::Buffer) -> io::Result<()>;
    fn close(&mut self) -> io::Result<()>;
    fn free(&mut self, buffer: Self::Buffer);
    fn destroy_owner(&mut self, owner: Self::Owner) -> io::Result<()>;
}

fn write_clipboard(api: &mut impl ClipboardApi, wide: &[u16]) -> io::Result<()> {
    // Allocation and conversion must succeed before EmptyClipboard can discard
    // the user's previous clipboard contents.
    let buffer = api.allocate(wide)?;
    let owner = match api.create_owner() {
        Ok(owner) => owner,
        Err(error) => {
            api.free(buffer);
            return Err(error);
        }
    };
    let mut opened = false;
    let mut transferred = false;
    let result = (|| {
        api.open(owner)?;
        opened = true;
        api.empty()?;
        api.set(buffer)?;
        transferred = true;
        Ok(())
    })();
    let closed = if opened { api.close() } else { Ok(()) };
    if !transferred {
        api.free(buffer);
    }
    let destroyed = api.destroy_owner(owner);
    // Cleanup always runs, and a secondary cleanup error never masks the first.
    result.and(closed).and(destroyed)
}

struct NativeClipboard;

#[derive(Clone, Copy)]
struct ClipboardBuffer {
    handle: HGLOBAL,
    bytes: usize,
}

fn checked(result: i32) -> io::Result<()> {
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

impl ClipboardApi for NativeClipboard {
    type Owner = HWND;
    type Buffer = ClipboardBuffer;

    fn allocate(&mut self, wide: &[u16]) -> io::Result<ClipboardBuffer> {
        let bytes = size_of_val(wide);
        // SAFETY: the live UTF-16 slice includes a terminating zero. The new
        // movable allocation has exactly `bytes` writable bytes and is locked
        // while copying. A failed lock releases the uninitialized allocation.
        unsafe {
            let handle = GlobalAlloc(GMEM_MOVEABLE, bytes);
            if handle.is_null() {
                return Err(io::Error::last_os_error());
            }
            let target = GlobalLock(handle);
            if target.is_null() {
                let error = io::Error::last_os_error();
                let _ = GlobalFree(handle);
                return Err(error);
            }
            ptr::copy_nonoverlapping(wide.as_ptr().cast::<u8>(), target.cast::<u8>(), bytes);
            let _ = GlobalUnlock(handle);
            Ok(ClipboardBuffer { handle, bytes })
        }
    }

    fn create_owner(&mut self) -> io::Result<HWND> {
        const CLASS: [u16; 7] = [83, 84, 65, 84, 73, 67, 0]; // Predefined STATIC class.
        // SAFETY: CLASS is zero-terminated, no application data is passed, and
        // HWND_MESSAGE creates an invisible window on this calling thread.
        // write_clipboard retains it until after CloseClipboard and destroys
        // it synchronously on the same thread. No secret becomes window text.
        unsafe {
            let instance = GetModuleHandleW(ptr::null());
            if instance.is_null() {
                return Err(io::Error::last_os_error());
            }
            let owner = CreateWindowExW(
                0,
                CLASS.as_ptr(),
                ptr::null(),
                0,
                0,
                0,
                0,
                0,
                HWND_MESSAGE,
                ptr::null_mut(),
                instance,
                ptr::null(),
            );
            if owner.is_null() {
                Err(io::Error::last_os_error())
            } else {
                Ok(owner)
            }
        }
    }

    fn open(&mut self, owner: HWND) -> io::Result<()> {
        // SAFETY: owner is the still-live window created on this thread above.
        unsafe { checked(OpenClipboard(owner)) }
    }

    fn empty(&mut self) -> io::Result<()> {
        // SAFETY: write_clipboard calls this only after a successful OpenClipboard.
        unsafe { checked(EmptyClipboard()) }
    }

    fn set(&mut self, buffer: ClipboardBuffer) -> io::Result<()> {
        const CF_UNICODETEXT: u32 = 13;
        // SAFETY: the clipboard is open with a valid owner. buffer is an
        // unlocked GMEM_MOVEABLE allocation containing terminated UTF-16.
        // Only success transfers ownership; failure is freed by the caller.
        unsafe {
            if SetClipboardData(CF_UNICODETEXT, buffer.handle).is_null() {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        }
    }

    fn close(&mut self) -> io::Result<()> {
        // SAFETY: exactly one close follows each successful open on this thread.
        unsafe { checked(CloseClipboard()) }
    }

    fn free(&mut self, buffer: ClipboardBuffer) {
        // SAFETY: this is called only before successful ownership transfer.
        // The allocation is still ours; wipe all bytes while locked, then free.
        unsafe {
            let target = GlobalLock(buffer.handle);
            if !target.is_null() {
                std::slice::from_raw_parts_mut(target.cast::<u8>(), buffer.bytes).zeroize();
                let _ = GlobalUnlock(buffer.handle);
            }
            let _ = GlobalFree(buffer.handle);
        }
    }

    fn destroy_owner(&mut self, owner: HWND) -> io::Result<()> {
        // SAFETY: the calling thread created and uniquely owns this window;
        // clipboard closing has already been attempted before destruction.
        unsafe { checked(DestroyWindow(owner)) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeClipboard {
        fail: Option<&'static str>,
        fail_cleanup: bool,
        calls: Vec<&'static str>,
    }
    impl FakeClipboard {
        fn step(&mut self, name: &'static str) -> io::Result<()> {
            self.calls.push(name);
            if self.fail == Some(name) || self.fail_cleanup && matches!(name, "close" | "destroy") {
                Err(io::Error::other(name))
            } else {
                Ok(())
            }
        }
    }
    impl ClipboardApi for FakeClipboard {
        type Owner = usize;
        type Buffer = usize;
        fn allocate(&mut self, wide: &[u16]) -> io::Result<usize> {
            assert_eq!(wide.last(), Some(&0));
            self.step("allocate")?;
            Ok(99)
        }
        fn create_owner(&mut self) -> io::Result<usize> {
            self.step("owner")?;
            Ok(42)
        }
        fn open(&mut self, owner: usize) -> io::Result<()> {
            assert_eq!(owner, 42);
            self.step("open")
        }
        fn empty(&mut self) -> io::Result<()> {
            self.step("empty")
        }
        fn set(&mut self, buffer: usize) -> io::Result<()> {
            assert_eq!(buffer, 99);
            self.step("set")
        }
        fn close(&mut self) -> io::Result<()> {
            self.step("close")
        }
        fn free(&mut self, buffer: usize) {
            assert_eq!(buffer, 99);
            self.calls.push("free");
        }
        fn destroy_owner(&mut self, owner: usize) -> io::Result<()> {
            assert_eq!(owner, 42);
            self.step("destroy")
        }
    }

    #[test]
    fn every_failure_preserves_handle_ownership_and_cleans_up_once() {
        for fail in [
            None,
            Some("allocate"),
            Some("owner"),
            Some("open"),
            Some("empty"),
            Some("set"),
            Some("close"),
            Some("destroy"),
        ] {
            let mut api = FakeClipboard {
                fail,
                ..Default::default()
            };
            let result = write_clipboard(&mut api, &[65, 0]);
            assert_eq!(result.is_ok(), fail.is_none());
            let expected: &[&str] = match fail {
                Some("allocate") => &["allocate"],
                Some("owner") => &["allocate", "owner", "free"],
                Some("open") => &["allocate", "owner", "open", "free", "destroy"],
                Some("empty") => &[
                    "allocate", "owner", "open", "empty", "close", "free", "destroy",
                ],
                Some("set") => &[
                    "allocate", "owner", "open", "empty", "set", "close", "free", "destroy",
                ],
                _ => &[
                    "allocate", "owner", "open", "empty", "set", "close", "destroy",
                ],
            };
            assert_eq!(api.calls, expected);
        }
        let mut api = FakeClipboard {
            fail: Some("set"),
            fail_cleanup: true,
            ..Default::default()
        };
        assert_eq!(
            write_clipboard(&mut api, &[65, 0]).unwrap_err().to_string(),
            "set"
        );
        assert_eq!(&api.calls[5..], ["close", "free", "destroy"]);
    }

    #[test]
    fn native_message_only_owner_can_be_created_without_touching_the_clipboard() {
        let mut api = NativeClipboard;
        let owner = api.create_owner().unwrap();
        // SAFETY: the private window is still alive on the thread that created it.
        let valid = unsafe { windows_sys::Win32::UI::WindowsAndMessaging::IsWindow(owner) };
        api.destroy_owner(owner).unwrap();
        assert_ne!(valid, 0);
        assert_eq!(
            copy(&[0xff]).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
