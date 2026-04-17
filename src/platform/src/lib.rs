//! Platform crate for OS, path, environment, and artifact abstractions.

use std::{
    collections::{BTreeSet, HashMap},
    error::Error,
    fmt, fs,
    path::{Path, PathBuf},
};

use matlab_codegen::{
    BackendKind, BytecodeClass, BytecodeExternalMethod, BytecodeFunction, BytecodeInstruction,
    BytecodeModule,
};

pub const CRATE_NAME: &str = "matlab-platform";
pub const BYTECODE_ARTIFACT_MAGIC: &str = "MATC-BYTECODE";
pub const BYTECODE_ARTIFACT_VERSION: &str = "1";
pub const BYTECODE_BUNDLE_MAGIC: &str = "MATC-BUNDLE";
pub const BYTECODE_BUNDLE_VERSION: &str = "1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlatformError {
    Io(String),
    Parse(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BytecodeBundle {
    pub root_source_path: String,
    pub root_module: BytecodeModule,
    pub dependency_modules: Vec<PackagedBytecodeModule>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackagedBytecodeModule {
    pub module_id: String,
    pub source_path: String,
    pub module: BytecodeModule,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BundleSummary {
    pub dependency_modules: usize,
    pub total_modules: usize,
    pub total_functions: usize,
    pub total_instructions: usize,
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) | Self::Parse(message) => f.write_str(message),
        }
    }
}

impl Error for PlatformError {}

pub fn summary() -> &'static str {
    "Owns filesystem, environment, cross-platform support, and serialized backend artifacts."
}

pub fn summarize_bundle(bundle: &BytecodeBundle) -> BundleSummary {
    let total_modules = 1 + bundle.dependency_modules.len();
    let mut modules = Vec::with_capacity(total_modules);
    modules.push(&bundle.root_module);
    modules.extend(
        bundle
            .dependency_modules
            .iter()
            .map(|module| &module.module),
    );
    BundleSummary {
        dependency_modules: bundle.dependency_modules.len(),
        total_modules,
        total_functions: modules.iter().map(|module| module.functions.len()).sum(),
        total_instructions: modules
            .iter()
            .flat_map(|module| &module.functions)
            .map(|function| function.instructions.len())
            .sum(),
    }
}

pub fn render_bundle_summary(bundle: &BytecodeBundle) -> String {
    let summary = summarize_bundle(bundle);
    format!(
        "bundle\n  root_source_path = {}\n  total_modules = {}\n  dependency_modules = {}\n  total_functions = {}\n  total_instructions = {}\n",
        bundle.root_source_path,
        summary.total_modules,
        summary.dependency_modules,
        summary.total_functions,
        summary.total_instructions
    )
}

pub fn rewrite_bytecode_bundle_targets(
    module: &BytecodeModule,
    path_to_module_id: &HashMap<PathBuf, String>,
) -> BytecodeModule {
    let mut rewritten = module.clone();
    for class in &mut rewritten.classes {
        if let Some(path) = class.superclass_path.as_ref().map(PathBuf::from) {
            if let Some(module_id) = path_to_module_id.get(&path) {
                class.superclass_bundle_module_id = Some(module_id.clone());
            }
        }
        for method in &mut class.external_methods {
            if let Some(path) = method.path.as_ref().map(PathBuf::from) {
                if let Some(module_id) = path_to_module_id.get(&path) {
                    method.bundle_module_id = Some(module_id.clone());
                }
            }
        }
    }
    for function in &mut rewritten.functions {
        for instruction in &mut function.instructions {
            match instruction {
                BytecodeInstruction::Call { target, .. }
                | BytecodeInstruction::MakeHandle { target, .. } => {
                    if let Some(path) = parse_resolved_path(target) {
                        if let Some(module_id) = path_to_module_id.get(&path) {
                            *target = attach_bundle_module_id(target, module_id);
                        }
                    }
                }
                _ => {}
            }
        }
    }
    rewritten
}

pub fn attach_bundle_module_id(target: &str, module_id: &str) -> String {
    if let Some(existing) = parse_bundle_module_id(target) {
        if existing == module_id {
            return target.to_string();
        }
        let marker = format!("bundle_id={existing}");
        return target.replacen(&marker, &format!("bundle_id={module_id}"), 1);
    }
    format!("{target} [bundle_id={module_id}]")
}

pub fn encode_bytecode_module(module: &BytecodeModule) -> String {
    let mut out = String::new();
    out.push_str(BYTECODE_ARTIFACT_MAGIC);
    out.push('\t');
    out.push_str(BYTECODE_ARTIFACT_VERSION);
    out.push('\n');
    push_record(
        &mut out,
        "MODULE",
        &[
            module.backend.as_str().to_string(),
            module.unit_kind.clone(),
            module.entry.clone(),
        ],
    );
    for class in &module.classes {
        let mut fields = vec![
            class.name.clone(),
            class.package.clone().unwrap_or_default(),
            class.superclass_name.clone().unwrap_or_default(),
            class.superclass_path.clone().unwrap_or_default(),
            class.superclass_bundle_module_id.clone().unwrap_or_default(),
            class.inherits_handle.to_string(),
            class.source_path.clone().unwrap_or_default(),
            class.default_initializer.clone().unwrap_or_default(),
            class.constructor.clone().unwrap_or_default(),
            class.property_names.len().to_string(),
        ];
        fields.extend(class.property_names.iter().cloned());
        fields.push(class.private_property_names.len().to_string());
        fields.extend(class.private_property_names.iter().cloned());
        fields.push(class.inline_methods.len().to_string());
        fields.extend(class.inline_methods.iter().cloned());
        fields.push(class.static_inline_methods.len().to_string());
        fields.extend(class.static_inline_methods.iter().cloned());
        fields.push(class.private_inline_methods.len().to_string());
        fields.extend(class.private_inline_methods.iter().cloned());
        fields.push(class.private_static_inline_methods.len().to_string());
        fields.extend(class.private_static_inline_methods.iter().cloned());
        fields.push(class.external_methods.len().to_string());
        for method in &class.external_methods {
            fields.push(method.name.clone());
            fields.push(method.path.clone().unwrap_or_default());
            fields.push(method.bundle_module_id.clone().unwrap_or_default());
        }
        push_record(&mut out, "CLASS", &fields);
    }
    for function in &module.functions {
        push_record(
            &mut out,
            "FUNCTION",
            &[
                function.name.clone(),
                function.role.clone(),
                function.owner_class_name.clone().unwrap_or_default(),
                function.temp_count.to_string(),
                function.label_count.to_string(),
            ],
        );
        for param in &function.params {
            push_record(&mut out, "PARAM", std::slice::from_ref(param));
        }
        for output in &function.outputs {
            push_record(&mut out, "OUTPUT", std::slice::from_ref(output));
        }
        for capture in &function.captures {
            push_record(&mut out, "CAPTURE", std::slice::from_ref(capture));
        }
        for instruction in &function.instructions {
            push_instruction(&mut out, instruction);
        }
        out.push_str("END_FUNCTION\n");
    }
    out
}

