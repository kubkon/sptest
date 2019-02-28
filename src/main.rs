#[macro_use]
extern crate mozjs;
extern crate libc;

use mozjs::jsapi::CallArgs;
use mozjs::jsapi::CompartmentOptions;
use mozjs::jsapi::JSAutoCompartment;
use mozjs::jsapi::JSContext;
use mozjs::jsapi::JS_DefineFunction;
use mozjs::jsapi::JS_EncodeStringToUTF8;
use mozjs::jsapi::JS_NewGlobalObject;
use mozjs::jsapi::OnNewGlobalHookOption;
use mozjs::jsapi::Value;
// use mozjs::jsapi::JS_ReportErrorASCII;
use mozjs::jsval::UndefinedValue;
use mozjs::rust::{JSEngine, Runtime, SIMPLE_GLOBAL_CLASS};

use std::ffi::CStr;
use std::ptr;
use std::str;

fn main() {
    let engine =
        JSEngine::init().unwrap_or_else(|err| panic!("Error initializing JSEngine: {:?}", err));
    let runtime = Runtime::new(engine);
    let ctx = runtime.cx();
    let h_option = OnNewGlobalHookOption::FireOnNewGlobalHook;
    let c_option = CompartmentOptions::default();

    unsafe {
        let global = JS_NewGlobalObject(
            ctx,
            &SIMPLE_GLOBAL_CLASS,
            ptr::null_mut(),
            h_option,
            &c_option,
        );

        rooted!(in(ctx) let global_root = global);

        let global = global_root.handle();
        let _ac = JSAutoCompartment::new(ctx, global.get());
        let _puts_fn = JS_DefineFunction(
            ctx,
            global.into(),
            b"puts\0".as_ptr() as *const libc::c_char,
            Some(puts),
            0,
            0,
        );

        let javascript = "
            function add(x, y) {
                return x + y;
            };
            puts(add(1, 1));
        ";

        rooted!(in(ctx) let mut rval = UndefinedValue());

        runtime
            .evaluate_script(global, javascript, "test", 0, rval.handle_mut())
            .unwrap_or_else(|err| panic!("Error evaluating script: {:?}", err));
    }
}

unsafe extern "C" fn puts(ctx: *mut JSContext, argc: u32, vp: *mut Value) -> bool {
    let args = CallArgs::from_vp(vp, argc);

    let arg = mozjs::rust::Handle::from_raw(args.get(0));
    let js = mozjs::rust::ToString(ctx, arg);

    rooted!(in(ctx) let message_root = js);
    let message = JS_EncodeStringToUTF8(ctx, message_root.handle().into());
    let message = CStr::from_ptr(message);
    println!("{}", str::from_utf8(message.to_bytes()).unwrap());

    args.rval().set(UndefinedValue());
    true
}
