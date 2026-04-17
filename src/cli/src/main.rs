use std::{
    env, fs,
    path::{Path, PathBuf},
    process,
    time::{SystemTime, UNIX_EPOCH},
};

#[cfg(target_os = "windows")]
mod windows_host;

#[cfg(not(target_os = "windows"))]
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use matlab_codegen::{
    emit_bytecode, render_bytecode, render_codegen_summary, render_verification_summary,
    summarize_bytecode, verify_bytecode, BytecodeModule,
};
use matlab_execution::{
    execute_function_file, execute_function_file_bytecode_bundle,
    execute_function_file_bytecode_module, execute_script, execute_script_bytecode_bundle,
    execute_script_bytecode_module, render_execution_result, render_matlab_execution_result,
};
use matlab_frontend::{
    ast::CompilationUnitKind,
    diagnostics::Diagnostic as FrontendDiagnostic,
    parser::{parse_source, ParseMode},
    source::SourceFileId,
    testing::render_compilation_unit,
};
use matlab_interop::{
    read_workspace_snapshot, write_workspace_snapshot_with_modules, WorkspaceSnapshotBundleModule,
};
use matlab_ir::{lower_to_hir, testing::render_hir};
use matlab_optimizer::{optimize_module, render_optimization_summary, OptimizationSummary};
use matlab_platform::{
    collect_bytecode_dependency_paths, encode_bytecode_module, read_bytecode_artifact, read_bytecode_bundle,
    render_bundle_summary, rewrite_bytecode_bundle_targets, write_bytecode_artifact,
    write_bytecode_bundle, BytecodeBundle, PackagedBytecodeModule,
};
use matlab_resolver::ResolverContext;
use matlab_runtime::{render_workspace, Value, Workspace};
use matlab_semantics::{
    analyze_compilation_unit_with_context, diagnostics::SemanticDiagnostic,
    testing::render_analysis,
};

#[cfg(target_os = "windows")]
const WINDOWS_FIGURE_HOST_COMMAND: &str = "__figure-host";

struct LiveFigureSession {
    dir: PathBuf,
    index_path: PathBuf,
    viewer_target: String,
    open_on_surface: bool,
    previous_dir: Option<std::ffi::OsString>,
    previous_title: Option<std::ffi::OsString>,
}

impl Drop for LiveFigureSession {
    fn drop(&mut self) {
        if let Some(previous) = &self.previous_dir {
            env::set_var("MATC_FIGURE_BACKEND_DIR", previous);
        } else {
            env::remove_var("MATC_FIGURE_BACKEND_DIR");
        }
        if let Some(previous) = &self.previous_title {
            env::set_var("MATC_FIGURE_BACKEND_TITLE", previous);
        } else {
            env::remove_var("MATC_FIGURE_BACKEND_TITLE");
        }
    }
}

fn main() {
    let exit_code = match run(env::args().skip(1).collect()) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            2
        }
    };

    process::exit(exit_code);
}

