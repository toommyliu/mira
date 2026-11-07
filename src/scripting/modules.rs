use std::{
    cell::RefCell,
    collections::HashMap,
    fs,
    path::{Component, Path, PathBuf},
    rc::Rc,
};

use oxc::{
    allocator::Allocator,
    codegen::{Codegen, CodegenOptions},
    parser::Parser,
    semantic::SemanticBuilder,
    span::SourceType,
    transformer::{TransformOptions, Transformer, TypeScriptOptions},
};
use rquickjs::{
    Ctx, Error, Module, Result,
    loader::{ImportAttributes, Loader, Resolver},
    module::Declared,
};

include!(concat!(env!("OUT_DIR"), "/runtime_assets.rs"));

pub(super) const INTERNAL: &str = "internal:/";
pub(super) const PRELUDE: &str = "internal:/lyra/prelude.js";
pub(super) const BOOTSTRAP: &str = "internal:/lyra/bootstrap.js";

const SCRIPT_EXTENSIONS: [&str; 4] = ["ts", "mts", "js", "mjs"];

fn asset(path: &str) -> Option<&'static str> {
    ASSETS
        .iter()
        .find(|(name, _)| *name == path)
        .map(|(_, source)| *source)
}

#[derive(Clone)]
pub(super) struct Modules(Rc<Shared>);

struct Shared {
    root: PathBuf,
    maps: RefCell<HashMap<String, LineMap>>,
}

impl Modules {
    pub(super) fn new(root: PathBuf) -> Self {
        Self(Rc::new(Shared {
            root,
            maps: RefCell::default(),
        }))
    }

    pub(super) fn entry(&self, path: &Path) -> std::result::Result<String, String> {
        self.user_path(path)
    }

    fn user_path(&self, path: &Path) -> std::result::Result<String, String> {
        let path =
            fs::canonicalize(path).map_err(|_| format!("{} doesn't exist", path.display()))?;
        if !path.starts_with(&self.0.root) {
            return Err(format!("{} is outside the script's folder", path.display()));
        }
        if !path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| SCRIPT_EXTENSIONS.contains(&extension))
        {
            return Err(format!(
                "{} isn't a .ts, .mts, .js or .mjs file",
                self.display(&path)
            ));
        }
        Ok(path.to_string_lossy().into_owned())
    }

    fn display(&self, path: &Path) -> String {
        path.strip_prefix(&self.0.root)
            .unwrap_or(path)
            .display()
            .to_string()
    }

    fn resolve_name(&self, base: &str, name: &str) -> std::result::Result<String, String> {
        if let Some(internal) = base.strip_prefix(INTERNAL) {
            return if name.starts_with("./") || name.starts_with("../") {
                let joined = normalize(
                    &Path::new(internal)
                        .parent()
                        .unwrap_or(Path::new(""))
                        .join(name),
                );
                asset(&joined)
                    .map(|_| format!("{INTERNAL}{joined}"))
                    .ok_or_else(|| format!("{name} isn't part of Lyra's runtime"))
            } else {
                package(name)
            };
        }
        if name.starts_with(INTERNAL) {
            return if base.is_empty() && asset(&name[INTERNAL.len()..]).is_some() {
                Ok(name.to_owned())
            } else {
                Err(format!("{name} can't be imported"))
            };
        }
        if name.starts_with("./") || name.starts_with("../") {
            let directory = Path::new(base).parent().unwrap_or(&self.0.root);
            return self.user_path(&directory.join(name));
        }
        if Path::new(name).is_absolute() {
            return self.user_path(Path::new(name));
        }
        package(name)
    }

    fn source(&self, name: &str) -> std::result::Result<String, String> {
        if let Some(internal) = name.strip_prefix(INTERNAL) {
            return asset(internal)
                .map(str::to_owned)
                .ok_or_else(|| format!("{name} isn't part of Lyra's runtime"));
        }
        let path = Path::new(name);
        let text = fs::read_to_string(path)
            .map_err(|error| format!("couldn't read {}: {error}", self.display(path)))?;
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("ts" | "mts") => {
                let (code, map) = strip_types(&self.display(path), &text)?;
                self.0.maps.borrow_mut().insert(name.to_owned(), map);
                Ok(code)
            }
            _ => Ok(text),
        }
    }

    pub(super) fn rewrite_stack(&self, stack: &str) -> String {
        let maps = self.0.maps.borrow();
        stack
            .lines()
            .map(|line| self.rewrite_frame(line, &maps))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn rewrite_frame(&self, line: &str, maps: &HashMap<String, LineMap>) -> String {
        let (prefix, location, suffix) = match (line.rfind('('), line.ends_with(')')) {
            (Some(open), true) => (&line[..=open], &line[open + 1..line.len() - 1], ")"),
            _ => match line.find("at ") {
                Some(at) => (&line[..at + 3], &line[at + 3..], ""),
                None => return line.to_owned(),
            },
        };
        let mut parts = location.rsplitn(3, ':');
        let (Some(column), Some(row), Some(file)) = (parts.next(), parts.next(), parts.next())
        else {
            return line.to_owned();
        };
        let (Ok(row), Ok(column)) = (row.parse::<u32>(), column.parse::<u32>()) else {
            return line.to_owned();
        };
        if file.starts_with(INTERNAL) {
            return line.to_owned();
        }
        let (row, column) = maps
            .get(file)
            .and_then(|map| map.original(row, column))
            .unwrap_or((row, column));
        format!(
            "{prefix}{}:{row}:{column}{suffix}",
            self.display(Path::new(file))
        )
    }
}

