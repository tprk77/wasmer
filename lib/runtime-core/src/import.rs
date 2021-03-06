use crate::export::Export;
use hashbrown::{hash_map::Entry, HashMap, HashSet};

pub struct ExportIter<'a> {
    like_namespace: &'a LikeNamespace,
    export_names: Box<dyn Iterator<Item = &'a str> + 'a>,
}

impl<'a> ExportIter<'a> {
    fn new(like_namespace: &'a LikeNamespace) -> ExportIter<'a> {
        ExportIter {
            like_namespace,
            export_names: Box::new(like_namespace.export_names()),
        }
    }
}

impl<'a> Iterator for ExportIter<'a> {
    type Item = (&'a str, Export);

    fn next(&mut self) -> Option<(&'a str, Export)> {
        let export_name = self.export_names.next()?;
        self.like_namespace
            .get_export(&export_name)
            .map(|export| (export_name, export))
    }
}

pub trait LikeNamespace {
    fn get_export(&self, name: &str) -> Option<Export>;
    fn export_names<'a>(&'a self) -> Box<dyn Iterator<Item = &'a str> + 'a>;
}

pub trait IsExport {
    fn to_export(&self) -> Export;
}

impl IsExport for Export {
    fn to_export(&self) -> Export {
        self.clone()
    }
}

impl<'a> IntoIterator for &'a LikeNamespace {
    type Item = (&'a str, Export);
    type IntoIter = ExportIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        ExportIter::new(self)
    }
}

/// All of the import data used when instantiating.
///
/// It's suggested that you use the [`imports!`] macro
/// instead of creating an `ImportObject` by hand.
///
/// [`imports!`]: macro.imports.html
///
/// # Usage:
/// ```
/// # use wasmer_runtime_core::{imports, func};
/// # use wasmer_runtime_core::vm::Ctx;
/// let import_object = imports! {
///     "env" => {
///         "foo" => func!(foo),
///     },
/// };
///
/// fn foo(_: &mut Ctx, n: i32) -> i32 {
///     n
/// }
/// ```
pub struct ImportObject {
    map: HashMap<String, Box<dyn LikeNamespace>>,
}

impl ImportObject {
    /// Create a new `ImportObject`.
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// Register anything that implements `LikeNamespace` as a namespace.
    ///
    /// # Usage:
    /// ```
    /// # use wasmer_runtime_core::Instance;
    /// # use wasmer_runtime_core::import::{ImportObject, Namespace};
    /// fn register(instance: Instance, namespace: Namespace) {
    ///     let mut import_object = ImportObject::new();
    ///
    ///     import_object.register("namespace0", instance);
    ///     import_object.register("namespace1", namespace);
    ///     // ...
    /// }
    /// ```
    pub fn register<S, N>(&mut self, name: S, namespace: N) -> Option<Box<dyn LikeNamespace>>
    where
        S: Into<String>,
        N: LikeNamespace + 'static,
    {
        match self.map.entry(name.into()) {
            Entry::Vacant(empty) => {
                empty.insert(Box::new(namespace));
                None
            }
            Entry::Occupied(mut occupied) => Some(occupied.insert(Box::new(namespace))),
        }
    }

    pub fn get_namespace(&self, namespace: &str) -> Option<&(dyn LikeNamespace + 'static)> {
        self.map.get(namespace).map(|namespace| &**namespace)
    }

    /// Merge two `ImportObject`'s into one, also merging namespaces.
    ///
    /// When a namespace is unique to the first or second import object, that namespace is moved
    /// directly into the merged import object. When a namespace with the same name occurs in both
    /// import objects, a merge namespace is created. When an export is unique to the first or
    /// second namespace, that export is moved directly into the merged namespace. When an export
    /// with the same name occurs in both namespaces, then the export from the first namespace is
    /// used, and the export from the second namespace is dropped.
    ///
    /// # Usage #
    ///
    /// ```
    /// # use wasmer_runtime_core::import::ImportObject;
    /// fn merge(imports_a: ImportObject, imports_b: ImportObject) {
    ///     let merged_imports = ImportObject::merge(imports_a, imports_b);
    ///     // ...
    /// }
    /// ```
    pub fn merge(mut imports_a: ImportObject, mut imports_b: ImportObject) -> Self {
        let names_a = imports_a.map.keys();
        let names_b = imports_b.map.keys();
        let names_ab: HashSet<String> = names_a.chain(names_b).cloned().collect();
        let mut merged_imports = ImportObject::new();
        for name in names_ab {
            match (imports_a.map.remove(&name), imports_b.map.remove(&name)) {
                (Some(namespace_a), Some(namespace_b)) => {
                    // Create a combined namespace
                    let mut namespace_ab = Namespace::new();
                    let mut exports_a: HashMap<&str, Export> = namespace_a.into_iter().collect();
                    let mut exports_b: HashMap<&str, Export> = namespace_b.into_iter().collect();
                    // Import from A will win over B
                    namespace_ab
                        .map
                        .extend(exports_b.drain().map(|(export_name, export)| {
                            (export_name.to_string(), Box::new(export) as Box<IsExport>)
                        }));
                    namespace_ab
                        .map
                        .extend(exports_a.drain().map(|(export_name, export)| {
                            (export_name.to_string(), Box::new(export) as Box<IsExport>)
                        }));
                    merged_imports.map.insert(name, Box::new(namespace_ab));
                }
                (Some(namespace_a), None) => {
                    merged_imports.map.insert(name, namespace_a);
                }
                (None, Some(namespace_b)) => {
                    merged_imports.map.insert(name, namespace_b);
                }
                (None, None) => panic!("Unreachable"),
            }
        }
        merged_imports
    }
}

impl IntoIterator for ImportObject {
    type Item = (String, Box<dyn LikeNamespace>);
    type IntoIter = hashbrown::hash_map::IntoIter<String, Box<dyn LikeNamespace>>;

