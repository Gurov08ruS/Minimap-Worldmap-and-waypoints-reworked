use quote::quote;
use std::{collections::BTreeMap, env, fs, path::Path};
use syn::{Block, Item};
fn block<'a>(items: &'a mut [Item], node: &[usize]) -> &'a mut Block {
    let (index, rest) = node.split_first().unwrap();
    match &mut items[*index] {
        Item::Fn(f) if rest.is_empty() => &mut f.block,
        Item::Impl(i) if rest.len() == 1 => match &mut i.items[rest[0]] {
            syn::ImplItem::Fn(f) => &mut f.block,
            _ => panic!("planned method absent"),
        },
        Item::Mod(m) => block(&mut m.content.as_mut().unwrap().1, rest),
        _ => panic!("planned target absent"),
    }
}
fn canonical(items: &[Item]) -> String {
    prettyplease::unparse(&syn::File {
        shebang: None,
        attrs: vec![],
        items: items.to_vec(),
    })
}
fn canonical_file_tokens(file: &syn::File) -> String {
    let formatted = syn::parse_file(&prettyplease::unparse(file)).unwrap();
    quote!(#formatted).to_string()
}
fn main() {
    let args: Vec<_> = env::args().collect();
    let plan: serde_json::Value = serde_json::from_slice(&fs::read(&args[3]).unwrap()).unwrap();
    let mut trees = BTreeMap::new();
    for section in ["injections", "modules"] {
        for entry in plan[section].as_array().unwrap() {
            let file = entry["file"].as_str().unwrap();
            trees.entry(file).or_insert_with(|| {
                syn::parse_file(&fs::read_to_string(Path::new(&args[2]).join(file)).unwrap())
                    .unwrap()
            });
        }
    }
    for p in plan["injections"].as_array().unwrap() {
        let file = p["file"].as_str().unwrap();
        let tree = trees.entry(file).or_insert_with(|| {
            syn::parse_file(&fs::read_to_string(Path::new(&args[2]).join(file)).unwrap()).unwrap()
        });
        let node: Vec<usize> = p["node"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_u64().unwrap() as usize)
            .collect();
        assert_eq!(p["kind"], "HEAD");
        let helper: syn::ItemFn = syn::parse_str(p["helper_source"].as_str().unwrap()).unwrap();
        let name = &helper.sig.ident;
        let args: Vec<syn::Expr> = p["bindings"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| syn::parse_str(b["value"].as_str().unwrap()).unwrap())
            .collect();
        let expected: syn::Stmt = if p["cancellable"].as_bool().unwrap() {
            syn::parse2(quote!(if let ::core::ops::ControlFlow::Break(__horizon_mixins_return) = {#helper #name(#(#args),*)} { return __horizon_mixins_return; })).unwrap()
        } else {
            syn::parse2(quote!({#helper #name(#(#args),*)};)).unwrap()
        };
        let b = block(&mut tree.items, &node);
        let actual = b.stmts.first().expect("HEAD missing");
        let actual_fn: Item = syn::parse_quote!(fn audit() {#actual});
        let expected_fn: Item = syn::parse_quote!(fn audit() {#expected});
        assert_eq!(
            canonical(&[actual_fn]),
            canonical(&[expected_fn]),
            "HEAD differs from planned helper/bindings"
        );
        b.stmts.remove(0);
    }
    for m in plan["modules"].as_array().unwrap() {
        let file = m["file"].as_str().unwrap();
        let tree = trees.get_mut(file).unwrap();
        let index = tree
            .items
            .iter()
            .position(|i| matches!(i,Item::Mod(v) if v.ident==m["name"].as_str().unwrap()))
            .expect("support module missing");
        let Item::Mod(module) = tree.items.remove(index) else {
            unreachable!()
        };
        let payload = syn::parse_file(m["source"].as_str().unwrap()).unwrap();
        assert!(module.attrs.is_empty());
        assert!(matches!(&module.vis, syn::Visibility::Restricted(v) if v.path.is_ident("crate")));
        let mut expected = module.clone();
        expected.content.as_mut().unwrap().1 = payload.items;
        assert!(
            canonical(&[Item::Mod(module)]) == canonical(&[Item::Mod(expected)]),
            "support module differs from planned source"
        );
    }
    fn verify_other_sources(original: &Path, stage: &Path, relative: &Path, changed: &[&str]) {
        for entry in fs::read_dir(original.join(relative)).unwrap() {
            let entry = entry.unwrap();
            let path = relative.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() {
                verify_other_sources(original, stage, &path, changed);
            } else if path.extension().is_some_and(|v| v == "rs")
                && !changed.contains(&path.to_str().unwrap())
            {
                assert_eq!(
                    fs::read(original.join(&path)).unwrap(),
                    fs::read(stage.join(&path)).unwrap(),
                    "unplanned source change: {}",
                    path.display()
                );
            }
        }
    }
    let changed: Vec<_> = trees.keys().copied().collect();
    verify_other_sources(
        Path::new(&args[1]),
        Path::new(&args[2]),
        Path::new(""),
        &changed,
    );
    for (file, tree) in trees {
        let original =
            syn::parse_file(&fs::read_to_string(Path::new(&args[1]).join(file)).unwrap()).unwrap();
        assert_eq!(original.shebang, tree.shebang, "changed shebang in {file}");
        // Normalize optional punctuation and match-arm braces with the same
        // formatter as the transformer, then ignore layout. prettyplease is
        // not whitespace-idempotent for every upstream method chain.
        if canonical_file_tokens(&original) != canonical_file_tokens(&tree) {
            let diagnostics = Path::new("target/source-audit");
            fs::create_dir_all(diagnostics).unwrap();
            fs::write(
                diagnostics.join("original.rs"),
                prettyplease::unparse(&original),
            )
            .unwrap();
            fs::write(
                diagnostics.join("restored.rs"),
                prettyplease::unparse(&tree),
            )
            .unwrap();
            panic!(
                "unexpected source change in {file}; compare target/source-audit/{{original,restored}}.rs"
            );
        }
    }
    println!(
        "all original Rust sources preserved apart from planned HEAD statements and support modules"
    );
}
