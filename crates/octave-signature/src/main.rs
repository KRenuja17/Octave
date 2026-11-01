//! One-shot public-player transform. No account credentials or host JS APIs.
use boa_engine::{Context, JsString, JsValue, Source, js_string};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    rc::Rc,
    time::Duration,
};

const INPUT_LIMIT: u64 = 8 * 1024 * 1024;
const PLAYER_LIMIT: usize = 6 * 1024 * 1024;
const VALUE_LIMIT: usize = 8192;

fn main() {
    // The parent applies OS limits before providing any input. A second,
    // independent deadline also bounds lifetime if the parent disappears.
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(25));
        std::process::exit(70);
    });
    std::panic::set_hook(Box::new(|_| {}));
    if std::env::args().skip(1).collect::<Vec<_>>() != ["--stdio-v1"] {
        std::process::exit(64);
    }
    let result = std::thread::Builder::new()
        .name("public-player".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(run)
        .ok()
        .and_then(|thread| thread.join().ok())
        .and_then(Result::ok);
    let Some(value) = result else {
        std::process::exit(65);
    };
    if serde_json::to_writer(std::io::stdout().lock(), &value).is_err()
        || std::io::stdout().flush().is_err()
    {
        std::process::exit(74);
    }
}

fn run() -> Result<Value, ()> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .take(INPUT_LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.len() as u64 > INPUT_LIMIT {
        return Err(());
    }
    let request: Value = serde_json::from_slice(&bytes).map_err(|_| ())?;
    let object = request.as_object().ok_or(())?;
    if object
        .keys()
        .any(|key| !["player", "n", "signature"].contains(&key.as_str()))
    {
        return Err(());
    }
    let player = request["player"]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= PLAYER_LIMIT)
        .ok_or(())?;
    let n = argument(&request, "n")?;
    let signature = argument(&request, "signature")?;
    if n.is_none() && signature.is_none() {
        return Err(());
    }
    let code = octave_ejs::preprocessor::preprocess_player(player).map_err(|_| ())?;
    if code.len() > 12 * 1024 * 1024 {
        return Err(());
    }
    solve(&code, n, signature)
}

fn argument<'a>(request: &'a Value, key: &str) -> Result<Option<&'a str>, ()> {
    if request.get(key).is_none_or(Value::is_null) {
        return Ok(None);
    }
    request[key]
        .as_str()
        .filter(|s| !s.is_empty() && s.len() <= VALUE_LIMIT)
        .map(Some)
        .ok_or(())
}

fn context() -> Result<Context, ()> {
    // Boa's default SimpleModuleLoader can read files. It must never be used.
    // No fetch/require/process/console/FFI callbacks or job runner are installed.
    let mut context = Context::builder()
        .module_loader(Rc::new(boa_engine::module::IdleModuleLoader))
        .build()
        .map_err(|_| ())?;
    context
        .runtime_limits_mut()
        .set_loop_iteration_limit(1_000_000);
    context.runtime_limits_mut().set_recursion_limit(256);
    context.runtime_limits_mut().set_stack_size_limit(16_384);
    Ok(context)
}

fn solve(code: &str, n: Option<&str>, signature: Option<&str>) -> Result<Value, ()> {
    let mut context = context()?;
    let result = boa_engine::object::ObjectInitializer::new(&mut context).build();
    context
        .register_global_property(
            js_string!("_result"),
            result.clone(),
            boa_engine::property::Attribute::all(),
        )
        .map_err(|_| ())?;
    context.eval(Source::from_bytes(code)).map_err(|_| ())?;
    let mut response = json!({});
    for (name, input, output) in [("n", n, "n"), ("sig", signature, "signature")] {
        let Some(input) = input else {
            continue;
        };
        let function = result
            .get(JsString::from(name), &mut context)
            .map_err(|_| ())?
            .as_callable()
            .ok_or(())?;
        // Pass a JS value directly. Never interpolate challenge text into code.
        let value = function
            .call(
                &JsValue::undefined(),
                &[JsString::from(input).into()],
                &mut context,
            )
            .map_err(|_| ())?;
        let value = value
            .as_string()
            .ok_or(())?
            .to_std_string()
            .map_err(|_| ())?;
        if value.is_empty()
            || value.len() > VALUE_LIMIT
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-.=+/".contains(&b))
        {
            return Err(());
        }
        response[output] = value.into();
    }
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn js_inputs_are_values_and_outputs_are_strictly_bounded() {
        let code = r#"_result.n = x => x === '";globalThis.injected=true;//' ? 'safe' : 'bad'; _result.sig = x => x.split('').reverse().join('');"#;
        assert_eq!(
            solve(code, Some("\";globalThis.injected=true;//"), Some("abc")).unwrap(),
            json!({"n":"safe","signature":"cba"})
        );
        assert!(solve("_result.n = x => 'x'.repeat(9000)", Some("x"), None).is_err());
        assert!(
            solve(
                "_result.n = x => ({toString(){return 'x'}})",
                Some("x"),
                None
            )
            .is_err()
        );
    }
    #[test]
    fn host_capabilities_and_module_loading_are_absent() {
        let mut context = context().unwrap();
        let value = context.eval(Source::from_bytes("[typeof fetch, typeof require, typeof process, typeof Deno, typeof console, typeof readFile, typeof WebAssembly].every(x => x === 'undefined')")).unwrap();
        assert_eq!(value.as_boolean(), Some(true));
        context.eval(Source::from_bytes("globalThis.importError = ''; import('file:///private/session').catch(e => {globalThis.importError = e.message});")).unwrap();
        context.run_jobs().unwrap();
        let reason = context.eval(Source::from_bytes("importError")).unwrap();
        assert_eq!(
            reason.as_string().unwrap().to_std_string().unwrap(),
            "module resolution is disabled for this context"
        );
    }
    #[test]
    fn finite_engine_budget_stops_loops() {
        assert!(solve("_result.n = x => {for(;;){}}", Some("x"), None).is_err());
        assert!(
            solve(
                "_result.n = x => {function f(){return f()}return f()}",
                Some("x"),
                None
            )
            .is_err()
        );
    }
}