fn run(args: Vec<String>) -> Result<i32, String> {
    let Some(command) = args.first().map(|value| value.as_str()) else {
        print_help();
        return Ok(0);
    };

    match command {
        #[cfg(target_os = "windows")]
        WINDOWS_FIGURE_HOST_COMMAND => {
            let (session_dir, fallback_path) = parse_figure_host_args(&args)?;
            windows_host::run_internal_host(session_dir, fallback_path)?;
            Ok(0)
        }
        "help" | "--help" | "-h" => {
            print_help();
            Ok(0)
        }
        "parse" => {
            let path = args
                .get(1)
                .ok_or_else(|| "missing file path for `matc parse`".to_string())?;
            let source = read_source(path)?;
            let parsed = parse_source(&source, SourceFileId(1), ParseMode::AutoDetect);
            if !parsed.diagnostics.is_empty() {
                print_frontend_diagnostics(&parsed.diagnostics);
                return Ok(1);
            }

            let unit = parsed
                .unit
                .ok_or_else(|| "parser produced no compilation unit".to_string())?;
            print!("{}", render_compilation_unit(&unit));
            Ok(0)
        }
        "check" => {
            let path = args
                .get(1)
                .ok_or_else(|| "missing file path for `matc check`".to_string())?;
            let source = read_source(path)?;
            let parsed = parse_source(&source, SourceFileId(1), ParseMode::AutoDetect);
            if !parsed.diagnostics.is_empty() {
                print_frontend_diagnostics(&parsed.diagnostics);
                return Ok(1);
            }

            let unit = parsed
                .unit
                .ok_or_else(|| "parser produced no compilation unit".to_string())?;
            let context = ResolverContext::from_source_file(Path::new(path).to_path_buf())
                .with_env_search_roots("MATC_PATH");
            let analysis = analyze_compilation_unit_with_context(&unit, &context);
            print!("{}", render_analysis(&analysis));
            Ok(if analysis.has_errors() { 1 } else { 0 })
        }
        "lower" => {
            let path = args
                .get(1)
                .ok_or_else(|| "missing file path for `matc lower`".to_string())?;
            let source = read_source(path)?;
            let parsed = parse_source(&source, SourceFileId(1), ParseMode::AutoDetect);
            if !parsed.diagnostics.is_empty() {
                print_frontend_diagnostics(&parsed.diagnostics);
                return Ok(1);
            }

            let unit = parsed
                .unit
                .ok_or_else(|| "parser produced no compilation unit".to_string())?;
            let context = ResolverContext::from_source_file(Path::new(path).to_path_buf())
                .with_env_search_roots("MATC_PATH");
            let analysis = analyze_compilation_unit_with_context(&unit, &context);
            let hir = lower_to_hir(&unit, &analysis);
            print!("{}", render_hir(&hir));
            Ok(if analysis.has_errors() { 1 } else { 0 })
        }
        "optimize" => {
            let path = args
                .get(1)
                .ok_or_else(|| "missing file path for `matc optimize`".to_string())?;
            let source = read_source(path)?;
            let parsed = parse_source(&source, SourceFileId(1), ParseMode::AutoDetect);
            if !parsed.diagnostics.is_empty() {
                print_frontend_diagnostics(&parsed.diagnostics);
                return Ok(1);
            }

            let unit = parsed
                .unit
                .ok_or_else(|| "parser produced no compilation unit".to_string())?;
            let context = ResolverContext::from_source_file(Path::new(path).to_path_buf())
                .with_env_search_roots("MATC_PATH");
            let analysis = analyze_compilation_unit_with_context(&unit, &context);
            if analysis.has_errors() {
                print_semantic_diagnostics(&analysis.diagnostics);
                return Ok(1);
            }

            let hir = lower_to_hir(&unit, &analysis);
            let optimized = optimize_module(&hir);
            print!("{}", render_optimization_summary(&optimized.summary));
            println!();
            print!("{}", render_hir(&optimized.module));
            Ok(0)
        }
        "codegen" => {
            let path = args
                .get(1)
                .ok_or_else(|| "missing file path for `matc codegen`".to_string())?;
            let source = read_source(path)?;
            let parsed = parse_source(&source, SourceFileId(1), ParseMode::AutoDetect);
            if !parsed.diagnostics.is_empty() {
                print_frontend_diagnostics(&parsed.diagnostics);
                return Ok(1);
            }

            let unit = parsed
                .unit
                .ok_or_else(|| "parser produced no compilation unit".to_string())?;
            let context = ResolverContext::from_source_file(Path::new(path).to_path_buf())
                .with_env_search_roots("MATC_PATH");
            let analysis = analyze_compilation_unit_with_context(&unit, &context);
            if analysis.has_errors() {
                print_semantic_diagnostics(&analysis.diagnostics);
                return Ok(1);
            }

            let hir = lower_to_hir(&unit, &analysis);
            let optimized = optimize_module(&hir);
            let bytecode = emit_bytecode(&optimized.module);
            let codegen_summary = summarize_bytecode(&bytecode);
            let verification = verify_bytecode(&bytecode);
            print!("{}", render_optimization_summary(&optimized.summary));
            println!();
            print!("{}", render_codegen_summary(&codegen_summary));
            println!();
            print!("{}", render_verification_summary(&verification));
            println!();
            print!("{}", render_bytecode(&bytecode));
            Ok(0)
        }
        "package-bytecode" => {
            let path = args.get(1).ok_or_else(|| {
                "missing source file path for `matc package-bytecode`".to_string()
            })?;
            let artifact_path = args.get(2).ok_or_else(|| {
                "missing artifact file path for `matc package-bytecode`".to_string()
            })?;
            let source = read_source(path)?;
            let parsed = parse_source(&source, SourceFileId(1), ParseMode::AutoDetect);
            if !parsed.diagnostics.is_empty() {
                print_frontend_diagnostics(&parsed.diagnostics);
                return Ok(1);
            }

            let unit = parsed
                .unit
                .ok_or_else(|| "parser produced no compilation unit".to_string())?;
            let context = ResolverContext::from_source_file(Path::new(path).to_path_buf())
                .with_env_search_roots("MATC_PATH");
            let analysis = analyze_compilation_unit_with_context(&unit, &context);
            if analysis.has_errors() {
                print_semantic_diagnostics(&analysis.diagnostics);
                return Ok(1);
            }

            let hir = lower_to_hir(&unit, &analysis);
            let optimized = optimize_module(&hir);
            let bytecode = emit_bytecode(&optimized.module);
            let codegen_summary = summarize_bytecode(&bytecode);
            let verification = verify_bytecode(&bytecode);
            if !verification.ok() {
                print!("{}", render_verification_summary(&verification));
                return Ok(1);
            }

            write_bytecode_artifact(Path::new(artifact_path), &bytecode)
                .map_err(|error| error.to_string())?;
            let byte_count = fs::metadata(artifact_path)
                .map(|metadata| metadata.len())
                .unwrap_or(0);

            print!("{}", render_optimization_summary(&optimized.summary));
            println!();
            print!("{}", render_codegen_summary(&codegen_summary));
            println!();
            print!("{}", render_verification_summary(&verification));
            println!();
            println!("artifact");
            println!("  path = {artifact_path}");
            println!("  bytes = {byte_count}");
            Ok(0)
        }
        "inspect-bytecode" => {
            let artifact_path = args.get(1).ok_or_else(|| {
                "missing artifact file path for `matc inspect-bytecode`".to_string()
            })?;
            let bytecode = read_bytecode_artifact(Path::new(artifact_path))
                .map_err(|error| error.to_string())?;
            let codegen_summary = summarize_bytecode(&bytecode);
            let verification = verify_bytecode(&bytecode);
            print!("{}", render_codegen_summary(&codegen_summary));
            println!();
            print!("{}", render_verification_summary(&verification));
            println!();
            print!("{}", render_bytecode(&bytecode));
            Ok(if verification.ok() { 0 } else { 1 })
        }
        "export-workspace" => {
            let input_path = args
                .get(1)
                .ok_or_else(|| "missing input path for `matc export-workspace`".to_string())?;
            let snapshot_path = args
                .get(2)
                .ok_or_else(|| "missing snapshot path for `matc export-workspace`".to_string())?;
            let runtime_args = parse_runtime_args(&args[3..])?;
            let exported = execute_input_workspace(input_path, &runtime_args)?;
            let snapshot_bundle_modules = exported
                .bundle_modules
                .iter()
                .map(|module| WorkspaceSnapshotBundleModule {
                    module_id: module.module_id.clone(),
                    source_path: module.source_path.clone(),
                    encoded_module: encode_bytecode_module(&module.module),
                })
                .collect::<Vec<_>>();
            write_workspace_snapshot_with_modules(
                Path::new(snapshot_path),
                &exported.workspace,
                &snapshot_bundle_modules,
            )
                .map_err(|error| error.to_string())?;
            let byte_count = fs::metadata(snapshot_path)
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            println!("workspace_snapshot");
            println!("  input = {input_path}");
            println!("  path = {snapshot_path}");
            println!("  variables = {}", exported.workspace.len());
            println!("  bytes = {byte_count}");
            Ok(0)
        }
        "inspect-workspace" => {
            let snapshot_path = args
                .get(1)
                .ok_or_else(|| "missing snapshot path for `matc inspect-workspace`".to_string())?;
            let workspace = read_workspace_snapshot(Path::new(snapshot_path))
                .map_err(|error| error.to_string())?;
            print!("{}", render_workspace(&workspace));
            Ok(0)
        }
        "bundle-bytecode" => {
            let path = args
                .get(1)
                .ok_or_else(|| "missing source file path for `matc bundle-bytecode`".to_string())?;
            let bundle_path = args
                .get(2)
                .ok_or_else(|| "missing bundle file path for `matc bundle-bytecode`".to_string())?;
            let (root, bundle) = build_bytecode_bundle(Path::new(path))?;
            let codegen_summary = summarize_bytecode(&root.bytecode);
            let verification = verify_bytecode(&root.bytecode);
            if !verification.ok() {
                print!("{}", render_verification_summary(&verification));
                return Ok(1);
            }

            write_bytecode_bundle(Path::new(bundle_path), &bundle)
                .map_err(|error| error.to_string())?;
            let byte_count = fs::metadata(bundle_path)
                .map(|metadata| metadata.len())
                .unwrap_or(0);

            print!(
                "{}",
                render_optimization_summary(&root.optimization_summary)
            );
            println!();
            print!("{}", render_codegen_summary(&codegen_summary));
            println!();
            print!("{}", render_verification_summary(&verification));
            println!();
            print!("{}", render_bundle_summary(&bundle));
            print!("{}", render_bundle_modules(&bundle));
            println!("bundle_artifact");
            println!("  path = {bundle_path}");
            println!("  bytes = {byte_count}");
            Ok(0)
        }
        "inspect-bundle" => {
            let bundle_path = args
                .get(1)
                .ok_or_else(|| "missing bundle file path for `matc inspect-bundle`".to_string())?;
            let bundle =
                read_bytecode_bundle(Path::new(bundle_path)).map_err(|error| error.to_string())?;
            let verification = verify_bytecode(&bundle.root_module);
            print!("{}", render_bundle_summary(&bundle));
            print!("{}", render_bundle_modules(&bundle));
            println!();
            print!(
                "{}",
                render_codegen_summary(&summarize_bytecode(&bundle.root_module))
            );
            println!();
            print!("{}", render_verification_summary(&verification));
            println!();
            print!("{}", render_bytecode(&bundle.root_module));
            Ok(if verification.ok() { 0 } else { 1 })
        }
        "run" => {
            let path = args
                .get(1)
                .ok_or_else(|| "missing file path for `matc run`".to_string())?;
            let runtime_args = parse_runtime_args(&args[2..])?;
            let live_figures = create_live_figure_session(Path::new(path));
            let source = read_source(path)?;
            let parsed = parse_source(&source, SourceFileId(1), ParseMode::AutoDetect);
            if !parsed.diagnostics.is_empty() {
                print_frontend_diagnostics(&parsed.diagnostics);
                return Ok(1);
            }

            let unit = parsed
                .unit
                .ok_or_else(|| "parser produced no compilation unit".to_string())?;
            let context = ResolverContext::from_source_file(Path::new(path).to_path_buf())
                .with_env_search_roots("MATC_PATH");
            let analysis = analyze_compilation_unit_with_context(&unit, &context);
            if analysis.has_errors() {
                print_semantic_diagnostics(&analysis.diagnostics);
                return Ok(1);
            }

            let hir = lower_to_hir(&unit, &analysis);
            let result = match unit.kind {
                CompilationUnitKind::Script => {
                    if !runtime_args.is_empty() {
                        return Err(
                            "`matc run` does not accept positional inputs for script files"
                                .to_string(),
                        );
                    }
                    execute_script(&hir)
                }
                CompilationUnitKind::FunctionFile => execute_function_file(&hir, &runtime_args),
                CompilationUnitKind::ClassFile => {
                    return Err("`matc run` does not execute class definition files directly".to_string())
                }
            }
            .map_err(|error| format!("execution failed for `{path}`: {error}"))?;
            match unit.kind {
                CompilationUnitKind::Script => {
                    print!("{}", render_matlab_execution_result(&result));
                }
                CompilationUnitKind::FunctionFile => {
                    print!("{}", render_execution_result(&result));
                }
                CompilationUnitKind::ClassFile => unreachable!("class-file execution returns early"),
            }
            maybe_surface_figures(&result, Path::new(path), live_figures.as_ref());
            Ok(0)
        }
        "run-workspace" => {
            let path = args
                .get(1)
                .ok_or_else(|| "missing file path for `matc run-workspace`".to_string())?;
            let runtime_args = parse_runtime_args(&args[2..])?;
            let live_figures = create_live_figure_session(Path::new(path));
            let source = read_source(path)?;
            let parsed = parse_source(&source, SourceFileId(1), ParseMode::AutoDetect);
            if !parsed.diagnostics.is_empty() {
                print_frontend_diagnostics(&parsed.diagnostics);
                return Ok(1);
            }

            let unit = parsed
                .unit
                .ok_or_else(|| "parser produced no compilation unit".to_string())?;
            let context = ResolverContext::from_source_file(Path::new(path).to_path_buf())
                .with_env_search_roots("MATC_PATH");
            let analysis = analyze_compilation_unit_with_context(&unit, &context);
            if analysis.has_errors() {
                print_semantic_diagnostics(&analysis.diagnostics);
                return Ok(1);
            }

            let hir = lower_to_hir(&unit, &analysis);
            let result = match unit.kind {
                CompilationUnitKind::Script => {
                    if !runtime_args.is_empty() {
                        return Err(
                            "`matc run-workspace` does not accept positional inputs for script files"
                                .to_string(),
                        );
                    }
                    execute_script(&hir)
                }
                CompilationUnitKind::FunctionFile => execute_function_file(&hir, &runtime_args),
                CompilationUnitKind::ClassFile => {
                    return Err(
                        "`matc run-workspace` does not execute class definition files directly"
                            .to_string(),
                    )
                }
            }
            .map_err(|error| format!("execution failed for `{path}`: {error}"))?;
            print!("{}", render_execution_result(&result));
            maybe_surface_figures(&result, Path::new(path), live_figures.as_ref());
            Ok(0)
        }
        "run-bytecode" => {
            let path = args
                .get(1)
                .ok_or_else(|| "missing file path for `matc run-bytecode`".to_string())?;
            let runtime_args = parse_runtime_args(&args[2..])?;
            let live_figures = create_live_figure_session(Path::new(path));
            let source = read_source(path)?;
            let parsed = parse_source(&source, SourceFileId(1), ParseMode::AutoDetect);
            if !parsed.diagnostics.is_empty() {
                print_frontend_diagnostics(&parsed.diagnostics);
                return Ok(1);
            }

            let unit = parsed
                .unit
                .ok_or_else(|| "parser produced no compilation unit".to_string())?;
            let context = ResolverContext::from_source_file(Path::new(path).to_path_buf())
                .with_env_search_roots("MATC_PATH");
            let analysis = analyze_compilation_unit_with_context(&unit, &context);
            if analysis.has_errors() {
                print_semantic_diagnostics(&analysis.diagnostics);
                return Ok(1);
            }

            let hir = lower_to_hir(&unit, &analysis);
            let optimized = optimize_module(&hir);
            let bytecode = emit_bytecode(&optimized.module);
            let result = match unit.kind {
                CompilationUnitKind::Script => {
                    if !runtime_args.is_empty() {
                        return Err(
                            "`matc run-bytecode` does not accept positional inputs for script files"
                                .to_string(),
                        );
                    }
                    execute_script_bytecode_module(&bytecode, path.to_string())
                }
                CompilationUnitKind::FunctionFile => execute_function_file_bytecode_module(
                    &bytecode,
                    &runtime_args,
                    path.to_string(),
                ),
                CompilationUnitKind::ClassFile => {
                    return Err(
                        "`matc run-bytecode` does not execute class definition files directly"
                            .to_string(),
                    )
                }
            }
            .map_err(|error| format!("bytecode execution failed for `{path}`: {error}"))?;
            print!("{}", render_execution_result(&result));
            maybe_surface_figures(&result, Path::new(path), live_figures.as_ref());
            Ok(0)
        }
        "run-artifact" => {
            let artifact_path = args
                .get(1)
                .ok_or_else(|| "missing artifact file path for `matc run-artifact`".to_string())?;
            let runtime_args = parse_runtime_args(&args[2..])?;
            let live_figures = create_live_figure_session(Path::new(artifact_path));
            let bytecode = read_bytecode_artifact(Path::new(artifact_path))
                .map_err(|error| error.to_string())?;
            let result = match bytecode.unit_kind.as_str() {
                "Script" => {
                    if !runtime_args.is_empty() {
                        return Err(
                            "`matc run-artifact` does not accept positional inputs for script artifacts"
                                .to_string(),
                        );
                    }
                    execute_script_bytecode_module(&bytecode, artifact_path.to_string())
                }
                "FunctionFile" => execute_function_file_bytecode_module(
                    &bytecode,
                    &runtime_args,
                    artifact_path.to_string(),
                ),
                "ClassFile" => {
                    return Err(serialized_class_execution_error("run-artifact", "artifacts"))
                }
                other => {
                    return Err(format!(
                        "artifact `{artifact_path}` has unsupported unit kind `{other}`"
                    ))
                }
            }
            .map_err(|error| format!("artifact execution failed for `{artifact_path}`: {error}"))?;
            print!("{}", render_execution_result(&result));
            maybe_surface_figures(&result, Path::new(artifact_path), live_figures.as_ref());
            Ok(0)
        }
        "run-bundle" => {
            let bundle_path = args
                .get(1)
                .ok_or_else(|| "missing bundle file path for `matc run-bundle`".to_string())?;
            let runtime_args = parse_runtime_args(&args[2..])?;
            let live_figures = create_live_figure_session(Path::new(bundle_path));
            let bundle =
                read_bytecode_bundle(Path::new(bundle_path)).map_err(|error| error.to_string())?;
            let result = match bundle.root_module.unit_kind.as_str() {
                "Script" => {
                    if !runtime_args.is_empty() {
                        return Err(
                            "`matc run-bundle` does not accept positional inputs for script bundles"
                                .to_string(),
                        );
                    }
                    execute_script_bytecode_bundle(&bundle)
                }
                "FunctionFile" => execute_function_file_bytecode_bundle(&bundle, &runtime_args),
                "ClassFile" => {
                    return Err(serialized_class_execution_error("run-bundle", "bundles"))
                }
                other => {
                    return Err(format!(
                        "bundle `{bundle_path}` has unsupported unit kind `{other}`"
                    ))
                }
            }
            .map_err(|error| format!("bundle execution failed for `{bundle_path}`: {error}"))?;
            print!("{}", render_execution_result(&result));
            maybe_surface_figures(&result, Path::new(bundle_path), live_figures.as_ref());
            Ok(0)
        }
        _ => Err(format!(
            "unknown command `{command}`\n\nUse `matc help` for usage."
        )),
    }
}

