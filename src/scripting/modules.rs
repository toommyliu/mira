use std::{
    cell::{OnceCell, RefCell},
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
    Ctx, Exception, Module, Result,
    loader::{ImportAttributes, Loader, Resolver},
    module::Declared,
};

use super::inputs::position;
use crate::packages::{Manifest, ManifestError};

include!(concat!(env!("OUT_DIR"), "/runtime_assets.rs"));

pub(super) const INTERNAL: &str = "internal:/";
pub(super) const PRELUDE: &str = "internal:/mira/prelude.js";
pub(super) const BOOTSTRAP: &str = "internal:/mira/bootstrap.js";

pub(crate) const SCRIPT_EXTENSIONS: [&str; 4] = ["ts", "mts", "js", "mjs"];

pub(crate) fn runnable(name: &str) -> bool {
    let Some((stem, extension)) = name.rsplit_once('.') else {
        return false;
    };
    SCRIPT_EXTENSIONS.contains(&extension) && !stem.ends_with(".d")
}

fn asset(path: &str) -> Option<&'static str> {
    ASSETS
        .iter()
        .find(|(name, _)| *name == path)
        .map(|(_, source)| *source)
}

#[derive(Clone)]
pub(super) struct Modules(Rc<Shared>);

struct Shared {
    script: Rc<Root>,
    packages: PathBuf,
    installed: OnceCell<Vec<Installed>>,
    roots: RefCell<HashMap<String, Rc<Root>>>,
    entry: OnceCell<(String, String)>,
    maps: RefCell<HashMap<String, LineMap>>,
}

struct Root {
    path: PathBuf,
    package: Option<String>,
}

impl Root {
    fn confine(&self, path: &Path) -> std::result::Result<PathBuf, &'static str> {
        let path = fs::canonicalize(path).map_err(|_| "doesn't exist")?;
        if !path.starts_with(&self.path) {
            return Err(match self.package {
                Some(_) => "is outside the package's folder",
                None => "is outside the script's folder",
            });
        }
        if !path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| SCRIPT_EXTENSIONS.contains(&extension))
        {
            return Err("isn't a .ts, .mts, .js or .mjs file");
        }
        Ok(path)
    }

    fn display(&self, path: &Path) -> String {
        let relative = path.strip_prefix(&self.path).unwrap_or(path).display();
        match &self.package {
            Some(name) => format!("{name}/{relative}"),
            None => relative.to_string(),
        }
    }
}

struct Installed {
    dir: String,
    root: PathBuf,
    name: String,
    manifest: std::result::Result<Manifest, ManifestError>,
}

impl Installed {
    fn main(&self) -> std::result::Result<(Root, PathBuf), String> {
        let manifest = self.manifest.as_ref().map_err(ToString::to_string)?;
        let Some(main) = &manifest.main else {
            return Err("the package has no \"main\" in its package.json".into());
        };
        let root = Root {
            path: fs::canonicalize(&self.root)
                .map_err(|error| format!("couldn't read the package: {error}"))?,
            package: Some(self.name.clone()),
        };
        let path = root
            .confine(&self.root.join(main))
            .map_err(|problem| format!("its main, {main}, {problem}"))?;
        Ok((root, path))
    }
}

fn scan(packages: &Path) -> Vec<Installed> {
    let Ok(items) = fs::read_dir(packages) else {
        return Vec::new();
    };
    let mut installed: Vec<Installed> = items
        .flatten()
        .filter_map(|item| {
            let root = item.path();
            let bytes = fs::read(root.join("package.json")).ok()?;
            let manifest = Manifest::parse(&bytes);
            let name = match &manifest {
                Ok(manifest) => manifest.name.clone(),
                Err(ManifestError::NotAPackage) => return None,
                Err(_) => serde_json::from_slice::<serde_json::Value>(&bytes)
                    .ok()?
                    .get("name")?
                    .as_str()?
                    .trim()
                    .to_owned(),
            };
            Some(Installed {
                dir: item.file_name().to_string_lossy().into_owned(),
                root,
                name,
                manifest,
            })
        })
        .collect();
    installed.sort_by(|a, b| a.dir.cmp(&b.dir));
    installed
}

