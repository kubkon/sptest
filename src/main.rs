#[macro_use]
extern crate mozjs;
extern crate libc;

use mozjs::glue::SetBuildId;
use mozjs::jsapi::BuildIdCharVector;
use mozjs::jsapi::CallArgs;
use mozjs::jsapi::CompartmentOptions;
use mozjs::jsapi::ContextOptionsRef;
use mozjs::jsapi::JSAutoCompartment;
use mozjs::jsapi::JSContext;
use mozjs::jsapi::JSObject;
use mozjs::jsapi::JS_ClearPendingException;
use mozjs::jsapi::JS_DefineFunction;
use mozjs::jsapi::JS_EncodeStringToUTF8;
use mozjs::jsapi::JS_IsExceptionPending;
use mozjs::jsapi::JS_NewGlobalObject;
use mozjs::jsapi::OnNewGlobalHookOption;
use mozjs::jsapi::SetBuildIdOp;
use mozjs::jsapi::Value;
use mozjs::jsval::ObjectValue;
use mozjs::jsval::UndefinedValue;
use mozjs::rust::wrappers::{JS_ErrorFromException, JS_GetPendingException};
use mozjs::rust::{HandleObject, JSEngine, Runtime, SIMPLE_GLOBAL_CLASS};
use mozjs::typedarray::{ArrayBuffer, CreateWith};

use std::ffi::CStr;
use std::fs::File;
use std::io::prelude::*;
use std::ptr;
use std::slice::from_raw_parts;
use std::str;

mod logger {
    struct Logger;

    impl log::Log for Logger {
        fn enabled(&self, metadata: &log::Metadata) -> bool {
            metadata.level() <= log::Level::Debug
        }

        fn log(&self, record: &log::Record) {
            if !self.enabled(record.metadata()) {
                return;
            }

            println!("{}:{}", record.level(), record.args());
        }

        fn flush(&self) {}
    }

    static LOGGER: Logger = Logger;

    pub fn init() -> Result<(), log::SetLoggerError> {
        log::set_logger(&LOGGER).map(|_| log::set_max_level(log::LevelFilter::Debug))
    }
}

fn read_js(filename: &str) -> std::io::Result<String> {
    let mut file = File::open(filename)?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    Ok(contents)
}

fn read_wasm(filename: &str) -> std::io::Result<Vec<u8>> {
    let mut file = File::open(filename)?;
    let mut contents = Vec::new();
    file.read_to_end(&mut contents)?;
    Ok(contents)
}

fn main() {
    logger::init().unwrap();

    let wasm_js = read_js("main.js").unwrap();

    let engine =
        JSEngine::init().unwrap_or_else(|err| panic!("Error initializing JSEngine: {:?}", err));
    let runtime = Runtime::new(engine);
    let ctx = runtime.cx();
    let h_option = OnNewGlobalHookOption::FireOnNewGlobalHook;
    let c_option = CompartmentOptions::default();

    unsafe {
        // runtime options
        let ctx_opts = &mut *ContextOptionsRef(ctx);
        ctx_opts.set_wasm_(true);
        ctx_opts.set_wasmBaseline_(true);
        ctx_opts.set_wasmIon_(true);
        SetBuildIdOp(ctx, Some(sp_build_id));

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
        let _readWasm_fn = JS_DefineFunction(
            ctx,
            global.into(),
            b"readWasm\0".as_ptr() as *const libc::c_char,
            Some(readWasm),
            0,
            0,
        );

        // init print funcs
        let javascript = String::from(
            "
            var Module = {'printErr': puts, 'print': puts};
            Module['wasmBinary'] = readWasm('main.wasm');
        ",
        ) + &wasm_js;

        rooted!(in(ctx) let mut rval = UndefinedValue());
        runtime
            .evaluate_script(global, &javascript, "noname", 0, rval.handle_mut())
            .unwrap_or_else(|_| {
                report_pending_exception(ctx, true);
            });
    }
}