pub fn decode_bytecode_module(source: &str) -> Result<BytecodeModule, PlatformError> {
    let lines = source
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| (index + 1, line.to_string()))
        .collect::<Vec<_>>();

    let mut cursor = 0usize;

    let (header_line, header) = lines
        .get(cursor)
        .ok_or_else(|| PlatformError::Parse("bytecode artifact is empty".to_string()))?;
    let header_fields = parse_fields(header, *header_line)?;
    if header_fields.len() != 2
        || header_fields[0] != BYTECODE_ARTIFACT_MAGIC
        || header_fields[1] != BYTECODE_ARTIFACT_VERSION
    {
        return Err(PlatformError::Parse(format!(
            "line {}: expected artifact header `{}` version `{}`",
            header_line, BYTECODE_ARTIFACT_MAGIC, BYTECODE_ARTIFACT_VERSION
        )));
    }
    cursor += 1;

    let (module_line, module_record) = lines.get(cursor).ok_or_else(|| {
        PlatformError::Parse("bytecode artifact is missing the MODULE record".to_string())
    })?;
    let module_fields = parse_fields(module_record, *module_line)?;
    if module_fields.first().map(String::as_str) != Some("MODULE") {
        return Err(PlatformError::Parse(format!(
            "line {}: expected MODULE record",
            module_line
        )));
    }
    if module_fields.len() != 4 {
        return Err(PlatformError::Parse(format!(
            "line {}: MODULE record must have 3 fields",
            module_line
        )));
    }

    let backend = parse_backend(&module_fields[1], *module_line)?;
    let unit_kind = module_fields[2].clone();
    let entry = module_fields[3].clone();
    cursor += 1;

    let mut classes = Vec::new();
    let mut functions = Vec::new();

    while let Some((line_index, record)) = lines.get(cursor) {
        let fields = parse_fields(record, *line_index)?;
        match fields.first().map(String::as_str) {
            Some("CLASS") => {
                classes.push(parse_class(fields, *line_index)?);
                cursor += 1;
            }
            Some("FUNCTION") => {
                let function = parse_function(&lines, &mut cursor, fields, *line_index)?;
                functions.push(function);
            }
            Some(other) => {
                return Err(PlatformError::Parse(format!(
                    "line {}: unexpected record `{other}`",
                    line_index
                )))
            }
            None => {}
        }
    }

    Ok(BytecodeModule {
        backend,
        unit_kind,
        entry,
        classes,
        functions,
    })
}

pub fn write_bytecode_artifact(path: &Path, module: &BytecodeModule) -> Result<(), PlatformError> {
    let encoded = encode_bytecode_module(module);
    fs::write(path, encoded).map_err(|error| {
        PlatformError::Io(format!(
            "failed to write bytecode artifact `{}`: {error}",
            path.display()
        ))
    })
}

pub fn read_bytecode_artifact(path: &Path) -> Result<BytecodeModule, PlatformError> {
    let source = fs::read_to_string(path).map_err(|error| {
        PlatformError::Io(format!(
            "failed to read bytecode artifact `{}`: {error}",
            path.display()
        ))
    })?;
    decode_bytecode_module(&source)
}

pub fn encode_bytecode_bundle(bundle: &BytecodeBundle) -> String {
    let mut out = String::new();
    out.push_str(BYTECODE_BUNDLE_MAGIC);
    out.push('\t');
    out.push_str(BYTECODE_BUNDLE_VERSION);
    out.push('\n');
    push_record(
        &mut out,
        "ROOT_SOURCE",
        std::slice::from_ref(&bundle.root_source_path),
    );
    push_record(
        &mut out,
        "ROOT_MODULE",
        &[encode_bytecode_module(&bundle.root_module)],
    );
    for module in &bundle.dependency_modules {
        push_record(
            &mut out,
            "DEPENDENCY_MODULE",
            &[
                module.module_id.clone(),
                module.source_path.clone(),
                encode_bytecode_module(&module.module),
            ],
        );
    }
    out
}

pub fn decode_bytecode_bundle(source: &str) -> Result<BytecodeBundle, PlatformError> {
    let lines = source
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .map(|(index, line)| (index + 1, line.to_string()))
        .collect::<Vec<_>>();

    let mut cursor = 0usize;
    let (header_line, header) = lines
        .get(cursor)
        .ok_or_else(|| PlatformError::Parse("bytecode bundle is empty".to_string()))?;
    let header_fields = parse_fields(header, *header_line)?;
    if header_fields.len() != 2
        || header_fields[0] != BYTECODE_BUNDLE_MAGIC
        || header_fields[1] != BYTECODE_BUNDLE_VERSION
    {
        return Err(PlatformError::Parse(format!(
            "line {}: expected bundle header `{}` version `{}`",
            header_line, BYTECODE_BUNDLE_MAGIC, BYTECODE_BUNDLE_VERSION
        )));
    }
    cursor += 1;

    let (root_source_line, root_source_record) = lines.get(cursor).ok_or_else(|| {
        PlatformError::Parse("bytecode bundle is missing the ROOT_SOURCE record".to_string())
    })?;
    let root_source_fields = parse_fields(root_source_record, *root_source_line)?;
    if root_source_fields.first().map(String::as_str) != Some("ROOT_SOURCE")
        || root_source_fields.len() != 2
    {
        return Err(PlatformError::Parse(format!(
            "line {}: expected ROOT_SOURCE record with 1 field",
            root_source_line
        )));
    }
    let root_source_path = root_source_fields[1].clone();
    cursor += 1;

    let (root_module_line, root_module_record) = lines.get(cursor).ok_or_else(|| {
        PlatformError::Parse("bytecode bundle is missing the ROOT_MODULE record".to_string())
    })?;
    let root_module_fields = parse_fields(root_module_record, *root_module_line)?;
    if root_module_fields.first().map(String::as_str) != Some("ROOT_MODULE")
        || root_module_fields.len() != 2
    {
        return Err(PlatformError::Parse(format!(
            "line {}: expected ROOT_MODULE record with 1 field",
            root_module_line
        )));
    }
    let root_module = decode_bytecode_module(&root_module_fields[1])?;
    cursor += 1;

    let mut dependency_modules = Vec::new();
    while let Some((line_index, record)) = lines.get(cursor) {
        let fields = parse_fields(record, *line_index)?;
        match fields.first().map(String::as_str) {
            Some("DEPENDENCY_MODULE") => {
                if fields.len() != 4 {
                    return Err(PlatformError::Parse(format!(
                        "line {}: DEPENDENCY_MODULE record must have 3 fields",
                        line_index
                    )));
                }
                dependency_modules.push(PackagedBytecodeModule {
                    module_id: fields[1].clone(),
                    source_path: fields[2].clone(),
                    module: decode_bytecode_module(&fields[3])?,
                });
            }
            Some(other) => {
                return Err(PlatformError::Parse(format!(
                    "line {}: unexpected record `{other}`",
                    line_index
                )));
            }
            None => {}
        }
        cursor += 1;
    }

    Ok(BytecodeBundle {
        root_source_path,
        root_module,
        dependency_modules,
    })
}

