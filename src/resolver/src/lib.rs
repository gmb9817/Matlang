//! Resolver crate for names, package lookups, and path resolution.

use std::{
    collections::HashSet,
    env,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

pub const CRATE_NAME: &str = "matlab-resolver";

pub fn summary() -> &'static str {
    "Owns symbol resolution, package lookup, import handling, and path search."
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolverContext {
    pub source_file: Option<PathBuf>,
    pub search_roots: Vec<PathBuf>,
}

impl ResolverContext {
    pub fn new(source_file: Option<PathBuf>, search_roots: Vec<PathBuf>) -> Self {
        Self {
            source_file,
            search_roots,
        }
    }

    pub fn from_source_file(path: impl Into<PathBuf>) -> Self {
        Self {
            source_file: Some(path.into()),
            search_roots: Vec::new(),
        }
    }

    pub fn with_env_search_roots(mut self, variable: &str) -> Self {
        self.search_roots.extend(search_roots_from_env(variable));
        self
    }

    pub fn push_search_root(&mut self, path: impl Into<PathBuf>) {
        self.search_roots.push(path.into());
    }

    pub fn source_dir(&self) -> Option<&Path> {
        self.source_file.as_deref().and_then(Path::parent)
    }

    pub fn effective_search_roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        let mut seen = HashSet::new();

        if let Some(source_dir) = self.source_dir() {
            push_unique_path(&mut roots, &mut seen, source_dir.to_path_buf());
            if let Some(enclosing_root) = enclosing_lookup_root(source_dir) {
                push_unique_path(&mut roots, &mut seen, enclosing_root);
            }
        }

        for root in &self.search_roots {
            push_unique_path(&mut roots, &mut seen, root.clone());
        }

        roots
    }
}

