use std::collections::HashMap;
use std::fs;
use std::path::Path;

use proc_macro2::{Span, TokenTree};
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{Expr, Item, Lit, Stmt, UseTree};

#[path = "filesystem_paths/rules.rs"]
mod rules;

use rules::{
    allowed, chooses_system_directory, directory_key, filesystem_arguments,
    imports_directory_api_glob, reads_environment,
};

const SOURCE_ROOTS: [&str; 3] = ["apps", "platforms", "tools"];
const STORAGE: &str = "apps/core/storage/src/";

#[test]
fn launcher_paths_are_not_rebuilt_by_consumers() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut violations = Vec::new();
    let mut scanned = 0;
    for source in SOURCE_ROOTS {
        scan_tree(root, &root.join(source), &mut scanned, &mut violations);
    }
    assert!(scanned > 0, "no Rust sources scanned");
    assert!(
        violations.is_empty(),
        "Use axlauncher_storage for managed paths (see .agents/docs/filesystem-paths.md):\n{}",
        violations.join("\n")
    );
}

fn scan_tree(root: &Path, directory: &Path, scanned: &mut usize, violations: &mut Vec<String>) {
    let entries = fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("cannot scan {}: {error}", directory.display()));
    for entry in entries {
        let entry = entry.expect("read source directory entry");
        let kind = entry.file_type().expect("read source file type");
        let path = entry.path();
        if ["node_modules", "target", ".git"]
            .iter()
            .any(|name| entry.file_name() == *name)
        {
            continue;
        }
        if kind.is_dir() {
            scan_tree(root, &path, scanned, violations);
        }
        if !kind.is_file() || !path.extension().is_some_and(|extension| extension == "rs") {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if !relative.starts_with(STORAGE) {
            *scanned += 1;
            violations.extend(inspect(
                &relative,
                &fs::read_to_string(&path).expect("read Rust source"),
            ));
        }
    }
}

fn inspect(path: &str, source: &str) -> Vec<String> {
    let file = syn::parse_file(source).unwrap_or_else(|error| panic!("{path}: {error}"));
    let mut scanner = Scanner {
        path,
        scopes: vec![Scope::new(file.items.iter())],
        function: None,
        violations: Vec::new(),
    };
    scanner.visit_file(&file);
    scanner.violations
}

#[derive(Default)]
struct Scope<'ast> {
    values: HashMap<String, &'ast Expr>,
    aliases: HashMap<String, Vec<String>>,
}

impl<'ast> Scope<'ast> {
    fn new(items: impl Iterator<Item = &'ast Item>) -> Self {
        let mut scope = Self::default();
        for item in items {
            scope.add_item(item);
        }
        scope
    }

    fn add_item(&mut self, item: &'ast Item) {
        let (name, value) = match item {
            Item::Const(item) => (&item.ident, &item.expr),
            Item::Static(item) => (&item.ident, &item.expr),
            Item::Use(item) => return self.add_use(&item.tree, &[]),
            _ => return,
        };
        self.values.insert(name.to_string(), value);
    }

    fn add_use(&mut self, tree: &UseTree, prefix: &[String]) {
        match tree {
            UseTree::Path(path) => {
                let mut prefix = prefix.to_vec();
                prefix.push(path.ident.to_string());
                self.add_use(&path.tree, &prefix);
            }
            UseTree::Group(group) => group
                .items
                .iter()
                .for_each(|tree| self.add_use(tree, prefix)),
            UseTree::Name(name) => {
                self.alias(&name.ident.to_string(), &name.ident.to_string(), prefix)
            }
            UseTree::Rename(name) => {
                self.alias(&name.rename.to_string(), &name.ident.to_string(), prefix)
            }
            UseTree::Glob(_) => {}
        }
    }

    fn alias(&mut self, name: &str, original: &str, prefix: &[String]) {
        let mut path = prefix.to_vec();
        if original != "self" {
            path.push(original.to_owned());
        }
        let alias = if name == "self" {
            prefix.last().map(String::as_str)
        } else {
            Some(name)
        };
        if let Some(alias) = alias {
            self.aliases.insert(alias.to_owned(), path);
        }
    }
}

struct Scanner<'ast, 'path> {
    path: &'path str,
    scopes: Vec<Scope<'ast>>,
    function: Option<String>,
    violations: Vec<String>,
}