fn find<'a>(installed: &'a [Installed], name: &str) -> std::result::Result<&'a Installed, String> {
    let named: Vec<&Installed> = installed
        .iter()
        .filter(|package| package.name == name)
        .collect();
    match named[..] {
        [] => Err("no installed package has that name; scripts can import mira, mira/http, mira/schema, effect, effect/<Module>, installed packages and their own files".into()),
        [package] => Ok(package),
        _ => {
            let dirs: Vec<&str> = named.iter().map(|package| package.dir.as_str()).collect();
            Err(format!(
                "more than one installed package has that name: {}",
                dirs.join(", ")
            ))
        }
    }
}

pub(super) fn importable(packages: &Path) -> Vec<(String, PathBuf)> {
    let installed = scan(packages);
    installed
        .iter()
        .filter(|package| builtin(&package.name).is_none())
        .filter_map(|package| {
            let (_, main) = find(&installed, &package.name).ok()?.main().ok()?;
            Some((package.name.clone(), main))
        })
        .collect()
}

impl Modules {
    pub(super) fn for_entry(entry: &Path, packages: &Path) -> std::result::Result<Self, String> {
        let folder = entry
            .parent()
            .and_then(|folder| fs::canonicalize(folder).ok())
            .ok_or_else(|| format!("{} doesn't exist", entry.display()))?;
        let root = package_root(&folder)?.unwrap_or(folder);
        Ok(Self(Rc::new(Shared {
            script: Rc::new(Root {
                path: root,
                package: None,
            }),
            packages: packages.to_owned(),
            installed: OnceCell::new(),
            roots: RefCell::default(),
            entry: OnceCell::new(),
            maps: RefCell::default(),
        })))
    }

    pub(super) fn entry(&self, path: &Path) -> std::result::Result<String, String> {
        let root = &self.0.script;
        root.confine(path)
            .map(|path| path.to_string_lossy().into_owned())
            .map_err(|problem| format!("{} {problem}", root.display(path)))
    }

    pub(super) fn retain_entry(&self, name: String, text: String) {
        let _ = self.0.entry.set((name, text));
    }

    fn root_of(&self, name: &str) -> Rc<Root> {
        self.0
            .roots
            .borrow()
            .get(name)
            .cloned()
            .unwrap_or_else(|| self.0.script.clone())
    }

    pub(super) fn display(&self, name: &str) -> String {
        self.root_of(name).display(Path::new(name))
    }