pub fn write_bytecode_bundle(path: &Path, bundle: &BytecodeBundle) -> Result<(), PlatformError> {
    let encoded = encode_bytecode_bundle(bundle);
    fs::write(path, encoded).map_err(|error| {
        PlatformError::Io(format!(
            "failed to write bytecode bundle `{}`: {error}",
            path.display()
        ))
    })
}

pub fn read_bytecode_bundle(path: &Path) -> Result<BytecodeBundle, PlatformError> {
    let source = fs::read_to_string(path).map_err(|error| {
        PlatformError::Io(format!(
            "failed to read bytecode bundle `{}`: {error}",
            path.display()
        ))
    })?;
    decode_bytecode_bundle(&source)
}

pub fn collect_bytecode_dependency_paths(module: &BytecodeModule) -> Vec<PathBuf> {
    let mut paths = BTreeSet::new();
    for class in &module.classes {
        if let Some(path) = &class.superclass_path {
            paths.insert(PathBuf::from(path));
        }
        for method in &class.external_methods {
            if let Some(path) = &method.path {
                paths.insert(PathBuf::from(path));
            }
        }
    }
    for function in &module.functions {
        for instruction in &function.instructions {
            match instruction {
                BytecodeInstruction::Call { target, .. }
                | BytecodeInstruction::MakeHandle { target, .. } => {
                    if let Some(path) = parse_resolved_path(target) {
                        paths.insert(path);
                    }
                }
                _ => {}
            }
        }
    }
    paths.into_iter().collect()
}

fn parse_bundle_module_id(value: &str) -> Option<String> {
    value
        .split("bundle_id=")
        .nth(1)
        .map(|rest| rest.split([' ', ']']).next().unwrap_or(rest).to_string())
}

fn push_record(out: &mut String, tag: &str, fields: &[String]) {
    out.push_str(tag);
    for field in fields {
        out.push('\t');
        out.push_str(&escape_text(field));
    }
    out.push('\n');
}

fn push_instruction(out: &mut String, instruction: &BytecodeInstruction) {
    let mut fields = vec!["INSTR".to_string()];
    match instruction {
        BytecodeInstruction::Label(label) => {
            fields.push("Label".to_string());
            fields.push(label.to_string());
        }
        BytecodeInstruction::LoadConst { dst, value } => {
            fields.push("LoadConst".to_string());
            fields.push(dst.to_string());
            fields.push(value.clone());
        }
        BytecodeInstruction::LoadBinding { dst, binding } => {
            fields.push("LoadBinding".to_string());
            fields.push(dst.to_string());
            fields.push(binding.clone());
        }
        BytecodeInstruction::LoadBindingLValue { dst, binding } => {
            fields.push("LoadBindingLValue".to_string());
            fields.push(dst.to_string());
            fields.push(binding.clone());
        }
        BytecodeInstruction::StoreBinding { binding, src } => {
            fields.push("StoreBinding".to_string());
            fields.push(binding.clone());
            fields.push(src.to_string());
        }
        BytecodeInstruction::StoreBindingIfPresent { binding, src } => {
            fields.push("StoreBindingIfPresent".to_string());
            fields.push(binding.clone());
            fields.push(src.to_string());
        }
        BytecodeInstruction::Unary { dst, op, src } => {
            fields.push("Unary".to_string());
            fields.push(dst.to_string());
            fields.push(op.clone());
            fields.push(src.to_string());
        }
        BytecodeInstruction::Binary { dst, op, lhs, rhs } => {
            fields.push("Binary".to_string());
            fields.push(dst.to_string());
            fields.push(op.clone());
            fields.push(lhs.to_string());
            fields.push(rhs.to_string());
        }
        BytecodeInstruction::BuildMatrix {
            dst,
            rows,
            cols,
            elements,
        } => {
            fields.push("BuildMatrix".to_string());
            fields.push(dst.to_string());
            fields.push(rows.to_string());
            fields.push(cols.to_string());
            fields.push(elements.len().to_string());
            fields.extend(elements.iter().map(ToString::to_string));
        }
        BytecodeInstruction::BuildMatrixList {
            dst,
            row_item_counts,
            elements,
        } => {
            fields.push("BuildMatrixList".to_string());
            fields.push(dst.to_string());
            fields.push(row_item_counts.len().to_string());
            fields.extend(row_item_counts.iter().map(ToString::to_string));
            fields.push(elements.len().to_string());
            fields.extend(elements.iter().cloned());
        }
        BytecodeInstruction::BuildCell {
            dst,
            rows,
            cols,
            elements,
        } => {
            fields.push("BuildCell".to_string());
            fields.push(dst.to_string());
            fields.push(rows.to_string());
            fields.push(cols.to_string());
            fields.push(elements.len().to_string());
            fields.extend(elements.iter().map(ToString::to_string));
        }
        BytecodeInstruction::BuildCellList {
            dst,
            row_item_counts,
            elements,
        } => {
            fields.push("BuildCellList".to_string());
            fields.push(dst.to_string());
            fields.push(row_item_counts.len().to_string());
            fields.extend(row_item_counts.iter().map(ToString::to_string));
            fields.push(elements.len().to_string());
            fields.extend(elements.iter().cloned());
        }
        BytecodeInstruction::PackSpreadMatrix { dst, src } => {
            fields.push("PackSpreadMatrix".to_string());
            fields.push(dst.to_string());
            fields.push(src.to_string());
        }
        BytecodeInstruction::PackSpreadCell { dst, src } => {
            fields.push("PackSpreadCell".to_string());
            fields.push(dst.to_string());
            fields.push(src.to_string());
        }
        BytecodeInstruction::MakeHandle { dst, target } => {
            fields.push("MakeHandle".to_string());
            fields.push(dst.to_string());
            fields.push(target.clone());
        }
        BytecodeInstruction::Range {
            dst,
            start,
            step,
            end,
        } => {
            fields.push("Range".to_string());
            fields.push(dst.to_string());
            fields.push(start.to_string());
            fields.push(
                step.map(|value| value.to_string())
                    .unwrap_or_else(|| "_".to_string()),
            );
            fields.push(end.to_string());
        }
        BytecodeInstruction::Call {
            outputs,
            target,
            args,
        } => {
            fields.push("Call".to_string());
            fields.push(outputs.len().to_string());
            fields.extend(outputs.iter().map(ToString::to_string));
            fields.push(target.clone());
            fields.push(args.len().to_string());
            fields.extend(args.iter().cloned());
        }
        BytecodeInstruction::LoadIndex {
            dst,
            target,
            kind,
            args,
        } => {
            fields.push("LoadIndex".to_string());
            fields.push(dst.to_string());
            fields.push(target.to_string());
            fields.push((*kind).to_string());
            fields.push(args.len().to_string());
            fields.extend(args.iter().cloned());
        }
        BytecodeInstruction::LoadIndexList {
            dst,
            target,
            kind,
            args,
        } => {
            fields.push("LoadIndexList".to_string());
            fields.push(dst.to_string());
            fields.push(target.to_string());
            fields.push((*kind).to_string());
            fields.push(args.len().to_string());
            fields.extend(args.iter().cloned());
        }
        BytecodeInstruction::StoreIndex {
            target,
            kind,
            args,
            src,
        } => {
            fields.push("StoreIndex".to_string());
            fields.push(target.to_string());
            fields.push((*kind).to_string());
            fields.push(args.len().to_string());
            fields.extend(args.iter().cloned());
            fields.push(src.to_string());
        }
        BytecodeInstruction::LoadField { dst, target, field } => {
            fields.push("LoadField".to_string());
            fields.push(dst.to_string());
            fields.push(target.to_string());
            fields.push(field.clone());
        }
        BytecodeInstruction::LoadFieldList { dst, target, field } => {
            fields.push("LoadFieldList".to_string());
            fields.push(dst.to_string());
            fields.push(target.to_string());
            fields.push(field.clone());
        }
        BytecodeInstruction::StoreField {
            target,
            field,
            src,
            list_assignment,
        } => {
            fields.push("StoreField".to_string());
            fields.push(target.to_string());
            fields.push(field.clone());
            fields.push(src.to_string());
            fields.push(if *list_assignment { "1" } else { "0" }.to_string());
        }
        BytecodeInstruction::SplitList { outputs, src } => {
            fields.push("SplitList".to_string());
            fields.push(outputs.len().to_string());
            fields.extend(outputs.iter().map(ToString::to_string));
            fields.push(src.to_string());
        }
        BytecodeInstruction::PushTry { catch } => {
            fields.push("PushTry".to_string());
            fields.push(catch.to_string());
        }
        BytecodeInstruction::StoreLastError { binding } => {
            fields.push("StoreLastError".to_string());
            fields.push(binding.clone());
        }
        BytecodeInstruction::JumpIfFalse { condition, target } => {
            fields.push("JumpIfFalse".to_string());
            fields.push(condition.to_string());
            fields.push(target.to_string());
        }
        BytecodeInstruction::Jump { target } => {
            fields.push("Jump".to_string());
            fields.push(target.to_string());
        }
        BytecodeInstruction::IterStart { iter, source } => {
            fields.push("IterStart".to_string());
            fields.push(iter.to_string());
            fields.push(source.to_string());
        }
        BytecodeInstruction::IterHasNext { dst, iter } => {
            fields.push("IterHasNext".to_string());
            fields.push(dst.to_string());
            fields.push(iter.to_string());
        }
        BytecodeInstruction::IterNext { dst, iter } => {
            fields.push("IterNext".to_string());
            fields.push(dst.to_string());
            fields.push(iter.to_string());
        }
        BytecodeInstruction::DeclareGlobal { bindings } => {
            fields.push("DeclareGlobal".to_string());
            fields.push(bindings.len().to_string());
            fields.extend(bindings.iter().cloned());
        }
        BytecodeInstruction::DeclarePersistent { bindings } => {
            fields.push("DeclarePersistent".to_string());
            fields.push(bindings.len().to_string());
            fields.extend(bindings.iter().cloned());
        }
        BytecodeInstruction::Return { values } => {
            fields.push("Return".to_string());
            fields.push(values.len().to_string());
            fields.extend(values.iter().map(ToString::to_string));
        }
    }
    push_record(out, &fields[0], &fields[1..]);
}