fn enclosing_lookup_root(source_dir: &Path) -> Option<PathBuf> {
    let mut current = source_dir.to_path_buf();
    let mut saw_special = false;

    while let Some(name) = current.file_name().and_then(|name| name.to_str()) {
        if name == "private" || name.starts_with('+') || name.starts_with('@') {
            let parent = current.parent()?.to_path_buf();
            current = parent;
            saw_special = true;
        } else {
            break;
        }
    }

    saw_special.then_some(current)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedFunctionKind {
    PrivateDirectory,
    CurrentDirectory,
    SearchPath,
    PackageDirectory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedClassKind {
    CurrentDirectory,
    SearchPath,
    PackageDirectory,
    FolderCurrentDirectory,
    FolderSearchPath,
    FolderPackageDirectory,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedFunction {
    pub name: String,
    pub kind: ResolvedFunctionKind,
    pub path: PathBuf,
    pub root: PathBuf,
    pub package: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedClass {
    pub name: String,
    pub kind: ResolvedClassKind,
    pub path: PathBuf,
    pub root: PathBuf,
    pub package: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolvedCallable {
    Function(ResolvedFunction),
    Class(ResolvedClass),
}

pub fn search_roots_from_env(variable: &str) -> Vec<PathBuf> {
    env::var_os(variable)
        .map(|value| env::split_paths(&value).collect())
        .unwrap_or_default()
}

pub fn resolve_function(name: &str, context: &ResolverContext) -> Option<ResolvedFunction> {
    if name.is_empty() {
        return None;
    }

    if let Some(source_dir) = context.source_dir() {
        if let Some(private) = resolve_private_function(name, source_dir) {
            return Some(private);
        }
    }

    for root in context.effective_search_roots() {
        if let Some(package) = resolve_package_function(name, &root) {
            return Some(package);
        }

        let candidate = root.join(format!("{name}.m"));
        if candidate.is_file() && !m_file_looks_like_classdef(&candidate) {
            let kind = if context
                .source_dir()
                .is_some_and(|dir| dir == root.as_path())
            {
                ResolvedFunctionKind::CurrentDirectory
            } else {
                ResolvedFunctionKind::SearchPath
            };
            return Some(ResolvedFunction {
                name: name.to_string(),
                kind,
                path: candidate,
                root,
                package: None,
            });
        }
    }

    None
}

pub fn resolve_callable(name: &str, context: &ResolverContext) -> Option<ResolvedCallable> {
    if name.is_empty() {
        return None;
    }

    if let Some(source_dir) = context.source_dir() {
        if let Some(private) = resolve_private_function(name, source_dir) {
            return Some(ResolvedCallable::Function(private));
        }
    }

    for root in context.effective_search_roots() {
        if let Some(class_def) = resolve_folder_class_definition(name, &root, context) {
            return Some(ResolvedCallable::Class(class_def));
        }

        if let Some(package) = resolve_package_function(name, &root) {
            return Some(ResolvedCallable::Function(package));
        }

        if let Some(class_def) = resolve_static_class_reference(name, &root, context) {
            return Some(ResolvedCallable::Class(class_def));
        }

        let candidate = root.join(format!("{name}.m"));
        if candidate.is_file() && !m_file_looks_like_classdef(&candidate) {
            let kind = if context
                .source_dir()
                .is_some_and(|dir| dir == root.as_path())
            {
                ResolvedFunctionKind::CurrentDirectory
            } else {
                ResolvedFunctionKind::SearchPath
            };
            return Some(ResolvedCallable::Function(ResolvedFunction {
                name: name.to_string(),
                kind,
                path: candidate,
                root: root.clone(),
                package: None,
            }));
        }

        if let Some(class_def) = resolve_plain_class_definition(name, &root, context) {
            return Some(ResolvedCallable::Class(class_def));
        }
    }

    None
}

pub fn resolve_all_callables(name: &str, context: &ResolverContext) -> Vec<ResolvedCallable> {
    if name.is_empty() {
        return Vec::new();
    }

    let mut matches = Vec::new();
    let mut seen = HashSet::new();

    if let Some(source_dir) = context.source_dir() {
        if let Some(private) = resolve_private_function(name, source_dir) {
            push_unique_callable_match(
                &mut matches,
                &mut seen,
                ResolvedCallable::Function(private),
            );
        }
    }

    for root in context.effective_search_roots() {
        if let Some(class_def) = resolve_folder_class_definition(name, &root, context) {
            push_unique_callable_match(&mut matches, &mut seen, ResolvedCallable::Class(class_def));
        }

        if let Some(package) = resolve_package_function(name, &root) {
            push_unique_callable_match(
                &mut matches,
                &mut seen,
                ResolvedCallable::Function(package),
            );
        }

        if let Some(class_def) = resolve_static_class_reference(name, &root, context) {
            push_unique_callable_match(&mut matches, &mut seen, ResolvedCallable::Class(class_def));
        }

        let candidate = root.join(format!("{name}.m"));
        if candidate.is_file() && !m_file_looks_like_classdef(&candidate) {
            let kind = if context
                .source_dir()
                .is_some_and(|dir| dir == root.as_path())
            {
                ResolvedFunctionKind::CurrentDirectory
            } else {
                ResolvedFunctionKind::SearchPath
            };
            push_unique_callable_match(
                &mut matches,
                &mut seen,
                ResolvedCallable::Function(ResolvedFunction {
                    name: name.to_string(),
                    kind,
                    path: candidate,
                    root: root.clone(),
                    package: None,
                }),
            );
        }

        if let Some(class_def) = resolve_plain_class_definition(name, &root, context) {
            push_unique_callable_match(&mut matches, &mut seen, ResolvedCallable::Class(class_def));
        }
    }

    matches
}

fn push_unique_callable_match(
    matches: &mut Vec<ResolvedCallable>,
    seen: &mut HashSet<PathBuf>,
    resolved: ResolvedCallable,
) {
    let path = match &resolved {
        ResolvedCallable::Function(function) => function.path.clone(),
        ResolvedCallable::Class(class_def) => class_def.path.clone(),
    };
    if seen.insert(path) {
        matches.push(resolved);
    }
}

fn resolve_static_class_reference(
    name: &str,
    root: &Path,
    context: &ResolverContext,
) -> Option<ResolvedClass> {
    let (class_name, _method_name) = name.rsplit_once('.')?;
    resolve_folder_class_definition(class_name, root, context)
        .or_else(|| resolve_plain_class_definition(class_name, root, context))
}

pub fn resolve_class_definition(name: &str, context: &ResolverContext) -> Option<ResolvedClass> {
    if name.is_empty() {
        return None;
    }

    for root in context.effective_search_roots() {
        if let Some(class_def) = resolve_folder_class_definition(name, &root, context) {
            return Some(class_def);
        }
        if let Some(class_def) = resolve_plain_class_definition(name, &root, context) {
            return Some(class_def);
        }
    }

    None
}

pub fn resolve_class_folder_method(
    class_definition_path: &Path,
    method_name: &str,
) -> Option<PathBuf> {
    let class_dir = class_definition_path.parent()?;
    let folder_name = class_dir.file_name()?.to_str()?;
    if !folder_name.starts_with('@') {
        return None;
    }
    let candidate = class_dir.join(format!("{method_name}.m"));
    candidate.is_file().then_some(candidate)
}

fn resolve_private_function(name: &str, source_dir: &Path) -> Option<ResolvedFunction> {
    if name.contains('.') {
        return None;
    }

    let candidate = source_dir.join("private").join(format!("{name}.m"));
    if !candidate.is_file() || m_file_looks_like_classdef(&candidate) {
        return None;
    }

    Some(ResolvedFunction {
        name: name.to_string(),
        kind: ResolvedFunctionKind::PrivateDirectory,
        path: candidate,
        root: source_dir.to_path_buf(),
        package: None,
    })
}

fn resolve_plain_class_definition(
    name: &str,
    root: &Path,
    context: &ResolverContext,
) -> Option<ResolvedClass> {
    if name.contains('.') {
        return resolve_package_class_definition(name, root, context);
    }

    let candidate = root.join(format!("{name}.m"));
    if !candidate.is_file() || !m_file_looks_like_classdef(&candidate) {
        return None;
    }

    Some(ResolvedClass {
        name: name.to_string(),
        kind: if context.source_dir().is_some_and(|dir| dir == root) {
            ResolvedClassKind::CurrentDirectory
        } else {
            ResolvedClassKind::SearchPath
        },
        path: candidate,
        root: root.to_path_buf(),
        package: None,
    })
}

fn resolve_folder_class_definition(
    name: &str,
    root: &Path,
    context: &ResolverContext,
) -> Option<ResolvedClass> {
    if name.contains('.') {
        return resolve_package_folder_class_definition(name, root, context);
    }

    let candidate = root.join(format!("@{name}")).join(format!("{name}.m"));
    if !candidate.is_file() {
        return None;
    }

    Some(ResolvedClass {
        name: name.to_string(),
        kind: if context.source_dir().is_some_and(|dir| dir == root) {
            ResolvedClassKind::FolderCurrentDirectory
        } else {
            ResolvedClassKind::FolderSearchPath
        },
        path: candidate,
        root: root.to_path_buf(),
        package: None,
    })
}

fn resolve_package_class_definition(
    name: &str,
    root: &Path,
    _context: &ResolverContext,
) -> Option<ResolvedClass> {
    let mut segments = name.split('.').peekable();
    let mut current = root.to_path_buf();
    let mut package_parts = Vec::new();

    while let Some(segment) = segments.next() {
        if segments.peek().is_none() {
            let candidate = current.join(format!("{segment}.m"));
            if candidate.is_file() && m_file_looks_like_classdef(&candidate) {
                return Some(ResolvedClass {
                    name: name.to_string(),
                    kind: ResolvedClassKind::PackageDirectory,
                    path: candidate,
                    root: root.to_path_buf(),
                    package: (!package_parts.is_empty()).then(|| package_parts.join(".")),
                });
            }
            return None;
        }
        package_parts.push(segment.to_string());
        current.push(format!("+{segment}"));
    }

    None
}

fn resolve_package_folder_class_definition(
    name: &str,
    root: &Path,
    context: &ResolverContext,
) -> Option<ResolvedClass> {
    let mut segments = name.split('.').peekable();
    let mut current = root.to_path_buf();
    let mut package_parts = Vec::new();

    while let Some(segment) = segments.next() {
        if segments.peek().is_none() {
            let candidate = current
                .join(format!("@{segment}"))
                .join(format!("{segment}.m"));
            if candidate.is_file() {
                return Some(ResolvedClass {
                    name: name.to_string(),
                    kind: if context.source_dir().is_some_and(|dir| dir == root) {
                        ResolvedClassKind::FolderCurrentDirectory
                    } else {
                        ResolvedClassKind::FolderPackageDirectory
                    },
                    path: candidate,
                    root: root.to_path_buf(),
                    package: (!package_parts.is_empty()).then(|| package_parts.join(".")),
                });
            }
            return None;
        }
        package_parts.push(segment.to_string());
        current.push(format!("+{segment}"));
    }

    None
}

fn resolve_package_function(name: &str, root: &Path) -> Option<ResolvedFunction> {
    if !name.contains('.') {
        return None;
    }

    let mut segments = name.split('.').peekable();
    let mut current = root.to_path_buf();
    let mut package_parts = Vec::new();

    while let Some(segment) = segments.next() {
        if segments.peek().is_none() {
            let candidate = current.join(format!("{segment}.m"));
            if candidate.is_file() && !m_file_looks_like_classdef(&candidate) {
                return Some(ResolvedFunction {
                    name: name.to_string(),
                    kind: ResolvedFunctionKind::PackageDirectory,
                    path: candidate,
                    root: root.to_path_buf(),
                    package: (!package_parts.is_empty()).then(|| package_parts.join(".")),
                });
            }
            return None;
        }

        package_parts.push(segment.to_string());
        current.push(format!("+{segment}"));
    }

    None
}

fn push_unique_path(roots: &mut Vec<PathBuf>, seen: &mut HashSet<OsString>, path: PathBuf) {
    let key = normalize_path_key(&path);
    if seen.insert(key) {
        roots.push(path);
    }
}

fn m_file_looks_like_classdef(path: &Path) -> bool {
    let Ok(source) = fs::read_to_string(path) else {
        return false;
    };
    let mut rest = source.as_str();
    loop {
        let trimmed = rest.trim_start_matches([' ', '\t', '\r', '\n']);
        if let Some(comment_rest) = trimmed.strip_prefix('%') {
            if let Some(newline) = comment_rest.find('\n') {
                rest = &comment_rest[newline + 1..];
                continue;
            }
            return false;
        }
        rest = trimmed;
        break;
    }
    rest.strip_prefix("classdef").is_some_and(|tail| {
        tail.chars()
            .next()
            .is_none_or(|ch| !ch.is_ascii_alphanumeric() && ch != '_')
    })
}

fn normalize_path_key(path: &Path) -> OsString {
    #[cfg(windows)]
    {
        OsString::from(path.to_string_lossy().to_lowercase())
    }

    #[cfg(not(windows))]
    {
        path.as_os_str().to_os_string()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        env, fs,
        path::{Path, PathBuf},
        sync::atomic::{AtomicU64, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::{
        resolve_callable, resolve_class_definition, resolve_class_folder_method, resolve_function,
        search_roots_from_env, ResolvedCallable, ResolvedClassKind, ResolvedFunctionKind,
        ResolverContext,
    };

    #[test]
    fn resolves_function_in_current_directory() {
        let workspace = temp_test_dir();
        write_file(&workspace.join("main.m"), "y = helper(1);\n");
        write_file(
            &workspace.join("helper.m"),
            "function y = helper(x)\ny = x;\nend\n",
        );

        let resolved = resolve_function(
            "helper",
            &ResolverContext::from_source_file(workspace.join("main.m")),
        )
        .expect("helper should resolve");

        assert_eq!(resolved.kind, ResolvedFunctionKind::CurrentDirectory);
        assert_eq!(resolved.path, workspace.join("helper.m"));
        cleanup(&workspace);
    }

    #[test]
    fn resolves_private_function_before_search_path() {
        let workspace = temp_test_dir();
        let search_root = workspace.join("search");
        fs::create_dir_all(workspace.join("private")).expect("create private dir");
        fs::create_dir_all(&search_root).expect("create search root");
        write_file(&workspace.join("main.m"), "y = helper(1);\n");
        write_file(
            &workspace.join("private").join("helper.m"),
            "function y = helper(x)\ny = x;\nend\n",
        );
        write_file(
            &search_root.join("helper.m"),
            "function y = helper(x)\ny = x + 1;\nend\n",
        );

        let mut context = ResolverContext::from_source_file(workspace.join("main.m"));
        context.push_search_root(&search_root);
        let resolved = resolve_function("helper", &context).expect("helper should resolve");

        assert_eq!(resolved.kind, ResolvedFunctionKind::PrivateDirectory);
        assert_eq!(resolved.path, workspace.join("private").join("helper.m"));
        cleanup(&workspace);
    }

    #[test]
    fn current_directory_function_can_shadow_builtin_name() {
        let workspace = temp_test_dir();
        write_file(&workspace.join("main.m"), "y = zeros(1, 2);\n");
        write_file(
            &workspace.join("zeros.m"),
            "function y = zeros(varargin)\ny = 1;\nend\n",
        );

        let resolved = resolve_function(
            "zeros",
            &ResolverContext::from_source_file(workspace.join("main.m")),
        )
        .expect("zeros should resolve to local file");

        assert_eq!(resolved.kind, ResolvedFunctionKind::CurrentDirectory);
        assert_eq!(resolved.path, workspace.join("zeros.m"));
        cleanup(&workspace);
    }

    #[test]
    fn search_path_order_is_stable() {
        let workspace = temp_test_dir();
        let source = workspace.join("src");
        let first = workspace.join("first");
        let second = workspace.join("second");
        fs::create_dir_all(&source).expect("create source dir");
        fs::create_dir_all(&first).expect("create first root");
        fs::create_dir_all(&second).expect("create second root");
        write_file(&source.join("main.m"), "y = helper(1);\n");
        write_file(
            &first.join("helper.m"),
            "function y = helper(x)\ny = x;\nend\n",
        );
        write_file(
            &second.join("helper.m"),
            "function y = helper(x)\ny = x + 1;\nend\n",
        );

        let context = ResolverContext::new(
            Some(source.join("main.m")),
            vec![first.clone(), second.clone()],
        );
        let resolved = resolve_function("helper", &context).expect("helper should resolve");

        assert_eq!(resolved.kind, ResolvedFunctionKind::SearchPath);
        assert_eq!(resolved.path, first.join("helper.m"));
        cleanup(&workspace);
    }

    #[test]
    fn resolves_package_function_from_search_root() {
        let workspace = temp_test_dir();
        let source = workspace.join("src");
        let root = workspace.join("packages");
        fs::create_dir_all(&source).expect("create source dir");
        fs::create_dir_all(root.join("+pkg")).expect("create package dir");
        write_file(&source.join("main.m"), "y = pkg.helper(1);\n");
        write_file(
            &root.join("+pkg").join("helper.m"),
            "function y = helper(x)\ny = x;\nend\n",
        );

        let context = ResolverContext::new(Some(source.join("main.m")), vec![root.clone()]);
        let resolved =
            resolve_function("pkg.helper", &context).expect("package helper should resolve");

        assert_eq!(resolved.kind, ResolvedFunctionKind::PackageDirectory);
        assert_eq!(resolved.package.as_deref(), Some("pkg"));
        assert_eq!(resolved.path, root.join("+pkg").join("helper.m"));
        cleanup(&workspace);
    }

    #[test]
    fn resolves_plain_class_definition_in_current_directory() {
        let workspace = temp_test_dir();
        write_file(&workspace.join("main.m"), "p = Point();\n");
        write_file(&workspace.join("Point.m"), "classdef Point\nend\n");

        let resolved = resolve_class_definition(
            "Point",
            &ResolverContext::from_source_file(workspace.join("main.m")),
        )
        .expect("Point class should resolve");

        assert_eq!(resolved.kind, ResolvedClassKind::CurrentDirectory);
        assert_eq!(resolved.path, workspace.join("Point.m"));
        cleanup(&workspace);
    }

    #[test]
    fn folder_class_definition_outranks_plain_function_in_callable_resolution() {
        let workspace = temp_test_dir();
        fs::create_dir_all(workspace.join("@Point")).expect("create class dir");
        write_file(&workspace.join("main.m"), "p = Point();\n");
        write_file(
            &workspace.join("Point.m"),
            "function y = Point()\ny = 1;\nend\n",
        );
        write_file(
            &workspace.join("@Point").join("Point.m"),
            "classdef Point\nend\n",
        );

        let resolved = resolve_callable(
            "Point",
            &ResolverContext::from_source_file(workspace.join("main.m")),
        )
        .expect("callable should resolve");

        let ResolvedCallable::Class(resolved) = resolved else {
            panic!("expected class resolution");
        };
        assert_eq!(resolved.kind, ResolvedClassKind::FolderCurrentDirectory);
        assert_eq!(resolved.path, workspace.join("@Point").join("Point.m"));
        cleanup(&workspace);
    }

    #[test]
    fn resolves_package_class_definition_from_search_root() {
        let workspace = temp_test_dir();
        let source = workspace.join("src");
        let root = workspace.join("packages");
        fs::create_dir_all(&source).expect("create source dir");
        fs::create_dir_all(root.join("+pkg")).expect("create package dir");
        write_file(&source.join("main.m"), "obj = pkg.Point();\n");
        write_file(&root.join("+pkg").join("Point.m"), "classdef Point\nend\n");

        let resolved = resolve_class_definition(
            "pkg.Point",
            &ResolverContext::new(Some(source.join("main.m")), vec![root.clone()]),
        )
        .expect("package class should resolve");

        assert_eq!(resolved.kind, ResolvedClassKind::PackageDirectory);
        assert_eq!(resolved.package.as_deref(), Some("pkg"));
        assert_eq!(resolved.path, root.join("+pkg").join("Point.m"));
        cleanup(&workspace);
    }

    #[test]
    fn resolves_package_class_definition_from_inside_package_directory() {
        let workspace = temp_test_dir();
        let package_dir = workspace.join("+pkg");
        fs::create_dir_all(&package_dir).expect("create package dir");
        let child_path = package_dir.join("Child.m");
        write_file(&child_path, "classdef Child < pkg.Base\nend\n");
        write_file(&package_dir.join("Base.m"), "classdef Base\nend\n");

        let resolved =
            resolve_class_definition("pkg.Base", &ResolverContext::from_source_file(child_path))
                .expect("package class should resolve from inside package");

        assert_eq!(resolved.kind, ResolvedClassKind::PackageDirectory);
        assert_eq!(resolved.package.as_deref(), Some("pkg"));
        assert_eq!(resolved.path, package_dir.join("Base.m"));
        cleanup(&workspace);
    }

    #[test]
    fn resolves_package_folder_class_definition_from_search_root() {
        let workspace = temp_test_dir();
        let source = workspace.join("src");
        let root = workspace.join("packages");
        fs::create_dir_all(&source).expect("create source dir");
        fs::create_dir_all(root.join("+pkg").join("@Counter"))
            .expect("create package folder class dir");
        write_file(&source.join("main.m"), "obj = pkg.Counter();\n");
        write_file(
            &root.join("+pkg").join("@Counter").join("Counter.m"),
            "classdef Counter\nend\n",
        );

        let resolved = resolve_class_definition(
            "pkg.Counter",
            &ResolverContext::new(Some(source.join("main.m")), vec![root.clone()]),
        )
        .expect("package folder class should resolve");

        assert_eq!(resolved.kind, ResolvedClassKind::FolderPackageDirectory);
        assert_eq!(resolved.package.as_deref(), Some("pkg"));
        assert_eq!(
            resolved.path,
            root.join("+pkg").join("@Counter").join("Counter.m")
        );
        cleanup(&workspace);
    }

    #[test]
    fn resolves_class_folder_method_from_definition_path() {
        let workspace = temp_test_dir();
        fs::create_dir_all(workspace.join("@Counter")).expect("create class dir");
        let class_path = workspace.join("@Counter").join("Counter.m");
        let method_path = workspace.join("@Counter").join("increment.m");
        write_file(&class_path, "classdef Counter < handle\nend\n");
        write_file(
            &method_path,
            "function obj = increment(obj, delta)\nobj.value = obj.value + delta;\nend\n",
        );

        let resolved =
            resolve_class_folder_method(&class_path, "increment").expect("increment method");
        assert_eq!(resolved, method_path);
        cleanup(&workspace);
    }

    #[test]
    fn returns_none_when_function_is_missing() {
        let workspace = temp_test_dir();
        write_file(&workspace.join("main.m"), "y = helper(1);\n");

        let resolved = resolve_function(
            "helper",
            &ResolverContext::from_source_file(workspace.join("main.m")),
        );

        assert!(resolved.is_none());
        cleanup(&workspace);
    }

    #[test]
    fn reads_search_roots_from_env_var() {
        let workspace = temp_test_dir();
        let first = workspace.join("first");
        let second = workspace.join("second");
        fs::create_dir_all(&first).expect("create first root");
        fs::create_dir_all(&second).expect("create second root");
        let joined = env::join_paths([first.as_path(), second.as_path()]).expect("join paths");
        env::set_var("MATC_TEST_PATH", joined);

        let roots = search_roots_from_env("MATC_TEST_PATH");
        assert_eq!(roots, vec![first.clone(), second.clone()]);

        env::remove_var("MATC_TEST_PATH");
        cleanup(&workspace);
    }

    static NEXT_ID: AtomicU64 = AtomicU64::new(0);

    fn temp_test_dir() -> PathBuf {
        let mut path = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("unix time")
            .as_nanos();
        path.push(format!(
            "matlab_resolver_test_{}_{}",
            nanos,
            NEXT_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).expect("create temp test dir");
        path
    }

    fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("create parent dir");
        }
        fs::write(path, contents).expect("write test file");
    }

    fn cleanup(path: &Path) {
        let _ = fs::remove_dir_all(path);
    }
}
