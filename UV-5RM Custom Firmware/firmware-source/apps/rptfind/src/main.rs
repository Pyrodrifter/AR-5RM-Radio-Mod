#![no_std]
#![no_main]

use bf5rh_abi::{Api, AppEvent, AppResult};

mod logic;

static mut API_PTR: *const Api = core::ptr::null();

#[no_mangle]
pub extern "C" fn app_entry(api: &Api, ev: AppEvent) -> AppResult {
    app_theme::load(api);
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