fn parse_function(
    lines: &[(usize, String)],
    cursor: &mut usize,
    fields: Vec<String>,
    line_number: usize,
) -> Result<BytecodeFunction, PlatformError> {
    if fields.len() != 6 {
        return Err(PlatformError::Parse(format!(
            "line {line_number}: FUNCTION record must have 5 fields"
        )));
    }

    let name = fields[1].clone();
    let role = fields[2].clone();
    let owner_class_name = (!fields[3].is_empty()).then(|| fields[3].clone());
    let temp_count = parse_u32(&fields[4], "temp count", line_number)?;
    let label_count = parse_u32(&fields[5], "label count", line_number)?;
    let mut function = BytecodeFunction {
        name,
        role,
        owner_class_name,
        params: Vec::new(),
        outputs: Vec::new(),
        captures: Vec::new(),
        temp_count,
        label_count,
        instructions: Vec::new(),
    };

    *cursor += 1;
    while let Some((record_line, record)) = lines.get(*cursor) {
        let fields = parse_fields(record, *record_line)?;
        match fields.first().map(String::as_str) {
            Some("PARAM") => function
                .params
                .push(exact_field(&fields, 1, "PARAM", *record_line)?),
            Some("OUTPUT") => {
                function
                    .outputs
                    .push(exact_field(&fields, 1, "OUTPUT", *record_line)?)
            }
            Some("CAPTURE") => {
                function
                    .captures
                    .push(exact_field(&fields, 1, "CAPTURE", *record_line)?)
            }
            Some("INSTR") => function
                .instructions
                .push(parse_instruction(&fields, *record_line)?),
            Some("END_FUNCTION") => {
                if fields.len() != 1 {
                    return Err(PlatformError::Parse(format!(
                        "line {}: END_FUNCTION does not take fields",
                        *record_line
                    )));
                }
                *cursor += 1;
                return Ok(function);
            }
            Some(other) => {
                return Err(PlatformError::Parse(format!(
                    "line {}: unexpected record `{other}` inside function",
                    *record_line
                )))
            }
            None => {}
        }
        *cursor += 1;
    }

    Err(PlatformError::Parse(format!(
        "line {line_number}: function `{}` is missing END_FUNCTION",
        function.name
    )))
}

