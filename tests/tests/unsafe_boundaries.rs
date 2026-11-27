use std::fs;
use std::path::Path;

use syn::visit::{self, Visit};
use syn::{Attribute, Signature};

const NATIVE: &str = "apps/core/lib/src/native.rs";
const EXPORTS: &[(&str, &str)] = &[
    (
        "apps/core/lib/accounts/src/worker.rs",
        "axlauncher_accounts_v1",
    ),
    (
        "apps/core/lib/download_engine/src/worker.rs",
        "axlauncher_downloads_v1",
    ),
    (
        "apps/core/lib/java_discovery/src/worker.rs",
        "axlauncher_java_discovery_v1",
    ),
    (
        "apps/core/lib/instance_settings/src/worker.rs",
        "axlauncher_instance_settings_v1",
    ),
];

#[test]
fn unsafe_is_local_to_the_embedded_library_boundary() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    for directory in ["apps", "platforms", "tools"] {
        scan(root, &root.join(directory));
    }
}

fn scan(root: &Path, directory: &Path) {
    for entry in fs::read_dir(directory).unwrap().map(Result::unwrap) {
        if ["node_modules", "target", ".git"]
            .iter()
            .any(|name| entry.file_name() == *name)
        {
            continue;
        }
        let path = entry.path();
        let kind = entry.file_type().unwrap();
        if kind.is_dir() {
            scan(root, &path);
        } else if kind.is_file() && path.extension().is_some_and(|extension| extension == "rs") {
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            let syntax = syn::parse_file(&fs::read_to_string(&path).unwrap()).unwrap();
            let mut boundary = Boundary {
                file: &relative,
                function: None,
            };
            boundary.visit_file(&syntax);
        }
    }
}

struct Boundary<'a> {
    file: &'a str,
    function: Option<String>,
}

impl Boundary<'_> {
    fn permits_export(&self) -> bool {
        self.function
            .as_deref()
            .is_some_and(|function| EXPORTS.contains(&(self.file, function)))
    }

    fn permits_loading(&self) -> bool {
        self.file == NATIVE && self.function.as_deref() == Some("invoke")
    }
}

impl<'ast> Visit<'ast> for Boundary<'_> {
    fn visit_item_fn(&mut self, item: &'ast syn::ItemFn) {
        let previous = self.function.replace(item.sig.ident.to_string());
        visit::visit_item_fn(self, item);
        self.function = previous;
    }

    fn visit_attribute(&mut self, attribute: &'ast Attribute) {
        let syn::Meta::List(list) = &attribute.meta else {
            return;
        };
        let arguments = list.tokens.to_string();
        if ["allow", "expect", "cfg_attr"]
            .iter()
            .any(|name| list.path.is_ident(name))
            && arguments.contains("unsafe_code")
        {
            assert!(
                self.permits_loading() || self.permits_export(),
                "unsafe lint override in {}",
                self.file
            );
        }
        if list.path.is_ident("unsafe") {
            assert!(
                self.permits_export() && arguments == "no_mangle",
                "unsafe attribute in {}",
                self.file
            );
        }
    }

    fn visit_signature(&mut self, signature: &'ast Signature) {
        assert!(
            signature.unsafety.is_none(),
            "unexpected unsafe signature in {}",
            self.file
        );
        visit::visit_signature(self, signature);
    }

    fn visit_expr_unsafe(&mut self, expression: &'ast syn::ExprUnsafe) {
        assert!(
            self.permits_loading(),
            "unexpected unsafe block in {}",
            self.file
        );
        visit::visit_expr_unsafe(self, expression);
    }
}
