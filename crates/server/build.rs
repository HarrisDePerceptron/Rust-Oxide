use std::{collections::HashSet, env, path::Path};

#[path = "build/docs.rs"]
mod docs;
#[path = "build/entities.rs"]
mod entities;
#[path = "build/routes.rs"]
mod routes;
#[path = "build/utils.rs"]
mod utils;

fn strict_build_mode() -> bool {
    if let Ok(raw) = env::var("RUST_OXIDE_BUILD_STRICT") {
        return match raw.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => true,
            "0" | "false" | "no" | "off" => false,
            invalid => {
                println!(
                    "cargo:warning=invalid RUST_OXIDE_BUILD_STRICT value '{}'; expected one of [1,true,yes,on,0,false,no,off], falling back to PROFILE",
                    invalid
                );
                profile_is_strict()
            }
        };
    }
    profile_is_strict()
}

fn profile_is_strict() -> bool {
    matches!(
        env::var("PROFILE").as_deref(),
        Ok("release") | Ok("bench")
    )
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=RUST_OXIDE_BUILD_STRICT");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR");
    let manifest_path = Path::new(&manifest_dir);
    let src_dir = manifest_path.join("src");
    let routes_dir = src_dir.join("routes");
    let out_dir = env::var("OUT_DIR").expect("missing OUT_DIR");
    let out_path = Path::new(&out_dir);

    let src_files = utils::collect_rust_files(&src_dir);
    for file in &src_files {
        println!("cargo:rerun-if-changed={}", file.display());
    }

    let mut parse_errors = Vec::new();
    for file in &src_files {
        if let Err(err) = utils::try_parse_rust_file(file) {
            parse_errors.push(err);
        }
    }

    if !parse_errors.is_empty() {
        for error in &parse_errors {
            println!("cargo:warning={error}");
        }

        if strict_build_mode() {
            panic!(
                "build metadata parse failed for {} file(s); set RUST_OXIDE_BUILD_STRICT=0 (or unset) to allow fallback metadata in local dev",
                parse_errors.len()
            );
        }

        println!(
            "cargo:warning=metadata generation fallback enabled: route/entity catalogs will be empty for this build; fix parse errors above to restore generated docs"
        );

        let empty_routes: Vec<routes::RouteEntry> = Vec::new();
        let empty_entities: Vec<entities::EntityEntry> = Vec::new();
        let empty_relations: Vec<entities::EntityRelationEntry> = Vec::new();

        routes::write_routes(out_path, &empty_routes);
        entities::write_entities(out_path, &empty_entities, &empty_relations);
        docs::write_docs_sections(manifest_path, out_path);
        return;
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

    routes::write_routes(out_path, &routes_list);
    entities::write_entities(out_path, &entities_list, &relations);
    docs::write_docs_sections(manifest_path, out_path);
}