impl<'ast> Scanner<'ast, '_> {
    fn resolve(&self, path: &syn::Path) -> Vec<String> {
        let mut segments = path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string());
        let Some(first) = segments.next() else {
            return Vec::new();
        };
        let mut resolved = self
            .scopes
            .iter()
            .rev()
            .find_map(|scope| scope.aliases.get(&first))
            .cloned()
            .unwrap_or_else(|| vec![first]);
        resolved.extend(segments);
        resolved
    }

    fn literal(&self, expression: &'ast Expr, remaining: usize) -> Option<String> {
        let remaining = remaining.checked_sub(1)?;
        match expression {
            Expr::Lit(value) => match &value.lit {
                Lit::Str(value) => Some(value.value()),
                _ => None,
            },
            Expr::Path(value) => {
                let name = value.path.segments.last()?.ident.to_string();
                let binding = self
                    .scopes
                    .iter()
                    .rev()
                    .find_map(|scope| scope.values.get(&name))?;
                self.literal(binding, remaining)
            }
            Expr::Reference(value) => self.literal(&value.expr, remaining),
            Expr::Paren(value) => self.literal(&value.expr, remaining),
            Expr::Macro(value) => macro_literal(&value.mac),
            Expr::MethodCall(value)
                if matches!(
                    value.method.to_string().as_str(),
                    "as_ref"
                        | "as_str"
                        | "as_path"
                        | "as_os_str"
                        | "into"
                        | "to_owned"
                        | "to_string"
                        | "to_path_buf"
                ) =>
            {
                self.literal(&value.receiver, remaining)
            }
            _ => None,
        }
    }

    fn report(&mut self, span: Span, message: &str) {
        self.violations
            .push(format!("{}:{}: {message}", self.path, span.start().line));
    }

    fn path_arg(&mut self, argument: &'ast Expr, push: bool) {
        let literal = self
            .literal(argument, 16)
            .or_else(|| constant_name(argument));
        let Some(literal) = literal else { return };
        if (push
            && (literal.starts_with("-D") || literal.contains("://") || literal.contains(": ")))
            || allowed(self.path, self.function.as_deref(), &literal)
        {
            return;
        }
        self.report(
            argument.span(),
            &format!("hard-coded filesystem path component {literal:?}"),
        );
    }

    fn call(&mut self, call: &'ast syn::ExprCall) {
        let Expr::Path(function) = call.func.as_ref() else {
            return;
        };
        let resolved = self.resolve(&function.path);
        let segments: Vec<_> = resolved.iter().map(String::as_str).collect();
        if chooses_system_directory(&segments) {
            self.report(
                function.span(),
                "system directory resolution belongs to storage",
            );
            return;
        }
        let Some(argument) = call.args.first() else {
            return;
        };
        if reads_environment(&segments)
            && self
                .literal(argument, 16)
                .is_some_and(|key| directory_key(&key))
        {
            self.report(
                argument.span(),
                "directory environment variables belong to storage",
            );
        }
        let arguments = filesystem_arguments(&segments)
            .iter()
            .filter_map(|index| call.args.iter().nth(*index));
        for argument in arguments {
            self.path_arg(argument, false);
        }
    }

    fn check_typed_path(&mut self, ty: &syn::Type, value: &'ast Expr) {
        if let syn::Type::Path(ty) = ty
            && self
                .resolve(&ty.path)
                .last()
                .is_some_and(|name| matches!(name.as_str(), "Path" | "PathBuf"))
        {
            self.path_arg(value, false);
        }
    }
}

fn constant_name(argument: &Expr) -> Option<String> {
    let Expr::Path(path) = argument else {
        return None;
    };
    let name = path.path.segments.last()?.ident.to_string();
    name.chars()
        .all(|symbol| symbol.is_ascii_uppercase() || symbol.is_ascii_digit() || symbol == '_')
        .then_some(name)
}

impl<'ast> Visit<'ast> for Scanner<'ast, '_> {
    fn visit_item_fn(&mut self, function: &'ast syn::ItemFn) {
        let previous = self.function.replace(function.sig.ident.to_string());
        visit::visit_item_fn(self, function);
        self.function = previous;
    }

    fn visit_impl_item_fn(&mut self, function: &'ast syn::ImplItemFn) {
        let previous = self.function.replace(function.sig.ident.to_string());
        visit::visit_impl_item_fn(self, function);
        self.function = previous;
    }

    fn visit_item_use(&mut self, item: &'ast syn::ItemUse) {
        if imports_directory_api_glob(&item.tree, false) {
            self.report(
                item.span(),
                "glob imports of directory APIs are not allowed",
            );
        }
        visit::visit_item_use(self, item);
    }

    fn visit_block(&mut self, block: &'ast syn::Block) {
        self.scopes.push(Scope::new(block.stmts.iter().filter_map(
            |statement| match statement {
                Stmt::Item(item) => Some(item),
                _ => None,
            },
        )));
        visit::visit_block(self, block);
        self.scopes.pop();
    }

    fn visit_item_mod(&mut self, module: &'ast syn::ItemMod) {
        if let Some((_, items)) = &module.content {
            self.scopes.push(Scope::new(items.iter()));
            visit::visit_item_mod(self, module);
            self.scopes.pop();
        }
    }