    fn user_path(&self, root: Rc<Root>, path: &Path) -> std::result::Result<String, String> {
        let path = root
            .confine(path)
            .map_err(|problem| format!("it {problem}"))?
            .to_string_lossy()
            .into_owned();
        self.0
            .roots
            .borrow_mut()
            .entry(path.clone())
            .or_insert(root);
        Ok(path)
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
                    .ok_or_else(|| "it isn't part of Mira's runtime".into())
            } else {
                builtin(name).unwrap_or_else(|| Err("it isn't part of Mira's runtime".into()))
            };
        }
        if name.starts_with(INTERNAL) {
            return if base.is_empty() && asset(&name[INTERNAL.len()..]).is_some() {
                Ok(name.to_owned())
            } else {
                Err("it's internal to Mira".into())
            };
        }
        let root = self.root_of(base);
        if name.starts_with("./") || name.starts_with("../") {
            let directory = Path::new(base).parent().unwrap_or(&root.path);
            let path = directory.join(name);
            return self.user_path(root, &path);
        }
        if Path::new(name).is_absolute() {
            return self.user_path(root, Path::new(name));
        }
        if let Some(builtin) = builtin(name) {
            return builtin;
        }
        let installed = self.0.installed.get_or_init(|| scan(&self.0.packages));
        let (root, main) = find(installed, name)?.main()?;
        let main = main.to_string_lossy().into_owned();
        self.0
            .roots
            .borrow_mut()
            .entry(main.clone())
            .or_insert_with(|| Rc::new(root));
        Ok(main)
    }

    fn import_error(&self, base: &str, name: &str, reason: &str) -> String {
        format!("Can't import {name} from {}: {reason}", self.display(base))
    }

    fn source(&self, name: &str) -> std::result::Result<String, String> {
        if let Some(internal) = name.strip_prefix(INTERNAL) {
            return asset(internal)
                .map(str::to_owned)
                .ok_or_else(|| format!("{name} isn't part of Mira's runtime"));
        }
        let path = Path::new(name);
        let display = self.display(name);
        let text = match self.0.entry.get() {
            Some((entry, text)) if entry == name => text.clone(),
            _ => fs::read_to_string(path)
                .map_err(|error| format!("couldn't read {display}: {error}"))?,
        };
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("ts" | "mts") => {
                let (code, map) = strip_types(&display, &text)?;
                self.0.maps.borrow_mut().insert(name.to_owned(), map);
                Ok(code)
            }
            _ => Ok(text),
        }
    }

    pub(super) fn rewrite_stack(&self, stack: &str) -> String {
        let maps = self.0.maps.borrow();
        let own: Vec<String> = stack
            .lines()
            .filter_map(|line| self.own_frame(line, &maps))
            .collect();
        if own.is_empty() {
            stack.to_owned()
        } else {
            own.join("\n")
        }
    }

    fn own_frame(&self, line: &str, maps: &HashMap<String, LineMap>) -> Option<String> {
        let (prefix, location, suffix) = match (line.rfind('('), line.ends_with(')')) {
            (Some(open), true) => (&line[..=open], &line[open + 1..line.len() - 1], ")"),
            _ => {
                let at = line.find("at ")?;
                (&line[..at + 3], &line[at + 3..], "")
            }
        };
        let mut parts = location.rsplitn(3, ':');
        let (Some(column), Some(row), Some(file)) = (parts.next(), parts.next(), parts.next())
        else {
            return None;
        };
        let (Ok(row), Ok(column)) = (row.parse::<u32>(), column.parse::<u32>()) else {
            return None;
        };
        if file.starts_with(INTERNAL) {
            return None;
        }
        let (row, column) = maps
            .get(file)
            .and_then(|map| map.original(row, column))
            .unwrap_or((row, column));
        Some(format!("{prefix}{file}:{row}:{column}{suffix}"))
    }
}

fn package_root(folder: &Path) -> std::result::Result<Option<PathBuf>, String> {
    for scripts in folder
        .ancestors()
        .filter(|dir| dir.file_name() == Some("scripts".as_ref()))
    {
        let Some(root) = scripts.parent() else {
            continue;
        };
        let Ok(bytes) = fs::read(root.join("package.json")) else {
            continue;
        };
        match Manifest::parse(&bytes) {
            Ok(_) => return Ok(Some(root.to_owned())),
            Err(ManifestError::NotAPackage) => {}
            Err(error) => return Err(error.to_string()),
        }
    }
    Ok(None)
}

fn builtin(name: &str) -> Option<std::result::Result<String, String>> {
    let path = match name {
        "mira" => "mira/index.js".to_owned(),
        "mira/http" | "mira/schema" => format!("{name}.js"),
        "effect" => "effect/index.js".to_owned(),
        _ => match name.strip_prefix("effect/") {
            Some(module) if !module.contains('/') => format!("effect/{module}.js"),
            Some(_) => return Some(Err("it isn't a module of effect".into())),
            None => return None,
        },
    };
    Some(
        asset(&path)
            .map(|_| format!("{INTERNAL}{path}"))
            .ok_or_else(|| "it isn't a module of effect".into()),
    )
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

pub(super) struct ModuleResolver(pub(super) Modules);

impl Resolver for ModuleResolver {
    fn resolve<'js>(
        &mut self,
        ctx: &Ctx<'js>,
        base: &str,
        name: &str,
        _attributes: Option<ImportAttributes<'js>>,
    ) -> Result<String> {
        self.0.resolve_name(base, name).map_err(|reason| {
            Exception::throw_message(ctx, &self.0.import_error(base, name, &reason))
        })
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
            .map_err(|message| Exception::throw_message(ctx, &message))?;
        Module::declare(ctx.clone(), name, source)
    }
}
