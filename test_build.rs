// Let's test a simplified version first to understand what's causing the issue

#[no_mangle]
pub extern "C" fn test_led_app_entry() -> ! {
    loop {}
}

#[link_section = ".apps"]
#[used]
static TEST_METADATA: u32 = 42;