fn parse_class(fields: Vec<String>, line_number: usize) -> Result<BytecodeClass, PlatformError> {
    if fields.len() < 11 {
        return Err(PlatformError::Parse(format!(
            "line {line_number}: CLASS record is too short"
        )));
    }
    let name = fields[1].clone();
    let package = (!fields[2].is_empty()).then(|| fields[2].clone());
    let superclass_name = (!fields[3].is_empty()).then(|| fields[3].clone());
    let superclass_path = (!fields[4].is_empty()).then(|| fields[4].clone());
    let superclass_bundle_module_id = (!fields[5].is_empty()).then(|| fields[5].clone());
    let inherits_handle = parse_bool(&fields[6], line_number)?;
    let source_path = (!fields[7].is_empty()).then(|| fields[7].clone());
    let default_initializer = (!fields[8].is_empty()).then(|| fields[8].clone());
    let constructor = (!fields[9].is_empty()).then(|| fields[9].clone());
    let property_count = parse_usize(&fields[10], "class property count", line_number)?;
    let property_start = 11;
    let property_end = property_start + property_count;
    if fields.len() < property_end + 1 {
        return Err(PlatformError::Parse(format!(
            "line {line_number}: CLASS record is missing private property count"
        )));
    }
    let property_names = fields[property_start..property_end].to_vec();
    let private_property_count =
        parse_usize(&fields[property_end], "class private property count", line_number)?;
    let private_property_start = property_end + 1;
    let private_property_end = private_property_start + private_property_count;
    if fields.len() < private_property_end + 1 {
        return Err(PlatformError::Parse(format!(
            "line {line_number}: CLASS record is missing inline method count"
        )));
    }
    let private_property_names = fields[private_property_start..private_property_end].to_vec();
    let method_count = parse_usize(
        &fields[private_property_end],
        "class inline method count",
        line_number,
    )?;
    let methods_start = private_property_end + 1;
    let methods_end = methods_start + method_count;
    if fields.len() < methods_end + 1 {
        return Err(PlatformError::Parse(format!(
            "line {line_number}: CLASS record is missing static inline method count"
        )));
    }
    let inline_methods = fields[methods_start..methods_end].to_vec();
    let static_count = parse_usize(
        &fields[methods_end],
        "class static inline method count",
        line_number,
    )?;
    let static_start = methods_end + 1;
    let static_end = static_start + static_count;
    if fields.len() < static_end + 1 {
        return Err(PlatformError::Parse(format!(
            "line {line_number}: CLASS record is missing external method count"
        )));
    }
    let static_inline_methods = fields[static_start..static_end].to_vec();
    if fields.len() < static_end + 1 {
        return Err(PlatformError::Parse(format!(
            "line {line_number}: CLASS record is missing private inline method count"
        )));
    }
    let private_inline_count = parse_usize(
        &fields[static_end],
        "class private inline method count",
        line_number,
    )?;
    let private_inline_start = static_end + 1;
    let private_inline_end = private_inline_start + private_inline_count;
    if fields.len() < private_inline_end + 1 {
        return Err(PlatformError::Parse(format!(
            "line {line_number}: CLASS record is missing private static inline method count"
        )));
    }
    let private_inline_methods = fields[private_inline_start..private_inline_end].to_vec();
    let private_static_count = parse_usize(
        &fields[private_inline_end],
        "class private static inline method count",
        line_number,
    )?;
    let private_static_start = private_inline_end + 1;
    let private_static_end = private_static_start + private_static_count;
    if fields.len() < private_static_end + 1 {
        return Err(PlatformError::Parse(format!(
            "line {line_number}: CLASS record is missing external method count"
        )));
    }
    let private_static_inline_methods =
        fields[private_static_start..private_static_end].to_vec();
    let external_count = parse_usize(
        &fields[private_static_end],
        "class external method count",
        line_number,
    )?;
    let external_start = private_static_end + 1;
    let external_end = external_start + external_count * 3;
    if fields.len() != external_end {
        return Err(PlatformError::Parse(format!(
            "line {line_number}: CLASS record external method payload does not match count"
        )));
    }
    let mut external_methods = Vec::new();
    let mut cursor = external_start;
    while cursor < external_end {
        external_methods.push(BytecodeExternalMethod {
            name: fields[cursor].clone(),
            path: (!fields[cursor + 1].is_empty()).then(|| fields[cursor + 1].clone()),
            bundle_module_id: (!fields[cursor + 2].is_empty()).then(|| fields[cursor + 2].clone()),
        });
        cursor += 3;
    }
    Ok(BytecodeClass {
        name,
        package,
        superclass_name,
        superclass_path,
        superclass_bundle_module_id,
        inherits_handle,
        source_path,
        property_names,
        private_property_names,
        default_initializer,
        constructor,
        inline_methods,
        static_inline_methods,
        private_inline_methods,
        private_static_inline_methods,
        external_methods,
    })
}