fn serialized_class_execution_error(command: &str, container: &str) -> String {
    format!("`matc {command}` does not execute class definition {container} directly")
}

struct CompiledBytecodeUnit {
    optimization_summary: OptimizationSummary,
    bytecode: BytecodeModule,
}

fn compile_bytecode_file(path: &Path) -> Result<CompiledBytecodeUnit, String> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("failed to read `{}`: {error}", path.display()))?;
    let parsed = parse_source(&source, SourceFileId(1), ParseMode::AutoDetect);
    if parsed.has_errors() {
        return Err(format!(
            "failed to parse `{}`: {}",
            path.display(),
            format_frontend_diagnostics(&parsed.diagnostics)
        ));
    }

    let unit = parsed.unit.ok_or_else(|| {
        format!(
            "parser produced no compilation unit for `{}`",
            path.display()
        )
    })?;
    let context =
        ResolverContext::from_source_file(path.to_path_buf()).with_env_search_roots("MATC_PATH");
    let analysis = analyze_compilation_unit_with_context(&unit, &context);
    if analysis.has_errors() {
        return Err(format!(
            "failed to analyze `{}`: {}",
            path.display(),
            format_semantic_diagnostics(&analysis.diagnostics)
        ));
    }

    let hir = lower_to_hir(&unit, &analysis);
    let optimized = optimize_module(&hir);
    let bytecode = emit_bytecode(&optimized.module);
    let verification = verify_bytecode(&bytecode);
    if !verification.ok() {
        return Err(format!(
            "bytecode verification failed for `{}`: {}",
            path.display(),
            verification
                .issues
                .iter()
                .map(|issue| format!("{}: {}", issue.function, issue.message))
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }

    Ok(CompiledBytecodeUnit {
        optimization_summary: optimized.summary,
        bytecode,
    })
}

fn build_bytecode_bundle(path: &Path) -> Result<(CompiledBytecodeUnit, BytecodeBundle), String> {
    let root = compile_bytecode_file(path)?;
    let mut seen = std::collections::BTreeSet::new();
    seen.insert(path.display().to_string());
    let mut pending = collect_bytecode_dependency_paths(&root.bytecode);
    let mut compiled_dependencies = Vec::new();

    while let Some(next_path) = pending.pop() {
        let key = next_path.display().to_string();
        if !seen.insert(key.clone()) {
            continue;
        }

        let compiled = compile_bytecode_file(&next_path)?;
        for dependency in collect_bytecode_dependency_paths(&compiled.bytecode) {
            let dependency_key = dependency.display().to_string();
            if !seen.contains(&dependency_key) {
                pending.push(dependency);
            }
        }
        compiled_dependencies.push((next_path, key, compiled.bytecode));
    }

    compiled_dependencies.sort_by(|lhs, rhs| lhs.1.cmp(&rhs.1));
    let path_to_module_id = compiled_dependencies
        .iter()
        .enumerate()
        .map(|(index, (path, _, _))| (path.clone(), format!("dep{index}")))
        .collect::<std::collections::HashMap<_, _>>();
    let root_module = rewrite_bytecode_bundle_targets(&root.bytecode, &path_to_module_id);
    let dependency_modules = compiled_dependencies
        .into_iter()
        .map(|(path, source_path, module)| PackagedBytecodeModule {
            module_id: path_to_module_id
                .get(&path)
                .cloned()
                .expect("bundle module id"),
            source_path,
            module: rewrite_bytecode_bundle_targets(&module, &path_to_module_id),
        })
        .collect::<Vec<_>>();
    Ok((
        root,
        BytecodeBundle {
            root_source_path: path.display().to_string(),
            root_module,
            dependency_modules,
        },
    ))
}

fn render_bundle_modules(bundle: &BytecodeBundle) -> String {
    let mut out = String::new();
    if bundle.dependency_modules.is_empty() {
        out.push_str("bundle_modules\n  (none)\n");
        return out;
    }

    out.push_str("bundle_modules\n");
    for module in &bundle.dependency_modules {
        let summary = summarize_bytecode(&module.module);
        out.push_str(&format!(
            "  id = {} path = {} functions={} instructions={}\n",
            module.module_id, module.source_path, summary.functions, summary.instructions
        ));
    }
    out
}

struct ExportedWorkspace {
    workspace: Workspace,
    bundle_modules: Vec<PackagedBytecodeModule>,
}

fn execute_input_workspace(
    input_path: &str,
    runtime_args: &[Value],
) -> Result<ExportedWorkspace, String> {
    let path = Path::new(input_path);
    match path.extension().and_then(|value| value.to_str()) {
        Some("matpkg") => {
            let bundle = read_bytecode_bundle(path).map_err(|error| error.to_string())?;
            let result = match bundle.root_module.unit_kind.as_str() {
                "Script" => {
                    if !runtime_args.is_empty() {
                        return Err(
                            "`matc export-workspace` does not accept positional inputs for script bundles"
                                .to_string(),
                        );
                    }
                    execute_script_bytecode_bundle(&bundle)
                }
                "FunctionFile" => execute_function_file_bytecode_bundle(&bundle, runtime_args),
                "ClassFile" => {
                    return Err(serialized_class_execution_error(
                        "export-workspace",
                        "bundles",
                    ))
                }
                other => {
                    return Err(format!(
                        "bundle `{input_path}` has unsupported unit kind `{other}`"
                    ))
                }
            }
            .map_err(|error| format!("bundle execution failed for `{input_path}`: {error}"))?;
            let bundle_modules = result.bundle_modules().to_vec();
            Ok(ExportedWorkspace {
                workspace: result.workspace,
                bundle_modules,
            })
        }
        Some("matbc") => {
            let bytecode = read_bytecode_artifact(path).map_err(|error| error.to_string())?;
            let result = match bytecode.unit_kind.as_str() {
                "Script" => {
                    if !runtime_args.is_empty() {
                        return Err(
                            "`matc export-workspace` does not accept positional inputs for script artifacts"
                                .to_string(),
                        );
                    }
                    execute_script_bytecode_module(&bytecode, input_path.to_string())
                }
                "FunctionFile" => execute_function_file_bytecode_module(
                    &bytecode,
                    runtime_args,
                    input_path.to_string(),
                ),
                "ClassFile" => {
                    return Err(serialized_class_execution_error(
                        "export-workspace",
                        "artifacts",
                    ))
                }
                other => {
                    return Err(format!(
                        "artifact `{input_path}` has unsupported unit kind `{other}`"
                    ))
                }
            }
            .map_err(|error| format!("artifact execution failed for `{input_path}`: {error}"))?;
            let bundle_modules = result.bundle_modules().to_vec();
            Ok(ExportedWorkspace {
                workspace: result.workspace,
                bundle_modules,
            })
        }
        _ => {
            let source = read_source(input_path)?;
            let parsed = parse_source(&source, SourceFileId(1), ParseMode::AutoDetect);
            if !parsed.diagnostics.is_empty() {
                return Err(format!(
                    "failed to parse `{input_path}`: {}",
                    format_frontend_diagnostics(&parsed.diagnostics)
                ));
            }

            let unit = parsed
                .unit
                .ok_or_else(|| format!("parser produced no compilation unit for `{input_path}`"))?;
            let context = ResolverContext::from_source_file(path.to_path_buf())
                .with_env_search_roots("MATC_PATH");
            let analysis = analyze_compilation_unit_with_context(&unit, &context);
            if analysis.has_errors() {
                return Err(format!(
                    "failed to analyze `{input_path}`: {}",
                    format_semantic_diagnostics(&analysis.diagnostics)
                ));
            }

            let hir = lower_to_hir(&unit, &analysis);
            let result = match unit.kind {
                CompilationUnitKind::Script => {
                    if !runtime_args.is_empty() {
                        return Err(
                            "`matc export-workspace` does not accept positional inputs for script files"
                                .to_string(),
                        );
                    }
                    execute_script(&hir)
                }
                CompilationUnitKind::FunctionFile => execute_function_file(&hir, runtime_args),
                CompilationUnitKind::ClassFile => {
                    return Err(
                        "`matc export-workspace` does not execute class definition files directly"
                            .to_string(),
                    )
                }
            }
            .map_err(|error| format!("execution failed for `{input_path}`: {error}"))?;
            let bundle_modules = result.bundle_modules().to_vec();
            Ok(ExportedWorkspace {
                workspace: result.workspace,
                bundle_modules,
            })
        }
    }
}

fn read_source(path: &str) -> Result<String, String> {
    fs::read_to_string(path).map_err(|error| format!("failed to read `{path}`: {error}"))
}

fn maybe_surface_figures(
    result: &matlab_execution::ExecutionResult,
    source_path: &Path,
    live_figures: Option<&LiveFigureSession>,
) {
    if result.figures.is_empty() {
        return;
    }

    if let Some(live_figures) = live_figures {
        println!();
        println!("figure_viewer");
        println!("  target = {}", live_figures.viewer_target);
        println!("  path = {}", live_figures.index_path.display());
        println!("  session = {}", live_figures.dir.display());
        if live_figures.open_on_surface {
            maybe_open_figure_path(&live_figures.index_path);
        }
        return;
    }

    let base_name = source_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("matc_figure");
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let output_dir = env::temp_dir().join("matc_figures");
    if fs::create_dir_all(&output_dir).is_err() {
        return;
    }

    for figure in &result.figures {
        let path = output_dir.join(format!("{base_name}-{timestamp}-{}.svg", figure.handle));
        if fs::write(&path, &figure.svg).is_err() {
            continue;
        }
        println!();
        println!("figure_svg");
        println!("  handle = {}", figure.handle);
        println!("  path = {}", path.display());
        maybe_open_figure_path(&path);
    }
}

fn maybe_open_figure_path(path: &Path) {
    #[cfg(target_os = "windows")]
    {
        if try_open_native_figure_window(path) {
            return;
        }
        if try_open_browser_figure_window(path) {
            return;
        }
        let target = path.to_string_lossy().to_string();
        let _ = process::Command::new("cmd")
            .arg("/C")
            .arg("start")
            .arg("")
            .arg(target)
            .spawn();
        return;
    }

    #[cfg(not(target_os = "windows"))]
    {
        let target = path.to_string_lossy().to_string();
        maybe_open_figure_target(&target, None, None);
    }
}

#[cfg(target_os = "windows")]
fn try_open_browser_figure_window(path: &Path) -> bool {
    let Some(browser) = preferred_app_browser_path() else {
        return false;
    };
    let Some(target) = file_url_from_path(path) else {
        return false;
    };
    let profile_dir = path
        .parent()
        .map(|parent| parent.join("browser-profile"))
        .unwrap_or_else(|| env::temp_dir().join("matc-browser-profile"));
    let _ = fs::create_dir_all(&profile_dir);
    process::Command::new(browser)
        .arg(format!("--user-data-dir={}", profile_dir.display()))
        .arg(format!("--app={target}"))
        .arg("--new-window")
        .arg("--window-size=1280,900")
        .arg("--no-first-run")
        .spawn()
        .is_ok()
}

#[cfg(target_os = "windows")]
fn try_open_native_figure_window(path: &Path) -> bool {
    let Some(session_dir) = path.parent() else {
        return false;
    };
    windows_host::launch_internal_host(session_dir, path)
}

#[cfg(target_os = "windows")]
fn parse_figure_host_args(args: &[String]) -> Result<(PathBuf, PathBuf), String> {
    let session_dir = args.get(1).ok_or_else(|| {
        format!("missing session directory for `matc {WINDOWS_FIGURE_HOST_COMMAND}`")
    })?;
    let fallback_path = args
        .get(2)
        .ok_or_else(|| format!("missing fallback path for `matc {WINDOWS_FIGURE_HOST_COMMAND}`"))?;
    Ok((PathBuf::from(session_dir), PathBuf::from(fallback_path)))
}

#[cfg(target_os = "windows")]
fn file_url_from_path(path: &Path) -> Option<String> {
    let canonical = path.canonicalize().ok()?;
    let mut raw = canonical.to_string_lossy().replace('\\', "/");
    if !raw.starts_with('/') {
        raw.insert(0, '/');
    }
    Some(format!("file://{}", percent_encode_for_file_url(&raw)))
}

#[cfg(target_os = "windows")]
fn percent_encode_for_file_url(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b':' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(not(target_os = "windows"))]
fn maybe_open_figure_target(target: &str, title: Option<&str>, session_dir: Option<&Path>) {
    let _ = title;
    let _ = session_dir;
    let _ = target;
}

#[cfg(target_os = "windows")]
fn preferred_app_browser_path() -> Option<&'static str> {
    [
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files (x86)\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
    ]
    .into_iter()
    .find(|path| Path::new(path).exists())
}