fn package(name: &str) -> std::result::Result<String, String> {
    if name == "lyra" {
        return Ok(format!("{INTERNAL}lyra/index.js"));
    }
    let path = match name.strip_prefix("effect") {
        Some("") => "effect/index.js".to_owned(),
        Some(module) if module.starts_with('/') && !module[1..].contains('/') => {
            format!("effect{module}.js")
        }
        _ => {
            return Err(format!(
                "{name} can't be imported; scripts can import lyra, effect, effect/<Module> and their own files"
            ));
        }
    };
    asset(&path)
        .map(|_| format!("{INTERNAL}{path}"))
        .ok_or_else(|| format!("{name} isn't a module of effect"))
}

fn normalize(path: &Path) -> String {
    let mut parts: Vec<&str> = Vec::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                parts.pop();
            }
            Component::Normal(part) => parts.push(part.to_str().unwrap_or_default()),
            _ => {}
        }
    }
    parts.join("/")
}

struct LineMap(Vec<[u32; 4]>);

impl LineMap {
    fn original(&self, row: u32, column: u32) -> Option<(u32, u32)> {
        let target = [row.checked_sub(1)?, column.saturating_sub(1)];
        let index = self
            .0
            .partition_point(|token| [token[0], token[1]] <= target)
            .checked_sub(1)?;
        let token = self.0[index];
        (token[0] == target[0]).then_some((token[2] + 1, token[3] + 1))
    }
}

fn strip_types(display: &str, text: &str) -> std::result::Result<(String, LineMap), String> {
    let allocator = Allocator::default();
    let source_type = SourceType::ts().with_module(true);
    let parsed = Parser::new(&allocator, text, source_type).parse();
    if let Some(diagnostic) = parsed.diagnostics.first() {
        let offset = diagnostic.labels.first().map_or(0, |label| label.offset());
        let (row, column) = position(text, offset as usize);
        return Err(format!("{display}:{row}:{column}: {}", diagnostic.message));
    }
    let mut program = parsed.program;
    let scoping = SemanticBuilder::new()
        .build(&program)
        .semantic
        .into_scoping();
    let options = TransformOptions {
        typescript: TypeScriptOptions {
            only_remove_type_imports: true,
            ..TypeScriptOptions::default()
        },
        ..TransformOptions::default()
    };
    let transformed = Transformer::new(&allocator, Path::new(display), &options)
        .build_with_scoping(scoping, &mut program);
    if let Some(diagnostic) = transformed.diagnostics.first() {
        return Err(format!("{display}: {}", diagnostic.message));
    }
    let generated = Codegen::new()
        .with_options(CodegenOptions {
            source_map_path: Some(PathBuf::from(display)),
            ..CodegenOptions::default()
        })
        .build(&program);
    let mut tokens: Vec<[u32; 4]> = generated
        .map
        .iter()
        .flat_map(|map| {
            map.get_source_view_tokens()
                .map(|token| {
                    [
                        token.get_dst_line(),
                        token.get_dst_col(),
                        token.get_src_line(),
                        token.get_src_col(),
                    ]
                })
                .collect::<Vec<_>>()
        })
        .collect();
    tokens.sort_unstable();
    Ok((generated.code, LineMap(tokens)))
}

fn position(text: &str, offset: usize) -> (usize, usize) {
    let before = &text[..offset.min(text.len())];
    let row = before.matches('\n').count() + 1;
    let column = before
        .rfind('\n')
        .map_or(before.len(), |newline| before.len() - newline - 1)
        + 1;
    (row, column)
}

pub(super) struct ModuleResolver(pub(super) Modules);

impl Resolver for ModuleResolver {
    fn resolve<'js>(
        &mut self,
        _ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> Result<String> {
        self.0
            .resolve_name(base, name)
            .map_err(|message| Error::new_resolving_message(base, name, message))
    }
}

pub(super) struct ModuleLoader(pub(super) Modules);

impl Loader for ModuleLoader {
    fn load<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> Result<Module<'js, Declared>> {
        let source = self
            .0
            .source(name)
            .map_err(|message| Error::new_loading_message(name, message))?;
        Module::declare(ctx.clone(), name, source)
    }
}
