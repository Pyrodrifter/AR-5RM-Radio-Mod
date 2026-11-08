#![no_std]
#![no_main]

use bf5rh_abi::{Api, AppEvent, AppResult};
use app_theme::pal;

const KEY_EXIT: u8 = 13;
const KIND_SINGLE: u8 = 2;

static mut API_PTR: *const Api = core::ptr::null();

static mut DRAWN: bool = false;

#[no_mangle]
pub extern "C" fn app_entry(api: &Api, ev: AppEvent) -> AppResult {
    app_theme::load(api);
    unsafe {
        API_PTR = api as *const Api;
    }

    match ev {
        AppEvent::Enter => {
            unsafe {
                DRAWN = false;
            }
            AppResult::Continue
        }
        AppEvent::Draw => {
            if unsafe { !DRAWN } {
                draw_once(api);
                unsafe {
                    DRAWN = true;
                }
            }
            AppResult::Continue
        }
        AppEvent::Key { id, kind } => {
            if id == KEY_EXIT && kind == KIND_SINGLE {
                AppResult::Exit
            } else {
                AppResult::Continue
            }
        }
        AppEvent::Tick { .. } | AppEvent::Leave => AppResult::Continue,
    }
}

fn draw_once(api: &Api) {
    (api.fill_rect)(20, 30, 40, 24, pal().accent);
    (api.draw_circle)(100, 60, 18, pal().text, false);
    (api.draw_line)(10, 100, 150, 100, pal().text);

    let text = b"HELLO OVERLAY";
    (api.draw_text)(10, 10, text.as_ptr(), text.len() as u16, pal().text, 0, 1);
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