#[cfg(not(target_os = "windows"))]
fn start_live_figure_server(dir: PathBuf) -> Option<String> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).ok()?;
    let address = listener.local_addr().ok()?;
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                continue;
            };
            let _ = handle_live_figure_request(&mut stream, &dir);
        }
    });
    Some(format!("http://127.0.0.1:{}/", address.port()))
}

#[cfg(not(target_os = "windows"))]
fn handle_live_figure_request(
    stream: &mut std::net::TcpStream,
    root: &Path,
) -> std::io::Result<()> {
    let mut buffer = [0u8; 4096];
    let count = stream.read(&mut buffer)?;
    if count == 0 {
        return Ok(());
    }
    let request = String::from_utf8_lossy(&buffer[..count]);
    let first_line = request.lines().next().unwrap_or_default();
    let path = first_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .split('?')
        .next()
        .unwrap_or("/");
    let relative = if path == "/" {
        PathBuf::from("index.html")
    } else {
        PathBuf::from(path.trim_start_matches('/'))
    };
    if relative
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        write_http_response(stream, 403, "text/plain; charset=utf-8", b"Forbidden")?;
        return Ok(());
    }
    let target = root.join(relative);
    match fs::read(&target) {
        Ok(bytes) => {
            let content_type = match target.extension().and_then(|value| value.to_str()) {
                Some("html") => "text/html; charset=utf-8",
                Some("json") => "application/json; charset=utf-8",
                Some("svg") => "image/svg+xml",
                Some("js") => "application/javascript; charset=utf-8",
                Some("css") => "text/css; charset=utf-8",
                _ => "application/octet-stream",
            };
            write_http_response(stream, 200, content_type, &bytes)?;
        }
        Err(_) => {
            write_http_response(stream, 404, "text/plain; charset=utf-8", b"Not Found")?;
        }
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn write_http_response(
    stream: &mut std::net::TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> std::io::Result<()> {
    let status_text = match status {
        200 => "OK",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "OK",
    };
    write!(
        stream,
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        status,
        status_text,
        content_type,
        body.len()
    )?;
    stream.write_all(body)?;
    stream.flush()
}

fn create_live_figure_session(source_path: &Path) -> Option<LiveFigureSession> {
    let base_name = source_path
        .file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("matc_figure");
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let dir = env::temp_dir()
        .join("matc_live_figures")
        .join(format!("{base_name}-{timestamp}"));
    if fs::create_dir_all(&dir).is_err() {
        return None;
    }

    let index_path = dir.join("index.html");
    let title = format!("MATC Live Figures - {base_name}");
    let placeholder = r#"<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="X-UA-Compatible" content="IE=edge"><title>__TITLE__</title><style>body{font-family:Segoe UI,Arial,sans-serif;margin:0;background:#f4f4f4;color:#111;}header{padding:12px 16px;background:#111;color:#fff;}main{padding:16px;}section{display:inline-block;vertical-align:top;width:48%;min-width:440px;margin:0 1% 16px 0;background:#fff;border:1px solid #ddd;border-radius:8px;overflow:hidden;box-shadow:0 1px 3px rgba(0,0,0,.08);}h2{margin:0;padding:10px 14px;font-size:14px;background:#fafafa;border-bottom:1px solid #eee;}img{display:block;width:100%;height:auto;background:#fff;}p{padding:16px;margin:0;color:#666;}small{opacity:.75;}</style></head><body><header><strong>__TITLE__</strong> <small id="status">waiting</small></header><main id="figures"><section><p>Waiting for figure output...</p></section></main><script>var lastRevision=null;function render(session){var root=document.getElementById('figures');var status=document.getElementById('status');status.innerText='revision '+session.revision+' | figures '+session.figure_count;if(!session.handles||session.handles.length===0){root.innerHTML='<section><p>Waiting for figure output...</p></section>';return;}var html='';for(var i=0;i<session.handles.length;i++){var handle=session.handles[i];html+='<section><h2>Figure '+handle+'</h2><img src="figure-'+handle+'.svg?v='+session.revision+'" alt="Figure '+handle+'"></section>';}root.innerHTML=html;}function tick(){var xhr=new XMLHttpRequest();xhr.onreadystatechange=function(){if(xhr.readyState===4&&xhr.status===200){try{var session=JSON.parse(xhr.responseText);if(session.revision!==lastRevision){lastRevision=session.revision;render(session);}}catch(e){}}};xhr.open('GET','session.json?ts='+new Date().getTime(),true);xhr.send();window.setTimeout(tick,150);}tick();</script></body></html>
"#
    .replace("__TITLE__", &title);
    if fs::write(&index_path, placeholder).is_err() {
        return None;
    }

    let previous_dir = env::var_os("MATC_FIGURE_BACKEND_DIR");
    let previous_title = env::var_os("MATC_FIGURE_BACKEND_TITLE");
    env::set_var("MATC_FIGURE_BACKEND_DIR", &dir);
    env::set_var("MATC_FIGURE_BACKEND_TITLE", &title);

    #[cfg(target_os = "windows")]
    let (viewer_target, open_on_surface) = ("windows-host".to_string(), true);

    #[cfg(not(target_os = "windows"))]
    let (viewer_target, open_on_surface) =
        if let Some(viewer_url) = start_live_figure_server(dir.clone()) {
            maybe_open_figure_target(&viewer_url, Some(&title), Some(&dir));
            (viewer_url, false)
        } else {
            (index_path.display().to_string(), true)
        };

    Some(LiveFigureSession {
        dir,
        index_path,
        viewer_target,
        open_on_surface,
        previous_dir,
        previous_title,
    })
}

fn parse_runtime_args(args: &[String]) -> Result<Vec<Value>, String> {
    args.iter()
        .map(|arg| {
            if arg.eq_ignore_ascii_case("true") {
                Ok(Value::Logical(true))
            } else if arg.eq_ignore_ascii_case("false") {
                Ok(Value::Logical(false))
            } else {
                arg.parse::<f64>().map(Value::Scalar).map_err(|error| {
                    format!("failed to parse runtime argument `{arg}` as scalar/logical: {error}")
                })
            }
        })
        .collect()
}

fn print_frontend_diagnostics(diagnostics: &[matlab_frontend::diagnostics::Diagnostic]) {
    for diagnostic in diagnostics {
        eprintln!(
            "{} {} at {}:{}",
            diagnostic.code,
            diagnostic.message,
            diagnostic.span.start.line,
            diagnostic.span.start.column
        );
    }
}

fn print_semantic_diagnostics(diagnostics: &[matlab_semantics::diagnostics::SemanticDiagnostic]) {
    for diagnostic in diagnostics {
        eprintln!(
            "{} {} at {}:{}",
            diagnostic.code,
            diagnostic.message,
            diagnostic.span.start.line,
            diagnostic.span.start.column
        );
    }
}

fn format_frontend_diagnostics(diagnostics: &[FrontendDiagnostic]) -> String {
    diagnostics
        .iter()
        .map(|diagnostic| {
            format!(
                "{} {} at {}:{}",
                diagnostic.code,
                diagnostic.message,
                diagnostic.span.start.line,
                diagnostic.span.start.column
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn format_semantic_diagnostics(diagnostics: &[SemanticDiagnostic]) -> String {
    diagnostics
        .iter()
        .map(|diagnostic| {
            format!(
                "{} {} at {}:{}",
                diagnostic.code,
                diagnostic.message,
                diagnostic.span.start.line,
                diagnostic.span.start.column
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn print_help() {
    println!("matc - MATLAB reimplementation CLI scaffold");
    println!();
    println!("Usage:");
    println!("  matc parse <file.m>");
    println!("  matc check <file.m>");
    println!("  matc lower <file.m>");
    println!("  matc optimize <file.m>");
    println!("  matc codegen <file.m>");
    println!("  matc package-bytecode <file.m> <artifact.matbc>");
    println!("  matc inspect-bytecode <artifact.matbc>");
    println!("  matc export-workspace <input> <snapshot.matws> [scalar args...]");
    println!("  matc inspect-workspace <snapshot.matws>");
    println!("  matc bundle-bytecode <file.m> <bundle.matpkg>");
    println!("  matc inspect-bundle <bundle.matpkg>");
    println!("  matc run <file.m> [scalar args...]");
    println!("  matc run-workspace <file.m> [scalar args...]");
    println!("  matc run-bytecode <file.m> [scalar args...]");
    println!("  matc run-artifact <artifact.matbc> [scalar args...]");
    println!("  matc run-bundle <bundle.matpkg> [scalar args...]");
    println!("  matc help");
    println!();
    println!("Environment:");
    println!("  MATC_PATH   Additional search roots used by resolver-aware `check` output.");
    println!();
    println!("Commands:");
    println!("  parse  Parse a .m file and print the current AST-oriented render.");
    println!("  check  Parse and run the current semantic binder, then print bindings, captures, and resolution details.");
    println!(
        "  lower  Parse, analyze, and lower the current MATLAB subset into the first HIR render."
    );
    println!("  optimize  Parse, analyze, lower, run the current HIR optimizer passes, and print the optimized HIR plus a pass summary.");
    println!("  codegen  Parse, analyze, lower, optimize, and emit the current bytecode-style backend render plus summaries.");
    println!("  package-bytecode  Parse, analyze, lower, optimize, verify, and save a serialized bytecode artifact to disk.");
    println!("  inspect-bytecode  Load a serialized bytecode artifact from disk, then print backend summaries and bytecode.");
    println!("  export-workspace  Execute a source file, artifact, or bundle, then write the final workspace snapshot to disk.");
    println!("  inspect-workspace  Load a serialized workspace snapshot from disk and print the rendered workspace.");
    println!("  bundle-bytecode  Parse, analyze, lower, optimize, recursively package external bytecode dependencies, and save a runnable bundle.");
    println!("  inspect-bundle  Load a serialized bytecode bundle from disk, then print bundle metadata plus the root backend render.");
    println!(
        "  run    Execute through the interpreter and print MATLAB-style displayed script output."
    );
    println!("  run-workspace  Execute through the interpreter and print the final workspace.");
    println!("  run-bytecode  Parse, analyze, lower, optimize, execute through the bytecode VM path, and print the final workspace.");
    println!("  run-artifact  Load and execute a serialized bytecode artifact, then print the final workspace.");
    println!("  run-bundle  Load and execute a serialized bytecode bundle, resolving packaged external modules before the filesystem.");
}

#[cfg(test)]
mod cli_tests {
    use super::{build_bytecode_bundle, execute_input_workspace, serialized_class_execution_error};
    use matlab_interop::{write_workspace_snapshot_with_modules, WorkspaceSnapshotBundleModule};
    use matlab_platform::{encode_bytecode_module, write_bytecode_bundle};
    use matlab_runtime::Value;
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{stamp}"))
    }

    #[test]
    fn serialized_class_execution_errors_are_explicit() {
        assert_eq!(
            serialized_class_execution_error("run-artifact", "artifacts"),
            "`matc run-artifact` does not execute class definition artifacts directly"
        );
        assert_eq!(
            serialized_class_execution_error("run-bundle", "bundles"),
            "`matc run-bundle` does not execute class definition bundles directly"
        );
        assert_eq!(
            serialized_class_execution_error("export-workspace", "artifacts"),
            "`matc export-workspace` does not execute class definition artifacts directly"
        );
    }

    #[test]
    fn export_workspace_preserves_embedded_bundle_modules_after_source_snapshot_reload() {
        let temp_dir = unique_temp_dir("matc-export-workspace-bundle-registry");
        let source_dir = temp_dir.join("src");
        fs::create_dir_all(&source_dir).expect("create source dir");
        let class_path = source_dir.join("Point.m");
        let producer_path = source_dir.join("producer.m");
        let consumer_path = temp_dir.join("consumer.m");
        let bundle_path = temp_dir.join("producer.matpkg");
        let snapshot_path = temp_dir.join("state.matws");
        let snapshot_text = snapshot_path.to_string_lossy().replace('\\', "/");
        fs::write(
            &class_path,
            "classdef Point\n\
             properties\n\
             x = 0;\n\
             child = [];\n\
             end\n\
             methods\n\
             function obj = Point(x, child)\n\
             obj.x = x;\n\
             obj.child = child;\n\
             end\n\
             function out = total(obj)\n\
             out = sum([obj.x]);\n\
             end\n\
             end\n\
             end\n",
        )
        .expect("write export-workspace class");
        fs::write(
            &producer_path,
            "objs = [Point(1, Point(10, [])), Point(2, Point(20, [])), Point(3, Point(30, []))];\n\
             f = @objs.child.total;\n",
        )
        .expect("write export-workspace producer");

        let (_, bundle) = build_bytecode_bundle(&producer_path).expect("build bytecode bundle");
        write_bytecode_bundle(&bundle_path, &bundle).expect("write bytecode bundle");
        let exported_bundle =
            execute_input_workspace(bundle_path.to_str().expect("bundle path"), &[])
                .expect("execute bundle input");
        let snapshot_modules = exported_bundle
            .bundle_modules
            .iter()
            .map(|module| WorkspaceSnapshotBundleModule {
                module_id: module.module_id.clone(),
                source_path: module.source_path.clone(),
                encoded_module: encode_bytecode_module(&module.module),
            })
            .collect::<Vec<_>>();
        write_workspace_snapshot_with_modules(
            &snapshot_path,
            &exported_bundle.workspace,
            &snapshot_modules,
        )
        .expect("write workspace snapshot");

        fs::remove_dir_all(&source_dir).expect("remove source tree");
        fs::write(
            &consumer_path,
            format!(
                "load('{snapshot_text}');\n\
                 out = f();\n"
            ),
        )
        .expect("write export-workspace consumer");

        let exported_source =
            execute_input_workspace(consumer_path.to_str().expect("consumer path"), &[])
                .expect("execute source input");
        assert_eq!(exported_source.bundle_modules.len(), 1);
        assert_eq!(exported_source.bundle_modules[0].module_id, "dep0");
        assert_eq!(
            exported_source.workspace.get("out"),
            Some(&Value::Scalar(60.0))
        );

        let _ = fs::remove_dir_all(temp_dir);
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::{parse_figure_host_args, WINDOWS_FIGURE_HOST_COMMAND};
    use std::path::PathBuf;

    #[test]
    fn parses_internal_figure_host_args() {
        let args = vec![
            WINDOWS_FIGURE_HOST_COMMAND.to_string(),
            r"C:\temp\session".to_string(),
            r"C:\temp\session\index.html".to_string(),
        ];
        let (session_dir, fallback_path) = parse_figure_host_args(&args).expect("parse args");
        assert_eq!(session_dir, PathBuf::from(r"C:\temp\session"));
        assert_eq!(fallback_path, PathBuf::from(r"C:\temp\session\index.html"));
    }

    #[test]
    fn internal_figure_host_args_require_session_and_fallback() {
        let missing_session = vec![WINDOWS_FIGURE_HOST_COMMAND.to_string()];
        assert!(parse_figure_host_args(&missing_session).is_err());

        let missing_fallback = vec![
            WINDOWS_FIGURE_HOST_COMMAND.to_string(),
            r"C:\temp\session".to_string(),
        ];
        assert!(parse_figure_host_args(&missing_fallback).is_err());
    }
}