    fn visit_local(&mut self, local: &'ast syn::Local) {
        visit::visit_local(self, local);
        let Some(init) = &local.init else { return };
        let mut pattern = &local.pat;
        if let syn::Pat::Type(typed) = pattern {
            self.check_typed_path(&typed.ty, &init.expr);
            pattern = &typed.pat;
        }
        if let syn::Pat::Ident(binding) = pattern {
            self.scopes
                .last_mut()
                .unwrap()
                .values
                .insert(binding.ident.to_string(), &init.expr);
        }
    }

    fn visit_expr_path(&mut self, value: &'ast syn::ExprPath) {
        let resolved = self.resolve(&value.path);
        let segments: Vec<_> = resolved.iter().map(String::as_str).collect();
        if chooses_system_directory(&segments) {
            self.report(
                value.span(),
                "system directory resolution belongs to storage",
            );
        }
        visit::visit_expr_path(self, value);
    }

    fn visit_expr_call(&mut self, call: &'ast syn::ExprCall) {
        self.call(call);
        for argument in &call.args {
            self.visit_expr(argument);
        }
        if !matches!(*call.func, Expr::Path(_)) {
            self.visit_expr(&call.func);
        }
    }

    fn visit_expr_method_call(&mut self, call: &'ast syn::ExprMethodCall) {
        let method = call.method.to_string();
        if matches!(method.as_str(), "tempdir" | "tempfile") && call.args.is_empty() {
            self.report(
                call.method.span(),
                "implicit temporary directory selection belongs to storage",
            );
        } else if matches!(
            method.as_str(),
            "join"
                | "push"
                | "with_file_name"
                | "with_extension"
                | "set_file_name"
                | "set_extension"
                | "open"
        ) && let Some(argument) = call.args.first()
        {
            self.path_arg(argument, method == "push");
        }
        visit::visit_expr_method_call(self, call);
    }

    fn visit_macro(&mut self, value: &'ast syn::Macro) {
        if value
            .path
            .segments
            .last()
            .is_some_and(|part| part.ident == "env" || part.ident == "option_env")
            && first_string(value).is_some_and(|key| directory_key(&key))
        {
            self.report(
                value.span(),
                "directory environment variables belong to storage",
            );
        }
        visit::visit_macro(self, value);
    }
}

fn first_string(value: &syn::Macro) -> Option<String> {
    let TokenTree::Literal(token) = value.tokens.clone().into_iter().next()? else {
        return None;
    };
    syn::parse2::<syn::LitStr>(TokenTree::Literal(token).into())
        .ok()
        .map(|text| text.value())
}

fn macro_literal(value: &syn::Macro) -> Option<String> {
    let name = value.path.segments.last()?.ident.to_string();
    matches!(name.as_str(), "format" | "format_args" | "concat")
        .then(|| first_string(value))
        .flatten()
}

#[test]
fn path_policy_rejects_hardcoded_paths_and_direct_platform_access() {
    let source = r#"
        use std::env::{temp_dir as system_temp, var_os};
        use std::path::{PathBuf as PathAlias, Path};
        const PROFILE_ROOT: &str = "profiles";
        fn paths(root: &Path) {
            root.join(PROFILE_ROOT);
            let folder = "download-cache";
            root.join(folder);
            root.join(format!("managed-{id}"));
            PathAlias::from(r"C:\Users\someone");
            let explicit: PathAlias = "preferences".into();
            Path::join(root, "logs");
            std::fs::write("/tmp/unmanaged", bytes);
            var_os("XDG_DATA_HOME");
            system_temp();
            tempfile::Builder::new().tempdir();
            option_env!("AXLAUNCHER_DATA_DIR");
        }
    "#;
    let violations = inspect("apps/example/src/lib.rs", source);
    assert_eq!(violations.len(), 11, "{violations:#?}");
    assert!(
        violations
            .iter()
            .all(|problem| problem.starts_with("apps/example/src/lib.rs:"))
    );
}

#[test]
fn path_policy_leaves_external_inputs_and_non_path_strings_alone() {
    let source = r#"
        fn write(root: &Path, path: &Path) {
            let url = "https://example.test/assets";
            let coordinate = "org.example:tool:1.0";
            let options = "settings.json could not load";
            root.join(path);
            // root.join("profiles");
            std::env::var_os("JAVA_HOME");
            Path::new(path);
        }
    "#;
    assert!(inspect("apps/example/src/lib.rs", source).is_empty());
    let java = r#"fn executable_in_home(root: &Path) { root.join("bin"); }"#;
    assert!(inspect("apps/core/launch/src/platform.rs", java).is_empty());
    assert_eq!(inspect("apps/example/src/lib.rs", java).len(), 1);
    assert_eq!(
        inspect(
            "apps/core/launch/src/platform.rs",
            &java.replace("executable_in_home", "other")
        )
        .len(),
        1
    );
}
