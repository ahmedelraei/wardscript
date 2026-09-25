//! Incremental analysis with `salsa`. Parses are memoized by file content, so an edit
//! re-parses only the edited file; an analysis is reused until an open document
//! changes or the disk may have.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use salsa::Setter as _;
use ward_check::Analysis;
use ward_resolve::FileSystem;
use ward_syntax::Diagnostic;
use ward_syntax::ast::Module;

#[salsa::input]
struct Document {
    #[returns(ref)]
    text: String,
}

/// Open documents, and a counter bumped whenever files on disk may have changed.
#[salsa::input]
struct Workspace {
    #[returns(ref)]
    docs: BTreeMap<PathBuf, Document>,
    #[returns(copy)]
    disk_epoch: u64,
}

#[salsa::interned]
struct Source {
    #[returns(ref)]
    path: PathBuf,
    #[returns(ref)]
    text: String,
}

#[salsa::interned]
struct Entry {
    #[returns(ref)]
    path: PathBuf,
}

#[derive(Clone)]
struct Parsed(Arc<Module>, Vec<Diagnostic>);

// Module has no Eq worth paying for; Parsed changes exactly when the source does.
#[salsa::tracked(returns(ref), no_eq)]
fn parse(db: &dyn salsa::Database, source: Source<'_>) -> Parsed {
    let parse = ward_syntax::parse(source.text(db));
    Parsed(Arc::new(parse.module), parse.diagnostics)
}

#[salsa::tracked(returns(ref), no_eq)]
fn analysis(db: &dyn salsa::Database, ws: Workspace, entry: Entry<'_>) -> Option<Arc<Analysis>> {
    ws.disk_epoch(db);
    let fs = Files { db, ws };
    let parse_fn = |path: &Path, src: &str| {
        let Parsed(module, diags) = parse(db, Source::new(db, path.to_owned(), src.to_owned()));
        (module.clone(), diags.clone())
    };
    ward_check::analyze_with(entry.path(db), &fs, &parse_fn)
        .ok()
        .map(Arc::new)
}

struct Files<'a> {
    db: &'a dyn salsa::Database,
    ws: Workspace,
}

impl FileSystem for Files<'_> {
    fn read(&self, path: &Path) -> io::Result<String> {
        match self.ws.docs(self.db).get(path) {
            Some(doc) => Ok(doc.text(self.db).clone()),
            None => std::fs::read_to_string(path),
        }
    }
}

#[salsa::db]
#[derive(Clone, Default)]
struct Storage {
    storage: salsa::Storage<Self>,
}

#[salsa::db]
impl salsa::Database for Storage {}

pub struct Db {
    storage: Storage,
    ws: Workspace,
}

impl Default for Db {
    fn default() -> Self {
        let storage = Storage::default();
        let ws = Workspace::new(&storage, BTreeMap::new(), 0);
        Db { storage, ws }
    }
}

impl Db {
    pub fn text(&self, path: &Path) -> Option<&str> {
        let doc = self.ws.docs(&self.storage).get(path)?;
        Some(doc.text(&self.storage))
    }

    pub fn set_text(&mut self, path: &Path, text: &str) {
        if let Some(&doc) = self.ws.docs(&self.storage).get(path) {
            if doc.text(&self.storage) != text {
                doc.set_text(&mut self.storage).to(text.to_owned());
            }
            return;
        }
        let doc = Document::new(&self.storage, text.to_owned());
        let mut docs = self.ws.docs(&self.storage).clone();
        docs.insert(path.to_owned(), doc);
        self.ws.set_docs(&mut self.storage).to(docs);
    }

    pub fn close(&mut self, path: &Path) {
        let mut docs = self.ws.docs(&self.storage).clone();
        if docs.remove(path).is_some() {
            self.ws.set_docs(&mut self.storage).to(docs);
        }
    }

    /// Files outside the open documents may have changed.
    pub fn disk_changed(&mut self) {
        let epoch = self.ws.disk_epoch(&self.storage);
        self.ws.set_disk_epoch(&mut self.storage).to(epoch + 1);
    }

    pub fn analysis(&self, entry: &Path) -> Option<Arc<Analysis>> {
        let entry = Entry::new(&self.storage, entry.to_owned());
        analysis(&self.storage, self.ws, entry).clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ward_resolve::ModuleId;

    fn tmp_program(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ward_lsp_db_{name}_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn analysis_is_reused_until_an_input_changes() {
        let dir = tmp_program("reuse");
        let main = dir.join("main.ward");
        let mut db = Db::default();
        db.set_text(&main, "fn f() -> Int { 1 }\n");
        let a = db.analysis(&main).expect("analysis");
        let b = db.analysis(&main).expect("analysis");
        assert!(Arc::ptr_eq(&a, &b));

        db.set_text(&main, "fn f() -> Int { 1 }\n");
        assert!(Arc::ptr_eq(&a, &db.analysis(&main).expect("analysis")));

        db.set_text(&main, "fn f() -> Int { \"no\" }\n");
        let c = db.analysis(&main).expect("analysis");
        assert!(!Arc::ptr_eq(&a, &c));
        assert!(!c.diagnostics().is_empty());
    }

    #[test]
    fn editing_one_file_reuses_the_parse_of_the_other() {
        let dir = tmp_program("imports");
        let main = dir.join("main.ward");
        let lib = dir.join("lib.ward");
        let mut db = Db::default();
        db.set_text(&lib, "pub fn one() -> Int { 1 }\n");
        db.set_text(&main, "import lib\n\nfn f() -> Int { lib.one() }\n");
        let before = db.analysis(&main).expect("analysis");
        assert!(
            before.diagnostics().is_empty(),
            "{:?}",
            before.diagnostics()
        );

        db.set_text(&main, "import lib\n\nfn g() -> Int { lib.one() }\n");
        let after = db.analysis(&main).expect("analysis");
        let lib_ast = |a: &Analysis| a.program.module(ModuleId(1)).ast.clone();
        assert!(Arc::ptr_eq(&lib_ast(&before), &lib_ast(&after)));
        assert!(!Arc::ptr_eq(
            &before.program.module(ModuleId(0)).ast,
            &after.program.module(ModuleId(0)).ast
        ));
    }

    #[test]
    fn disk_changes_are_seen_after_disk_changed() {
        let dir = tmp_program("disk");
        let main = dir.join("main.ward");
        let lib = dir.join("lib.ward");
        std::fs::write(&lib, "pub fn one() -> Int { 1 }\n").expect("write");
        let mut db = Db::default();
        db.set_text(&main, "import lib\n\nfn f() -> Int { lib.one() }\n");
        assert!(
            db.analysis(&main)
                .expect("analysis")
                .diagnostics()
                .is_empty()
        );

        std::fs::write(&lib, "pub fn two() -> Int { 2 }\n").expect("write");
        db.disk_changed();
        assert!(
            !db.analysis(&main)
                .expect("analysis")
                .diagnostics()
                .is_empty()
        );
    }
}
