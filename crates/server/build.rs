use std::{collections::HashSet, env, path::Path};

#[path = "build/docs.rs"]
mod docs;
#[path = "build/entities.rs"]
mod entities;
#[path = "build/routes.rs"]
mod routes;
#[path = "build/utils.rs"]
mod utils;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR");
    let manifest_path = Path::new(&manifest_dir);
    let src_dir = manifest_path.join("src");
    let routes_dir = src_dir.join("routes");

    let src_files = utils::collect_rust_files(&src_dir);
    for file in &src_files {
        println!("cargo:rerun-if-changed={}", file.display());
    }

    let mut registry = routes::TypeRegistry::default();
    let mut crud_context = routes::CrudTypeContext::default();
    for file in &src_files {
        let parsed = utils::parse_rust_file(file);
        let module_path = utils::module_path_for_file(file, &src_dir);
        routes::collect_type_docs(&parsed, &module_path, &mut registry);
        routes::collect_crud_service_impls(&parsed, &mut crud_context.service_to_dao);
        routes::collect_dao_base_impls(&parsed, &mut crud_context.dao_to_entity);
    }
    crud_context.entity_to_model =
        routes::collect_entity_model_map(&src_dir.join("db/entities"), &src_dir);

    let mut routes_list = Vec::new();
    for file in routes::collect_route_files(&routes_dir) {
        routes_list.extend(routes::parse_routes_file(
            &file,
            manifest_path,
            &src_dir,
            &registry,
            &crud_context,
        ));
        routes_list.extend(routes::parse_crud_router_routes(
            &file,
            manifest_path,
            &registry,
            &crud_context,
        ));
    }

    routes_list.sort_by(|a, b| a.path.cmp(&b.path).then(a.method.cmp(&b.method)));

    let mut entities_list = Vec::new();
    for file in &src_files {
        let parsed = utils::parse_rust_file(file);
        let module_path = utils::module_path_for_file(file, &src_dir);
        entities::collect_entity_entries(&parsed.items, &module_path, &mut entities_list);
    }

    entities_list.sort_by(|a, b| a.entity.cmp(&b.entity));

    let mut relations = Vec::new();
    for file in &src_files {
        let parsed = utils::parse_rust_file(file);
        let module_path = utils::module_path_for_file(file, &src_dir);
        entities::collect_entity_relations(&parsed.items, &module_path, &mut relations);
    }

    let entity_names: HashSet<String> = entities_list
        .iter()
        .map(|entity| entity.entity.clone())
        .collect();
    relations.retain(|relation| {
        entity_names.contains(&relation.from) && entity_names.contains(&relation.to)
    });
    relations.sort_by(|a, b| {
        a.from
            .cmp(&b.from)
            .then(a.to.cmp(&b.to))
            .then(a.kind.as_str().cmp(b.kind.as_str()))
            .then(a.label.cmp(&b.label))
    });
    relations.dedup();

    let out_dir = env::var("OUT_DIR").expect("missing OUT_DIR");
    let out_path = Path::new(&out_dir);
    routes::write_routes(out_path, &routes_list);
    entities::write_entities(out_path, &entities_list, &relations);
    docs::write_docs_sections(manifest_path, out_path);
}
