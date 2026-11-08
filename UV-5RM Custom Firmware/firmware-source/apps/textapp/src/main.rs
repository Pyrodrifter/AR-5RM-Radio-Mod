#![no_std]
#![no_main]

use bf5rh_abi::{Api, AppEvent, AppResult};

mod logic;

// TEXT copies short records. A byte loop avoids pulling the generic, unrolled
// ARM memcpy into the 8 KiB app arena. All three EABI variants have the same
// arguments and may use this implementation, including on unaligned buffers.
#[cfg(all(target_arch = "arm", target_os = "none"))]
core::arch::global_asm!(
    ".section .text.text_memcpy,\"ax\",%progbits",
    ".thumb",
    ".global __aeabi_memcpy, __aeabi_memcpy4, __aeabi_memcpy8",
    ".thumb_func",
    "__aeabi_memcpy:",
    ".thumb_func",
    "__aeabi_memcpy4:",
    ".thumb_func",
    "__aeabi_memcpy8:",
    "cbz r2, 3f",
    "2:",
    "ldrb r3, [r1], #1",
    "strb r3, [r0], #1",
    "subs r2, r2, #1",
    "bne 2b",
    "3:",
    "bx lr",
);

static mut API_PTR: *const Api = core::ptr::null();

#[no_mangle]
pub extern "C" fn app_entry(api: &Api, ev: AppEvent) -> AppResult {
    unsafe {
        API_PTR = api as *const Api;
    }
    logic::entry(api, ev)
}

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    unsafe {
        let ptr = API_PTR;
        if !ptr.is_null() {
            ((*ptr).app_fault)(0);
        }
    }
    loop {}
}