    fn into_iter(self) -> Self::IntoIter {
        self.map.into_iter()
    }
}

impl<'a> IntoIterator for &'a ImportObject {
    type Item = (&'a String, &'a Box<dyn LikeNamespace>);
    type IntoIter = hashbrown::hash_map::Iter<'a, String, Box<dyn LikeNamespace>>;

    fn into_iter(self) -> Self::IntoIter {
        self.map.iter()
    }
}

pub struct Namespace {
    map: HashMap<String, Box<dyn IsExport>>,
}

impl Namespace {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn insert<S, E>(&mut self, name: S, export: E) -> Option<Box<dyn IsExport>>
    where
        S: Into<String>,
        E: IsExport + 'static,
    {
        self.map.insert(name.into(), Box::new(export))
    }
}

impl LikeNamespace for Namespace {
    fn get_export(&self, name: &str) -> Option<Export> {
        self.map.get(name).map(|is_export| is_export.to_export())
    }

    fn export_names<'a>(&'a self) -> Box<dyn Iterator<Item = &'a str> + 'a> {
        Box::new(self.map.keys().map(|s| s.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::global::Global;
    use crate::types::Value;
    use crate::vm::Ctx;

    #[test]
    fn test_merge_import() {
        // Create some imports for testing
        fn func_a(_ctx: &mut Ctx) -> i32 {
            0_i32
        }
        let imports_a = imports! {
            "only_in_a" => {
                "a" => func!(func_a),
            },
            "env" => {
                "a" => func!(func_a),
                "x" => func!(func_a),
            },
        };
        let imports_b = imports! {
            "only_in_b" => {
                "b" => Global::new(Value::I32(77)),
            },
            "env" => {
                "b" => Global::new(Value::I32(77)),
                "x" => Global::new(Value::I32(77)),
            },
        };
        let merged_imports = ImportObject::merge(imports_a, imports_b);
        // Make sure everything is there that should be
        let namespace_a = merged_imports.get_namespace("only_in_a").unwrap();
        let namespace_b = merged_imports.get_namespace("only_in_b").unwrap();
        let namespace_env = merged_imports.get_namespace("env").unwrap();
        let export_a_a = namespace_a.get_export("a").unwrap();
        let export_b_b = namespace_b.get_export("b").unwrap();
        let export_env_a = namespace_env.get_export("a").unwrap();
        let export_env_b = namespace_env.get_export("b").unwrap();
        let export_env_x = namespace_env.get_export("x").unwrap();
        // Make sure that the types are what we expected
        assert!(match export_a_a {
            Export::Function { .. } => true,
            _ => false,
        });
        assert!(match export_b_b {
            Export::Global(_) => true,
            _ => false,
        });
        assert!(match export_env_a {
            Export::Function { .. } => true,
            _ => false,
        });
        assert!(match export_env_b {
            Export::Global(_) => true,
            _ => false,
        });
        // This should be the funtion from A, not the global from B
        assert!(match export_env_x {
            Export::Function { .. } => true,
            _ => false,
        });
    }

    #[test]
    fn test_import_object_iteration() {
        // Create some imports for testing
        let imports = imports! {
            "env" => {
                "x" => Global::new(Value::I32(77)),
            },
            "env2" => {
                "x" => Global::new(Value::I32(77)),
            },
        };
        // Iterate over the namespaces by reference
        for (namespace_name, namespace) in &imports {
            let export = namespace.get_export("x").unwrap();
            assert!(match export {
                Export::Global(_) => true,
                _ => false,
            });
        }
        assert!((&imports).into_iter().count() == 2);
        // Iterate over the namespaces by value
        let mut iter_counter = 0;
        for (namespace_name, namespace) in imports {
            let export = namespace.get_export("x").unwrap();
            assert!(match export {
                Export::Global(_) => true,
                _ => false,
            });
            iter_counter += 1;
        }
        assert!(iter_counter == 2);
    }

    #[test]
    fn test_like_namespace_iteration() {
        // Create some imports for testing
        let imports = imports! {
            "env" => {
                "x" => Global::new(Value::I32(77)),
                "y" => Global::new(Value::I32(77)),
                "z" => Global::new(Value::I32(77)),
            },
        };
        // Get the namespace and iterate over it
        let namespace = imports.get_namespace("env").unwrap();
        for (export_name, export) in namespace {
            assert!(match export {
                Export::Global(_) => true,
                _ => false,
            });
        }
        assert!(namespace.into_iter().count() == 3);
    }
}