fn parse_instruction(
    fields: &[String],
    line_number: usize,
) -> Result<BytecodeInstruction, PlatformError> {
    let opcode = fields.get(1).ok_or_else(|| {
        PlatformError::Parse(format!(
            "line {line_number}: INSTR record is missing an opcode"
        ))
    })?;
    match opcode.as_str() {
        "Label" => Ok(BytecodeInstruction::Label(parse_label(
            exact_slice(fields, 3, "Label", line_number)?[0].as_str(),
            line_number,
        )?)),
        "LoadConst" => {
            let fields = exact_slice(fields, 4, "LoadConst", line_number)?;
            Ok(BytecodeInstruction::LoadConst {
                dst: parse_temp(&fields[0], line_number)?,
                value: fields[1].clone(),
            })
        }
        "LoadBinding" => {
            let fields = exact_slice(fields, 4, "LoadBinding", line_number)?;
            Ok(BytecodeInstruction::LoadBinding {
                dst: parse_temp(&fields[0], line_number)?,
                binding: fields[1].clone(),
            })
        }
        "LoadBindingLValue" => {
            let fields = exact_slice(fields, 4, "LoadBindingLValue", line_number)?;
            Ok(BytecodeInstruction::LoadBindingLValue {
                dst: parse_temp(&fields[0], line_number)?,
                binding: fields[1].clone(),
            })
        }
        "StoreBinding" => {
            let fields = exact_slice(fields, 4, "StoreBinding", line_number)?;
            Ok(BytecodeInstruction::StoreBinding {
                binding: fields[0].clone(),
                src: parse_temp(&fields[1], line_number)?,
            })
        }
        "StoreBindingIfPresent" => {
            let fields = exact_slice(fields, 4, "StoreBindingIfPresent", line_number)?;
            Ok(BytecodeInstruction::StoreBindingIfPresent {
                binding: fields[0].clone(),
                src: parse_temp(&fields[1], line_number)?,
            })
        }
        "Unary" => {
            let fields = exact_slice(fields, 5, "Unary", line_number)?;
            Ok(BytecodeInstruction::Unary {
                dst: parse_temp(&fields[0], line_number)?,
                op: fields[1].clone(),
                src: parse_temp(&fields[2], line_number)?,
            })
        }
        "Binary" => {
            let fields = exact_slice(fields, 6, "Binary", line_number)?;
            Ok(BytecodeInstruction::Binary {
                dst: parse_temp(&fields[0], line_number)?,
                op: fields[1].clone(),
                lhs: parse_temp(&fields[2], line_number)?,
                rhs: parse_temp(&fields[3], line_number)?,
            })
        }
        "BuildMatrix" => parse_build(fields, line_number, true),
        "BuildMatrixList" => parse_build_list(fields, line_number, true),
        "BuildCell" => parse_build(fields, line_number, false),
        "BuildCellList" => parse_build_list(fields, line_number, false),
        "PackSpreadMatrix" => parse_pack_spread(fields, line_number, true),
        "PackSpreadCell" => parse_pack_spread(fields, line_number, false),
        "MakeHandle" => {
            let fields = exact_slice(fields, 4, "MakeHandle", line_number)?;
            Ok(BytecodeInstruction::MakeHandle {
                dst: parse_temp(&fields[0], line_number)?,
                target: fields[1].clone(),
            })
        }
        "Range" => {
            let fields = exact_slice(fields, 6, "Range", line_number)?;
            Ok(BytecodeInstruction::Range {
                dst: parse_temp(&fields[0], line_number)?,
                start: parse_temp(&fields[1], line_number)?,
                step: if fields[2] == "_" {
                    None
                } else {
                    Some(parse_temp(&fields[2], line_number)?)
                },
                end: parse_temp(&fields[3], line_number)?,
            })
        }
        "Call" => parse_call(fields, line_number),
        "LoadIndex" => parse_load_index(fields, line_number),
        "LoadIndexList" => parse_load_index_list(fields, line_number),
        "StoreIndex" => parse_store_index(fields, line_number),
        "LoadField" => {
            let fields = exact_slice(fields, 5, "LoadField", line_number)?;
            Ok(BytecodeInstruction::LoadField {
                dst: parse_temp(&fields[0], line_number)?,
                target: parse_temp(&fields[1], line_number)?,
                field: fields[2].clone(),
            })
        }
        "LoadFieldList" => {
            let fields = exact_slice(fields, 5, "LoadFieldList", line_number)?;
            Ok(BytecodeInstruction::LoadFieldList {
                dst: parse_temp(&fields[0], line_number)?,
                target: parse_temp(&fields[1], line_number)?,
                field: fields[2].clone(),
            })
        }
        "StoreField" => {
            let fields = exact_slice(fields, 6, "StoreField", line_number)?;
            Ok(BytecodeInstruction::StoreField {
                target: parse_temp(&fields[0], line_number)?,
                field: fields[1].clone(),
                src: parse_temp(&fields[2], line_number)?,
                list_assignment: parse_bool(&fields[3], line_number)?,
            })
        }
        "SplitList" => parse_split_list(fields, line_number),
        "PushTry" => {
            let fields = exact_slice(fields, 3, "PushTry", line_number)?;
            Ok(BytecodeInstruction::PushTry {
                catch: parse_label(&fields[0], line_number)?,
            })
        }
        "StoreLastError" => {
            let fields = exact_slice(fields, 3, "StoreLastError", line_number)?;
            Ok(BytecodeInstruction::StoreLastError {
                binding: fields[0].clone(),
            })
        }
        "JumpIfFalse" => {
            let fields = exact_slice(fields, 4, "JumpIfFalse", line_number)?;
            Ok(BytecodeInstruction::JumpIfFalse {
                condition: parse_temp(&fields[0], line_number)?,
                target: parse_label(&fields[1], line_number)?,
            })
        }
        "Jump" => {
            let fields = exact_slice(fields, 3, "Jump", line_number)?;
            Ok(BytecodeInstruction::Jump {
                target: parse_label(&fields[0], line_number)?,
            })
        }
        "IterStart" => {
            let fields = exact_slice(fields, 4, "IterStart", line_number)?;
            Ok(BytecodeInstruction::IterStart {
                iter: parse_temp(&fields[0], line_number)?,
                source: parse_temp(&fields[1], line_number)?,
            })
        }
        "IterHasNext" => {
            let fields = exact_slice(fields, 4, "IterHasNext", line_number)?;
            Ok(BytecodeInstruction::IterHasNext {
                dst: parse_temp(&fields[0], line_number)?,
                iter: parse_temp(&fields[1], line_number)?,
            })
        }
        "IterNext" => {
            let fields = exact_slice(fields, 4, "IterNext", line_number)?;
            Ok(BytecodeInstruction::IterNext {
                dst: parse_temp(&fields[0], line_number)?,
                iter: parse_temp(&fields[1], line_number)?,
            })
        }
        "DeclareGlobal" => parse_bindings_instruction(fields, line_number, true),
        "DeclarePersistent" => parse_bindings_instruction(fields, line_number, false),
        "Return" => {
            let values = parse_temp_vector(fields, line_number, "Return", 2)?;
            Ok(BytecodeInstruction::Return { values })
        }
        other => Err(PlatformError::Parse(format!(
            "line {line_number}: unknown instruction opcode `{other}`"
        ))),
    }
}

fn parse_build(
    fields: &[String],
    line_number: usize,
    matrix: bool,
) -> Result<BytecodeInstruction, PlatformError> {
    if fields.len() < 6 {
        let kind = if matrix { "BuildMatrix" } else { "BuildCell" };
        return Err(PlatformError::Parse(format!(
            "line {line_number}: {kind} requires at least 5 fields"
        )));
    }
    let dst = parse_temp(&fields[2], line_number)?;
    let rows = parse_usize(&fields[3], "rows", line_number)?;
    let cols = parse_usize(&fields[4], "cols", line_number)?;
    let count = parse_usize(&fields[5], "element count", line_number)?;
    if fields.len() != 6 + count {
        let kind = if matrix { "BuildMatrix" } else { "BuildCell" };
        return Err(PlatformError::Parse(format!(
            "line {line_number}: {kind} expected {count} element temp(s), got {}",
            fields.len().saturating_sub(6)
        )));
    }
    let elements = fields[6..]
        .iter()
        .map(|field| parse_temp(field, line_number))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(if matrix {
        BytecodeInstruction::BuildMatrix {
            dst,
            rows,
            cols,
            elements,
        }
    } else {
        BytecodeInstruction::BuildCell {
            dst,
            rows,
            cols,
            elements,
        }
    })
}

fn parse_build_list(
    fields: &[String],
    line_number: usize,
    matrix: bool,
) -> Result<BytecodeInstruction, PlatformError> {
    let kind = if matrix {
        "BuildMatrixList"
    } else {
        "BuildCellList"
    };
    if fields.len() < 5 {
        return Err(PlatformError::Parse(format!(
            "line {line_number}: {kind} requires at least 5 fields"
        )));
    }
    let dst = parse_temp(&fields[2], line_number)?;
    let row_count = parse_usize(&fields[3], "row count", line_number)?;
    if fields.len() < 5 + row_count {
        return Err(PlatformError::Parse(format!(
            "line {line_number}: {kind} is truncated before row item counts"
        )));
    }
    let row_item_counts = fields[4..4 + row_count]
        .iter()
        .map(|field| parse_usize(field, "row item count", line_number))
        .collect::<Result<Vec<_>, _>>()?;
    let element_count_index = 4 + row_count;
    let element_count = parse_usize(&fields[element_count_index], "element count", line_number)?;
    if fields.len() != element_count_index + 1 + element_count {
        return Err(PlatformError::Parse(format!(
            "line {line_number}: {kind} expected {element_count} element temp reference(s), got {}",
            fields.len().saturating_sub(element_count_index + 1)
        )));
    }
    let elements = fields[element_count_index + 1..].to_vec();
    Ok(if matrix {
        BytecodeInstruction::BuildMatrixList {
            dst,
            row_item_counts,
            elements,
        }
    } else {
        BytecodeInstruction::BuildCellList {
            dst,
            row_item_counts,
            elements,
        }
    })
}