unsafe extern "C" fn sp_build_id(build_id: *mut BuildIdCharVector) -> bool {
    let sp_id = b"SP\0";
    SetBuildId(build_id, &sp_id[0], sp_id.len())
}

unsafe extern "C" fn readWasm(ctx: *mut JSContext, argc: u32, vp: *mut Value) -> bool {
    let args = CallArgs::from_vp(vp, argc);

    let arg = mozjs::rust::Handle::from_raw(args.get(0));
    let filename = mozjs::rust::ToString(ctx, arg);

    rooted!(in(ctx) let filename_root = filename);
    let filename = JS_EncodeStringToUTF8(ctx, filename_root.handle().into());
    let filename = CStr::from_ptr(filename);

    let contents = read_wasm(str::from_utf8(filename.to_bytes()).unwrap()).unwrap();

    rooted!(in(ctx) let mut rval = ptr::null_mut::<JSObject>());
    ArrayBuffer::create(ctx, CreateWith::Slice(&contents), rval.handle_mut()).unwrap();

    args.rval().set(ObjectValue(rval.get()));
    true
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

/// A struct encapsulating information about a runtime script error.
pub struct ErrorInfo {
    /// The error message.
    pub message: String,
    /// The file name.
    pub filename: String,
    /// The line number.
    pub lineno: libc::c_uint,
    /// The column number.
    pub column: libc::c_uint,
}

impl ErrorInfo {
    unsafe fn from_native_error(cx: *mut JSContext, object: HandleObject) -> Option<ErrorInfo> {
        let report = JS_ErrorFromException(cx, object);
        if report.is_null() {
            return None;
        }

        let filename = {
            let filename = (*report)._base.filename as *const u8;
            if !filename.is_null() {
                let length = (0..).find(|idx| *filename.offset(*idx) == 0).unwrap();
                let filename = from_raw_parts(filename, length as usize);
                String::from_utf8_lossy(filename).into_owned()
            } else {
                "none".to_string()
            }
        };

        let lineno = (*report)._base.lineno;
        let column = (*report)._base.column;

        let message = {
            let message = (*report)._base.message_.data_ as *const u8;
            let length = (0..).find(|idx| *message.offset(*idx) == 0).unwrap();
            let message = from_raw_parts(message, length as usize);
            String::from_utf8_lossy(message).into_owned()
        };

        Some(ErrorInfo {
            filename: filename,
            message: message,
            lineno: lineno,
            column: column,
        })
    }
}

unsafe extern "C" fn report_pending_exception(ctx: *mut JSContext, dispatch_event: bool) {
    if !JS_IsExceptionPending(ctx) {
        return;
    }

    rooted!(in(ctx) let mut value = UndefinedValue());

    if !JS_GetPendingException(ctx, value.handle_mut()) {
        JS_ClearPendingException(ctx);
        panic!("Uncaught exception: JS_GetPendingException failed");
    }

    JS_ClearPendingException(ctx);

    if value.is_object() {
        rooted!(in(ctx) let object = value.to_object());
        let error_info =
            ErrorInfo::from_native_error(ctx, object.handle()).unwrap_or_else(|| ErrorInfo {
                message: format!("uncaught exception: unknown (can't convert to string)"),
                filename: String::new(),
                lineno: 0,
                column: 0,
            });

        eprintln!(
            "Error at {}:{}:{} {}",
            error_info.filename, error_info.lineno, error_info.column, error_info.message
        );
    } else if value.is_string() {
        rooted!(in(ctx) let object = value.to_string());
        let message = JS_EncodeStringToUTF8(ctx, object.handle().into());
        let message = std::ffi::CStr::from_ptr(message);
        eprintln!(
            "Error: {}",
            String::from_utf8_lossy(message.to_bytes()).into_owned()
        );
    } else {
        panic!("Uncaught exception: failed to stringify primitive");
    };
}
