use super::*;

pub(super) fn compile_packages(
    records: &BTreeMap<Key, Value>,
    revision: u64,
) -> Result<BTreeMap<String, Value>> {
    let mut sources = BTreeMap::<Key, Source>::new();
    let mut dashboards = BTreeMap::<Key, Dashboard>::new();
    for (key, doc) in records.iter().filter(|(k, _)| authored(&k.kind)) {
        if key.kind == "dashboard" {
            let d: Dashboard = parse(key, doc)?;
            if !d.params.is_object() {
                return Err(Diagnostic::new("invalid_input", "params must be an object")
                    .at(key, "document.params"));
            }
            dashboards.insert(key.clone(), d);
        } else {
            sources.insert(key.clone(), parse(key, doc)?);
        }
    }
    // Resolve every shared definition, including currently unused definitions.
    fn visit(
        k: &Key,
        sources: &BTreeMap<Key, Source>,
        active: &mut BTreeSet<Key>,
        done: &mut BTreeSet<Key>,
        order: &mut Vec<Key>,
    ) -> Result<()> {
        check_key(k)?;
        if done.contains(k) {
            return Ok(());
        }
        let s = sources.get(k).ok_or_else(|| {
            Diagnostic::new("validation_failed", "missing shared definition").at(k, "references")
        })?;
        if active.len() >= 64 {
            return Err(
                Diagnostic::new("limit_exceeded", "reference depth limit").at(k, "references")
            );
        }
        if !active.insert(k.clone()) {
            return Err(
                Diagnostic::new("validation_failed", "shared reference cycle").at(k, "references"),
            );
        }
        if s.references.len() > 64 {
            return Err(Diagnostic::new("limit_exceeded", "reference limit").at(k, "references"));
        }
        for dep in &s.references {
            if k.kind == "ui" && dep.kind != "ui" {
                return Err(
                    Diagnostic::new("validation_failed", "UI references must target UI")
                        .at(k, "references"),
                );
            }
            visit(dep, sources, active, done, order)?;
        }
        active.remove(k);
        done.insert(k.clone());
        order.push(k.clone());
        Ok(())
    }
    let compiler = Script::new()?;
    compiler.eval(include_str!("../../../dashboard/shared/ui.js"))?;
    let mut compiled_ui = BTreeMap::new();
    let mut compiled_js = BTreeMap::new();
    for (key, s) in &sources {
        visit(
            key,
            &sources,
            &mut BTreeSet::new(),
            &mut BTreeSet::new(),
            &mut vec![],
        )?;
        if s.source.len() > 131072 {
            return Err(Diagnostic::new("limit_exceeded", "source size limit").at(key, "source"));
        }
        if key.kind == "ui" {
            compiled_ui.insert(key.clone(), ui_compile(&compiler, key, &s.source, true)?);
        } else {
            let call = if key.kind == "vm" {
                "defineVMReference"
            } else {
                "defineFunction"
            };
            let code = format!("{call}({},({}));", json!(key.id), s.source);
            syntax(key, &code)?;
            compiled_js.insert(key.clone(), code);
        }
    }
    let mut packages = BTreeMap::new();
    // Validate shared UI references even when no dashboard uses them.
    for key in compiled_ui.keys() {
        let mut order = vec![];
        visit(
            key,
            &sources,
            &mut BTreeSet::new(),
            &mut BTreeSet::new(),
            &mut order,
        )?;
        // ScreenRef targets belong to the containing dashboard; check those only
        // when linking a package. Shared Use references must resolve even unused.
        let allowed: BTreeSet<_> = order
            .iter()
            .filter(|k| k.kind == "ui" && *k != key)
            .map(|k| k.id.as_str())
            .collect();
        let mut nodes = vec![&compiled_ui[key]];
        while let Some(node) = nodes.pop() {
            if node["type"] == "Use"
                && !node["props"]["definition"]
                    .as_str()
                    .is_some_and(|name| allowed.contains(name))
            {
                return Err(Diagnostic::new(
                    "validation_failed",
                    "missing or cyclic shared UI reference",
                )
                .at(key, "references"));
            }
            if let Some(children) = node["children"].as_array() {
                nodes.extend(children);
            }
        }
    }
    for (key, d) in dashboards {
        if !key
            .id
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
            || !key
                .id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
            || key.id.len() > 64
        {
            return Err(Diagnostic::new("invalid_input", "dashboard ID syntax").at(&key, "key.id"));
        }
        if d.references.len() > 64 {
            return Err(Diagnostic::new("limit_exceeded", "reference limit").at(&key, "references"));
        }
        let mut order = vec![];
        let mut done = BTreeSet::new();
        for dep in &d.references {
            visit(dep, &sources, &mut BTreeSet::new(), &mut done, &mut order)?;
        }
        let mut definitions = serde_json::Map::new();
        let mut code = String::new();
        for dep in order {
            if let Some(ui) = compiled_ui.get(&dep) {
                definitions.insert(dep.id.clone(), ui.clone());
            }
            if let Some(js) = compiled_js.get(&dep) {
                code.push_str(js);
                code.push('\n');
            }
        }
        syntax(&key, &d.view_model)?;
        code.push_str(&d.view_model);
        syntax(&key, &code)?;
        let ui = ui_compile(&compiler, &key, &d.ui, false)?;
        let mut pkg = json!({"version":1,"id":key.id,"revision":format!("catalog-{revision}-{}",key.id),"ui":ui,"definitions":definitions,"viewModel":code,"params":d.params});
        if let Some(grants) = d.grants {
            pkg["grants"] = grants;
        }
        trusted(
            &compiler,
            &key,
            "document",
            &format!("TaliaUI.validatePackage({pkg});"),
        )?;
        if serde_json::to_vec(&pkg)?.len() > 262144 {
            return Err(
                Diagnostic::new("limit_exceeded", "compiled package size limit")
                    .at(&key, "document"),
            );
        }
        packages.insert(key.id, pkg);
    }
    Ok(packages)
}
fn trusted(s: &Script, key: &Key, path: &str, code: &str) -> Result<()> {
    s.eval(code).map_err(|e| {
        let mut d = Diagnostic::from(e.clone()).at(key, path);
        let parts: Vec<_> = e.splitn(3, ':').collect();
        if parts.len() == 3 {
            d.line = parts[0].parse().ok();
            d.column = parts[1].parse().ok();
        }
        d
    })
}
fn ui_compile(s: &Script, key: &Key, source: &str, fragment: bool) -> Result<Value> {
    let method = if fragment {
        "compileDefinition"
    } else {
        "compile"
    };
    trusted(
        s,
        key,
        if fragment { "source" } else { "ui" },
        &format!("globalThis.compiled=TaliaUI.{method}({});", json!(source)),
    )
    .map_err(|mut e| {
        if fragment && e.line == Some(1) {
            let prefix =
                "<Dashboard id=\"DefinitionRoot\"><Surface id=\"DefinitionSurface\">".len() as u64;
            e.column = e.column.map(|n| n.saturating_sub(prefix).max(1));
        }
        e
    })?;
    Ok(serde_json::from_str(
        &s.string("JSON.stringify(compiled)")?,
    )?)
}
fn syntax(key: &Key, source: &str) -> Result<()> {
    if source.len() > 131072 {
        return Err(Diagnostic::new("limit_exceeded", "JavaScript source limit").at(key, "source"));
    }
    let s = Script::new()?;
    // Compile as a module to reject top-level return/strict syntax errors, without eval.
    s.budget();
    s.ctx.with(|c| {
        rquickjs::Module::declare(c.clone(), "authored.js", source)
            .map(|_| ())
            .map_err(|error| {
                let exception = c.catch();
                let object = exception.as_object();
                let message = object
                    .and_then(|o| o.get::<_, String>("message").ok())
                    .unwrap_or_else(|| error.to_string());
                let mut diagnostic =
                    Diagnostic::new("validation_failed", message).at(key, "source");
                diagnostic.line = object.and_then(|o| o.get::<_, u64>("lineNumber").ok());
                diagnostic.column = object.and_then(|o| o.get::<_, u64>("columnNumber").ok());
                if let Some(stack) = object.and_then(|o| o.get::<_, String>("stack").ok()) {
                    if let Some(location) = stack.split("authored.js:").nth(1) {
                        let mut numbers = location.split(|c: char| !c.is_ascii_digit());
                        diagnostic.line = numbers
                            .next()
                            .and_then(|n| n.parse().ok())
                            .or(diagnostic.line);
                        diagnostic.column = numbers
                            .next()
                            .and_then(|n| n.parse().ok())
                            .or(diagnostic.column);
                    }
                }
                diagnostic
            })
    })?;
    // Function construction rejects module-only syntax (imports/exports/top-level await).
    // Neither parsing step executes authored startup hooks.
    trusted(
        &s,
        key,
        "source",
        &format!("new Function({});", json!(source)),
    )
}