fn parse_call(fields: &[String], line_number: usize) -> Result<BytecodeInstruction, PlatformError> {
    if fields.len() < 4 {
        return Err(PlatformError::Parse(format!(
            "line {line_number}: Call requires output and argument counts"
        )));
    }
    let output_count = parse_usize(&fields[2], "output count", line_number)?;
    let mut cursor = 3usize;
    if fields.len() < cursor + output_count + 2 {
        return Err(PlatformError::Parse(format!(
            "line {line_number}: Call record is truncated"
        )));
    }
    let outputs = fields[cursor..cursor + output_count]
        .iter()
        .map(|field| parse_temp(field, line_number))
        .collect::<Result<Vec<_>, _>>()?;
    cursor += output_count;
    let target = fields[cursor].clone();
    cursor += 1;
    let arg_count = parse_usize(&fields[cursor], "argument count", line_number)?;
    cursor += 1;
    if fields.len() != cursor + arg_count {
        return Err(PlatformError::Parse(format!(
            "line {line_number}: Call expected {arg_count} argument reference(s), got {}",
            fields.len().saturating_sub(cursor)
        )));
    }
    let args = fields[cursor..].to_vec();
    Ok(BytecodeInstruction::Call {
        outputs,
        target,
        args,
    })
}

fn parse_load_index(
    fields: &[String],
    line_number: usize,
) -> Result<BytecodeInstruction, PlatformError> {
    if fields.len() < 6 {
        return Err(PlatformError::Parse(format!(
            "line {line_number}: LoadIndex requires target, kind, and argument count"
        )));
    }
    let dst = parse_temp(&fields[2], line_number)?;
    let target = parse_temp(&fields[3], line_number)?;
    let kind = parse_index_kind(&fields[4], line_number)?;
    let arg_count = parse_usize(&fields[5], "index argument count", line_number)?;
    if fields.len() != 6 + arg_count {
        return Err(PlatformError::Parse(format!(
            "line {line_number}: LoadIndex expected {arg_count} argument reference(s), got {}",
            fields.len().saturating_sub(6)
        )));
    }
    Ok(BytecodeInstruction::LoadIndex {
        dst,
        target,
        kind,
        args: fields[6..].to_vec(),
    })
}

fn parse_load_index_list(
    fields: &[String],
    line_number: usize,
) -> Result<BytecodeInstruction, PlatformError> {
    if fields.len() < 6 {
        return Err(PlatformError::Parse(format!(
            "line {line_number}: LoadIndexList requires target, kind, and argument count"
        )));
    }
    let dst = parse_temp(&fields[2], line_number)?;
    let target = parse_temp(&fields[3], line_number)?;
    let kind = parse_index_kind(&fields[4], line_number)?;
    let arg_count = parse_usize(&fields[5], "index list argument count", line_number)?;
    if fields.len() != 6 + arg_count {
        return Err(PlatformError::Parse(format!(
            "line {line_number}: LoadIndexList expected {arg_count} argument reference(s), got {}",
            fields.len().saturating_sub(6)
        )));
    }
    Ok(BytecodeInstruction::LoadIndexList {
        dst,
        target,
        kind,
        args: fields[6..].to_vec(),
    })
}

fn parse_store_index(
    fields: &[String],
    line_number: usize,
) -> Result<BytecodeInstruction, PlatformError> {
    if fields.len() < 6 {
        return Err(PlatformError::Parse(format!(
            "line {line_number}: StoreIndex requires target, kind, argument count, and source"
        )));
    }
    let target = parse_temp(&fields[2], line_number)?;
    let kind = parse_index_kind(&fields[3], line_number)?;
    let arg_count = parse_usize(&fields[4], "index argument count", line_number)?;
    if fields.len() != 6 + arg_count {
        return Err(PlatformError::Parse(format!(
            "line {line_number}: StoreIndex expected {arg_count} argument reference(s) plus source, got {}",
            fields.len().saturating_sub(5)
        )));
    }
    let args_end = 5 + arg_count;
    Ok(BytecodeInstruction::StoreIndex {
        target,
        kind,
        args: fields[5..args_end].to_vec(),
        src: parse_temp(&fields[args_end], line_number)?,
    })
}

fn parse_split_list(
    fields: &[String],
    line_number: usize,
) -> Result<BytecodeInstruction, PlatformError> {
    if fields.len() < 4 {
        return Err(PlatformError::Parse(format!(
            "line {line_number}: SplitList requires output count and source"
        )));
    }
    let output_count = parse_usize(&fields[2], "split-list output count", line_number)?;
    if fields.len() != output_count + 4 {
        return Err(PlatformError::Parse(format!(
            "line {line_number}: SplitList expected {output_count} output temp(s) plus source, got {}",
            fields.len().saturating_sub(3)
        )));
    }
    let outputs = fields[3..3 + output_count]
        .iter()
        .map(|field| parse_temp(field, line_number))
        .collect::<Result<Vec<_>, _>>()?;
    let src = parse_temp(&fields[3 + output_count], line_number)?;
    Ok(BytecodeInstruction::SplitList { outputs, src })
}

fn parse_pack_spread(
    fields: &[String],
    line_number: usize,
    matrix: bool,
) -> Result<BytecodeInstruction, PlatformError> {
    let kind = if matrix {
        "PackSpreadMatrix"
    } else {
        "PackSpreadCell"
    };
    let fields = exact_slice(fields, 4, kind, line_number)?;
    let dst = parse_temp(&fields[0], line_number)?;
    let src = parse_temp(&fields[1], line_number)?;
    Ok(if matrix {
        BytecodeInstruction::PackSpreadMatrix { dst, src }
    } else {
        BytecodeInstruction::PackSpreadCell { dst, src }
    })
}

fn parse_bindings_instruction(
    fields: &[String],
    line_number: usize,
    global: bool,
) -> Result<BytecodeInstruction, PlatformError> {
    if fields.len() < 3 {
        let kind = if global {
            "DeclareGlobal"
        } else {
            "DeclarePersistent"
        };
        return Err(PlatformError::Parse(format!(
            "line {line_number}: {kind} requires a binding count"
        )));
    }
    let count = parse_usize(&fields[2], "binding count", line_number)?;
    if fields.len() != 3 + count {
        let kind = if global {
            "DeclareGlobal"
        } else {
            "DeclarePersistent"
        };
        return Err(PlatformError::Parse(format!(
            "line {line_number}: {kind} expected {count} binding name(s), got {}",
            fields.len().saturating_sub(3)
        )));
    }
    let bindings = fields[3..].to_vec();
    Ok(if global {
        BytecodeInstruction::DeclareGlobal { bindings }
    } else {
        BytecodeInstruction::DeclarePersistent { bindings }
    })
}

