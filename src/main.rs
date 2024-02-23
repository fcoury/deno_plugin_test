use std::{
    env,
    rc::Rc,
    sync::{Arc, Mutex},
};

use deno_ast::{MediaType, ParseParams, SourceTextInfo};
use deno_core::{
    error::AnyError, extension, futures::FutureExt, op2, url::Url, JsRuntime, ModuleLoadResponse,
    ModuleSource, ModuleSourceCode, PollEventLoopOptions,
};
use tokio::sync::OnceCell;

struct Editor {
    buffer: String,
}

impl Editor {
    fn send(&mut self, url: String) {
        self.buffer += &url;
    }
}

static EDITOR: OnceCell<Arc<Mutex<Editor>>> = OnceCell::const_new();

struct TsModuleLoader;

impl deno_core::ModuleLoader for TsModuleLoader {
    fn resolve(
        &self,
        specifier: &str,
        referrer: &str,
        _kind: deno_core::ResolutionKind,
    ) -> Result<deno_core::ModuleSpecifier, deno_core::error::AnyError> {
        deno_core::resolve_import(specifier, referrer).map_err(|e| e.into())
    }

    fn load(
        &self,
        module_specifier: &deno_core::ModuleSpecifier,
        _maybe_referrer: Option<&deno_core::ModuleSpecifier>,
        _is_dyn_import: bool,
        _requested_module_type: deno_core::RequestedModuleType,
    ) -> ModuleLoadResponse {
        let module_specifier = module_specifier.clone();
        ModuleLoadResponse::Async(
            async move {
                let path = module_specifier.to_file_path().unwrap();

                // Determine what the MediaType is (this is done based on the file
                // extension) and whether transpiling is required.
                let media_type = MediaType::from_path(&path);
                let (module_type, should_transpile) = match MediaType::from_path(&path) {
                    MediaType::JavaScript | MediaType::Mjs | MediaType::Cjs => {
                        (deno_core::ModuleType::JavaScript, false)
                    }
                    MediaType::Jsx => (deno_core::ModuleType::JavaScript, true),
                    MediaType::TypeScript
                    | MediaType::Mts
                    | MediaType::Cts
                    | MediaType::Dts
                    | MediaType::Dmts
                    | MediaType::Dcts
                    | MediaType::Tsx => (deno_core::ModuleType::JavaScript, true),
                    MediaType::Json => (deno_core::ModuleType::Json, false),
                    _ => panic!("Unknown extension {:?}", path.extension()),
                };

                // Read the file, transpile if necessary.
                let code = std::fs::read_to_string(&path)?;
                let code = if should_transpile {
                    let parsed = deno_ast::parse_module(ParseParams {
                        specifier: module_specifier.to_string(),
                        text_info: SourceTextInfo::from_string(code),
                        media_type,
                        capture_tokens: false,
                        scope_analysis: false,
                        maybe_syntax: None,
                    })?;
                    parsed.transpile(&Default::default())?.text
                } else {
                    code
                };

                // Load and return module.
                let module = ModuleSource::new(
                    module_type,
                    ModuleSourceCode::String(code.into()),
                    &Url::parse(&module_specifier.to_string())?,
                );

                Ok(module)
            }
            .boxed_local(),
        )
    }
}

#[op2(async)]
async fn op_add_to_buffer(#[string] text: String) -> Result<(), AnyError> {
    let editor = EDITOR
        .get_or_init(|| async {
            Arc::new(Mutex::new(Editor {
                buffer: String::new(),
            }))
        })
        .await;

    editor.lock().unwrap().send(text);

    Ok(())
}

#[op2(async)]
#[string]
async fn op_get_buffer() -> Result<String, AnyError> {
    let editor = EDITOR
        .get_or_init(|| async {
            Arc::new(Mutex::new(Editor {
                buffer: String::new(),
            }))
        })
        .await;

    Ok(editor.lock().unwrap().buffer.clone())
}

extension!(
    runjs,
    ops = [op_add_to_buffer, op_get_buffer],
    js = ["src/runtime.js"],
    docs = "An extension for runjs"
);

fn main() {
    let args = &env::args().collect::<Vec<String>>()[1..];

    if args.is_empty() {
        eprintln!("Usage: runjs <file>");
        std::process::exit(1);
    }
    let file_path = &args[0];

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    if let Err(error) = runtime.block_on(run_js(file_path)) {
        eprintln!("error: {error}");
    }
}

async fn run_js(file_path: &str) -> Result<(), AnyError> {
    let main_module = deno_core::resolve_path(file_path, env::current_dir()?.as_path())?;
    let mut js_runtime = JsRuntime::new(deno_core::RuntimeOptions {
        module_loader: Some(Rc::new(TsModuleLoader)),
        extensions: vec![runjs::init_ops_and_esm()],
        ..Default::default()
    });

    let mod_id = js_runtime.load_main_module(&main_module, None).await?;
    let result = js_runtime.mod_evaluate(mod_id);
    js_runtime
        .run_event_loop(PollEventLoopOptions::default())
        .await?;
    result.await?;

    Ok(())
}