fn parse_temp_vector(
    fields: &[String],
    line_number: usize,
    opcode: &str,
    count_index: usize,
) -> Result<Vec<u32>, PlatformError> {
    let count = parse_usize(
        fields.get(count_index).ok_or_else(|| {
            PlatformError::Parse(format!(
                "line {line_number}: {opcode} is missing its temp count"
            ))
        })?,
        "temp count",
        line_number,
    )?;
    let values_start = count_index + 1;
    if fields.len() != values_start + count {
        return Err(PlatformError::Parse(format!(
            "line {line_number}: {opcode} expected {count} temp reference(s), got {}",
            fields.len().saturating_sub(values_start)
        )));
    }
    fields[values_start..]
        .iter()
        .map(|field| parse_temp(field, line_number))
        .collect()
}

fn exact_slice<'a>(
    fields: &'a [String],
    expected_len: usize,
    opcode: &str,
    line_number: usize,
) -> Result<&'a [String], PlatformError> {
    if fields.len() != expected_len {
        return Err(PlatformError::Parse(format!(
            "line {line_number}: {opcode} expected {} field(s), got {}",
            expected_len.saturating_sub(2),
            fields.len().saturating_sub(2)
        )));
    }
    Ok(&fields[2..])
}

fn exact_field(
    fields: &[String],
    expected_len: usize,
    record: &str,
    line_number: usize,
) -> Result<String, PlatformError> {
    if fields.len() != expected_len + 1 {
        return Err(PlatformError::Parse(format!(
            "line {line_number}: {record} expected {expected_len} field(s)"
        )));
    }
    Ok(fields[1].clone())
}

fn parse_index_kind(value: &str, line_number: usize) -> Result<&'static str, PlatformError> {
    match value {
        "paren" => Ok("paren"),
        "brace" => Ok("brace"),
        other => Err(PlatformError::Parse(format!(
            "line {line_number}: unsupported index kind `{other}`"
        ))),
    }
}

fn parse_temp(value: &str, line_number: usize) -> Result<u32, PlatformError> {
    parse_u32(value, "temp reference", line_number)
}

fn parse_label(value: &str, line_number: usize) -> Result<u32, PlatformError> {
    parse_u32(value, "label", line_number)
}

fn parse_u32(value: &str, kind: &str, line_number: usize) -> Result<u32, PlatformError> {
    value.parse::<u32>().map_err(|error| {
        PlatformError::Parse(format!(
            "line {line_number}: invalid {kind} `{value}`: {error}"
        ))
    })
}

fn parse_usize(value: &str, kind: &str, line_number: usize) -> Result<usize, PlatformError> {
    value.parse::<usize>().map_err(|error| {
        PlatformError::Parse(format!(
            "line {line_number}: invalid {kind} `{value}`: {error}"
        ))
    })
}

fn parse_bool(value: &str, line_number: usize) -> Result<bool, PlatformError> {
    match value {
        "0" | "false" => Ok(false),
        "1" | "true" => Ok(true),
        _ => Err(PlatformError::Parse(format!(
            "line {line_number}: invalid boolean flag `{value}`"
        ))),
    }
}

fn parse_backend(value: &str, line_number: usize) -> Result<BackendKind, PlatformError> {
    match value {
        "bytecode" => Ok(BackendKind::Bytecode),
        "c" => Ok(BackendKind::C),
        "llvm" => Ok(BackendKind::Llvm),
        other => Err(PlatformError::Parse(format!(
            "line {line_number}: unknown backend `{other}`"
        ))),
    }
}

fn parse_fields(line: &str, line_number: usize) -> Result<Vec<String>, PlatformError> {
    line.split('\t')
        .map(|field| unescape_text(field, line_number))
        .collect()
}

fn parse_resolved_path(value: &str) -> Option<PathBuf> {
    let marker = "path: \"";
    let start = value.find(marker)? + marker.len();
    let rest = &value[start..];
    let mut escaped = false;
    let mut out = String::new();
    for ch in rest.chars() {
        if escaped {
            out.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '"' => break,
            _ => out.push(ch),
        }
    }
    Some(PathBuf::from(out))
}

fn escape_text(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            _ => out.push(ch),
        }
    }
    out
}

fn unescape_text(value: &str, line_number: usize) -> Result<String, PlatformError> {
    let mut out = String::new();
    let mut chars = value.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            out.push(ch);
            continue;
        }
        let Some(escaped) = chars.next() else {
            return Err(PlatformError::Parse(format!(
                "line {line_number}: unterminated escape sequence"
            )));
        };
        match escaped {
            '\\' => out.push('\\'),
            'n' => out.push('\n'),
            'r' => out.push('\r'),
            't' => out.push('\t'),
            other => {
                return Err(PlatformError::Parse(format!(
                    "line {line_number}: unsupported escape `\\{other}`"
                )))
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_roundtrip() {
        let original = "tab\tnewline\nbackslash\\";
        let encoded = escape_text(original);
        let decoded = unescape_text(&encoded, 1).expect("decode");
        assert_eq!(decoded, original);
    }

    #[test]
    fn bundle_summary_counts_root_and_dependencies() {
        let bundle = BytecodeBundle {
            root_source_path: "root.m".to_string(),
            root_module: BytecodeModule {
                backend: BackendKind::Bytecode,
                unit_kind: "Script".to_string(),
                entry: "<script>".to_string(),
                classes: Vec::new(),
                functions: vec![BytecodeFunction {
                    name: "<script>".to_string(),
                    role: "script_entry".to_string(),
                    owner_class_name: None,
                    params: Vec::new(),
                    outputs: Vec::new(),
                    captures: Vec::new(),
                    temp_count: 1,
                    label_count: 0,
                    instructions: vec![BytecodeInstruction::Return { values: Vec::new() }],
                }],
            },
            dependency_modules: vec![PackagedBytecodeModule {
                module_id: "dep0".to_string(),
                source_path: "dep.m".to_string(),
                module: BytecodeModule {
                    backend: BackendKind::Bytecode,
                    unit_kind: "FunctionFile".to_string(),
                    entry: "dep#s0w0".to_string(),
                    classes: Vec::new(),
                    functions: vec![BytecodeFunction {
                        name: "dep#s0w0".to_string(),
                        role: "function".to_string(),
                        owner_class_name: None,
                        params: Vec::new(),
                        outputs: Vec::new(),
                        captures: Vec::new(),
                        temp_count: 1,
                        label_count: 0,
                        instructions: vec![BytecodeInstruction::Return { values: Vec::new() }],
                    }],
                },
            }],
        };

        let summary = summarize_bundle(&bundle);
        assert_eq!(summary.total_modules, 2);
        assert_eq!(summary.dependency_modules, 1);
        assert_eq!(summary.total_functions, 2);
        assert_eq!(summary.total_instructions, 2);
    }
}
