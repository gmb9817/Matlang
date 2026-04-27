//! First semantic binder pass for scopes, workspaces, bindings, and name resolution.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use matlab_frontend::ast::{
    AssignmentTarget, ClassDef, ClassMemberAccess, ClassMethodBlock, ClassPropertyBlock,
    CompilationUnit, CompilationUnitKind, Expression, ExpressionKind, FunctionDef,
    FunctionHandleTarget, Identifier, IndexArgument, Item, QualifiedName, Statement, StatementKind,
};
use matlab_frontend::source::{SourceFileId, SourcePosition, SourceSpan};
use matlab_interop::{read_mat_file, read_workspace_snapshot};

use crate::{
    diagnostics::SemanticDiagnostic,
    symbols::{
        Binding, BindingId, BindingStorage, Capture, CaptureAccess, ClassInfo, ClassPropertyInfo,
        ExternalMethodInfo, ReferenceResolution, ReferenceRole, ResolvedReference, Symbol,
        SymbolId, SymbolKind, SymbolReference,
    },
    workspace::{Scope, ScopeId, ScopeKind, Workspace, WorkspaceId, WorkspaceKind},
};

#[derive(Debug, Clone)]
pub struct AnalysisResult {
    pub scopes: Vec<Scope>,
    pub workspaces: Vec<Workspace>,
    pub symbols: Vec<Symbol>,
    pub bindings: Vec<Binding>,
    pub classes: Vec<ClassInfo>,
    pub references: Vec<SymbolReference>,
    pub resolved_references: Vec<ResolvedReference>,
    pub captures: Vec<Capture>,
    pub diagnostics: Vec<SemanticDiagnostic>,
}

impl AnalysisResult {
    pub fn has_errors(&self) -> bool {
        !self.diagnostics.is_empty()
    }
}

pub fn analyze_compilation_unit(unit: &CompilationUnit) -> AnalysisResult {
    let mut binder = Binder::new();
    binder.bind_compilation_unit(unit);
    binder.finish()
}

pub fn analyze_compilation_unit_with_source_context(
    unit: &CompilationUnit,
    source_file: Option<PathBuf>,
) -> AnalysisResult {
    let mut binder = Binder::new_with_source_file(source_file);
    binder.bind_compilation_unit(unit);
    binder.finish()
}

#[derive(Default)]
struct ScopeSymbolTable {
    values: HashMap<String, SymbolId>,
    functions: HashMap<String, SymbolId>,
}

#[derive(Debug, Clone, Copy)]
struct ResolvedSymbol {
    id: SymbolId,
    kind: SymbolKind,
    scope_id: ScopeId,
    workspace_id: WorkspaceId,
    binding_id: Option<BindingId>,
}

struct Binder {
    scopes: Vec<Scope>,
    workspaces: Vec<Workspace>,
    symbols: Vec<Symbol>,
    bindings: Vec<Binding>,
    classes: Vec<ClassInfo>,
    references: Vec<SymbolReference>,
    captures: Vec<Capture>,
    diagnostics: Vec<SemanticDiagnostic>,
    next_scope_id: u32,
    next_workspace_id: u32,
    next_symbol_id: u32,
    next_binding_id: u32,
    scope_symbols: HashMap<ScopeId, ScopeSymbolTable>,
    capture_index: HashMap<(ScopeId, SymbolId), usize>,
    global_bindings: HashMap<String, BindingId>,
    persistent_bindings: HashMap<(WorkspaceId, String), BindingId>,
    source_file: Option<PathBuf>,
    property_default_depth: u32,
}

const BUILTIN_FUNCTIONS: &[&str] = &[
    "size",
    "length",
    "numel",
    "ndims",
    "isempty",
    "isscalar",
    "isvector",
    "ismatrix",
    "isrow",
    "iscolumn",
    "deal",
    "struct",
    "arrayfun",
    "cellfun",
    "structfun",
    "cell",
    "zeros",
    "ones",
    "true",
    "false",
    "uplus",
    "uminus",
    "plus",
    "minus",
    "times",
    "rdivide",
    "ldivide",
    "power",
    "eq",
    "ne",
    "eye",
    "reshape",
    "squeeze",
    "shiftdim",
    "transpose",
    "pagetranspose",
    "pagectranspose",
    "linspace",
    "meshgrid",
    "ndgrid",
    "histcounts",
    "histcounts2",
    "interp1",
    "interp2",
    "interp3",
    "interpn",
    "path",
    "addpath",
    "rmpath",
    "genpath",
    "restoredefaultpath",
    "rehash",
    "userpath",
    "savepath",
    "path2rc",
    "matlabroot",
    "pathdef",
    "pathsep",
    "filesep",
    "mkdir",
    "rmdir",
    "copyfile",
    "movefile",
    "fileattrib",
    "pathtool",
    "toolboxdir",
    "fullfile",
    "fileparts",
    "version",
    "ver",
    "verLessThan",
    "computer",
    "tempdir",
    "tempname",
    "license",
    "fopen",
    "openedFiles",
    "fclose",
    "feof",
    "ferror",
    "fread",
    "fscanf",
    "frewind",
    "fseek",
    "ftell",
    "fgetl",
    "fgets",
    "fwrite",
    "sscanf",
    "fileread",
    "type",
    "isfile",
    "isfolder",
    "isdir",
    "pwd",
    "cd",
    "dir",
    "ls",
    "linsolve",
    "accumarray",
    "diff",
    "conv",
    "conv2",
    "convn",
    "cconv",
    "convmtx",
    "corrmtx",
    "toeplitz",
    "hankel",
    "vander",
    "hilb",
    "invhilb",
    "pascal",
    "compan",
    "filter2",
    "deconv",
    "filter",
    "ctffilt",
    "filtfilt",
    "fftfilt",
    "sosfilt",
    "ctf",
    "digitalFilter",
    "cascade",
    "getNumStages",
    "setSampleRate",
    "scaleFilterSections",
    "fft",
    "ifft",
    "fft2",
    "ifft2",
    "fftn",
    "ifftn",
    "dftmtx",
    "freqz",
    "phasez",
    "zerophase",
    "grpdelay",
    "phasedelay",
    "zplane",
    "isstable",
    "isfir",
    "isiir",
    "islinphase",
    "firtype",
    "isminphase",
    "ismaxphase",
    "isallpass",
    "eqtflength",
    "filtord",
    "impzlength",
    "filtic",
    "freqs",
    "impz",
    "stepz",
    "sos2cell",
    "cell2sos",
    "sos2ctf",
    "sos2ss",
    "sos2zp",
    "sos2tf",
    "ss2sos",
    "ss2zp",
    "tf2ss",
    "imfilter",
    "poly",
    "roots",
    "residuez",
    "residue",
    "ss2tf",
    "tf2ctf",
    "tf2zp",
    "tf2sos",
    "zp2tf",
    "zp2ctf",
    "zp2sos",
    "zp2ss",
    "ctf2zp",
    "ctf2sos",
    "ss",
    "tf",
    "zpk",
    "polyfit",
    "polyder",
    "polyint",
    "polyval",
    "polyvalm",
    "del2",
    "divergence",
    "curl",
    "gradient",
    "trapz",
    "cumtrapz",
    "cummax",
    "cummin",
    "flip",
    "flipud",
    "fliplr",
    "rot90",
    "circshift",
    "fftshift",
    "ifftshift",
    "repelem",
    "cumsum",
    "cumprod",
    "movsum",
    "movmean",
    "movmax",
    "movmin",
    "movprod",
    "movmedian",
    "movmad",
    "movvar",
    "movstd",
    "unique",
    "horzcat",
    "vertcat",
    "cat",
    "blkdiag",
    "num2cell",
    "mat2cell",
    "cell2struct",
    "struct2cell",
    "cell2mat",
    "cellstr",
    "permute",
    "ipermute",
    "single",
    "double",
    "isdouble",
    "issingle",
    "info",
    "logical",
    "int64",
    "uint64",
    "sum",
    "prod",
    "exp",
    "log",
    "floor",
    "ceil",
    "fix",
    "round",
    "sign",
    "isfinite",
    "isinf",
    "isnan",
    "sin",
    "cos",
    "sinh",
    "cosh",
    "sqrt",
    "tan",
    "sec",
    "csc",
    "cot",
    "deg2rad",
    "rad2deg",
    "log10",
    "log2",
    "pow2",
    "sind",
    "cosd",
    "tand",
    "asin",
    "acos",
    "atan",
    "asind",
    "acosd",
    "atand",
    "atan2",
    "atan2d",
    "mod",
    "rem",
    "hypot",
    "tanh",
    "sech",
    "csch",
    "coth",
    "asinh",
    "acosh",
    "atanh",
    "asec",
    "acsc",
    "acot",
    "asech",
    "acsch",
    "acoth",
    "abs",
    "angle",
    "unwrap",
    "mag2db",
    "db2mag",
    "pow2db",
    "db2pow",
    "real",
    "imag",
    "conj",
    "complex",
    "expm",
    "sqrtm",
    "logm",
    "asinm",
    "acosm",
    "atanm",
    "sinm",
    "cosm",
    "tanm",
    "secm",
    "cscm",
    "cotm",
    "asinhm",
    "acoshm",
    "atanhm",
    "asecm",
    "acscm",
    "acotm",
    "asechm",
    "acschm",
    "acothm",
    "sinhm",
    "coshm",
    "tanhm",
    "sechm",
    "cschm",
    "cothm",
    "funm",
    "min",
    "max",
    "nanmin",
    "nanmax",
    "bounds",
    "maxk",
    "mink",
    "mean",
    "mean2",
    "nanmean",
    "median",
    "nanmedian",
    "mode",
    "mad",
    "geomean",
    "harmmean",
    "trimmean",
    "rms",
    "moment",
    "skewness",
    "kurtosis",
    "nansum",
    "nanprod",
    "var",
    "nanvar",
    "std",
    "std2",
    "stdfilt",
    "ordfilt2",
    "medfilt2",
    "medfilt3",
    "wiener2",
    "modefilt",
    "padarray",
    "imboxfilt",
    "imboxfilt3",
    "imgaussfilt",
    "imgaussfilt3",
    "nlfilter",
    "colfilt",
    "im2col",
    "col2im",
    "blockproc",
    "nanstd",
    "corr2",
    "xcorr2",
    "normxcorr2",
    "immse",
    "psnr",
    "ssim",
    "multissim",
    "multissim3",
    "entropy",
    "entropyfilt",
    "rangefilt",
    "nancov",
    "cov",
    "corr",
    "corrcoef",
    "partialcorr",
    "xcorr",
    "xcov",
    "quantile",
    "prctile",
    "iqr",
    "range",
    "zscore",
    "any",
    "all",
    "ismember",
    "union",
    "intersect",
    "setdiff",
    "setxor",
    "nnz",
    "isequal",
    "find",
    "issorted",
    "issortedrows",
    "sort",
    "sortrows",
    "diag",
    "repmat",
    "isstruct",
    "iscell",
    "iscellstr",
    "isobject",
    "isnumeric",
    "isreal",
    "islogical",
    "ischar",
    "isstring",
    "class",
    "isa",
    "isfield",
    "fieldnames",
    "orderfields",
    "rmfield",
    "getfield",
    "setfield",
    "qr",
    "lu",
    "chol",
    "ldl",
    "rref",
    "rank",
    "cond",
    "rcond",
    "pinv",
    "norm",
    "orth",
    "null",
    "trace",
    "det",
    "inv",
    "svd",
    "eig",
    "issymmetric",
    "ishermitian",
    "istriu",
    "istril",
    "triu",
    "tril",
    "mldivide",
    "mrdivide",
    "mtimes",
    "mpower",
    "ctranspose",
    "char",
    "string",
    "num2str",
    "int2str",
    "disp",
    "display",
    "sprintf",
    "fprintf",
    "clc",
    "format",
    "clear",
    "clearvars",
    "save",
    "load",
    "who",
    "whos",
    "tic",
    "toc",
    "drawnow",
    "animatedline",
    "addpoints",
    "clearpoints",
    "getpoints",
    "fill",
    "patch",
    "pause",
    "pi",
    "eps",
    "realmin",
    "realmax",
    "flintmax",
    "inf",
    "Inf",
    "nan",
    "NaN",
    "strcmp",
    "strcmpi",
    "strncmp",
    "strncmpi",
    "functions",
    "localfunctions",
    "exist",
    "inmem",
    "help",
    "lookfor",
    "doc",
    "methods",
    "properties",
    "isprop",
    "ismethod",
    "superclasses",
    "events",
    "what",
    "which",
    "filemarker",
    "func2str",
    "str2func",
    "strlength",
    "str2double",
    "str2num",
    "base2dec",
    "dec2base",
    "dec2hex",
    "dec2bin",
    "hex2dec",
    "bin2dec",
    "contains",
    "count",
    "strfind",
    "matches",
    "upper",
    "lower",
    "mat2str",
    "startsWith",
    "endsWith",
    "append",
    "erase",
    "replace",
    "strrep",
    "strcat",
    "split",
    "splitlines",
    "strip",
    "strtrim",
    "deblank",
    "strjoin",
    "strsplit",
    "strtok",
    "join",
    "extractBefore",
    "extractAfter",
    "extractBetween",
    "replaceBefore",
    "replaceAfter",
    "replaceBetween",
    "pad",
    "compose",
    "MException",
    "addCause",
    "getReport",
    "error",
    "warning",
    "lastwarn",
    "throw",
    "rethrow",
    "throwAsCaller",
    "figure",
    "gcf",
    "gca",
    "gco",
    "clf",
    "cla",
    "close",
    "closereq",
    "allchild",
    "ancestor",
    "ishghandle",
    "isgraphics",
    "delete",
    "copyobj",
    "reset",
    "findobj",
    "findall",
    "axes",
    "subplot",
    "tiledlayout",
    "nexttile",
    "hold",
    "ishold",
    "get",
    "set",
    "line",
    "xline",
    "yline",
    "plot",
    "fplot",
    "fsurf",
    "fmesh",
    "fimplicit",
    "fcontour",
    "fcontour3",
    "fplot3",
    "plot3",
    "plotyy",
    "errorbar",
    "semilogx",
    "semilogy",
    "loglog",
    "scatter",
    "scatter3",
    "quiver",
    "quiver3",
    "pie",
    "pie3",
    "histogram",
    "histogram2",
    "area",
    "stairs",
    "bar",
    "barh",
    "stem",
    "stem3",
    "contour",
    "contour3",
    "contourf",
    "mesh",
    "meshc",
    "meshz",
    "waterfall",
    "ribbon",
    "bar3",
    "bar3h",
    "surf",
    "surfc",
    "image",
    "imagesc",
    "imshow",
    "text",
    "rectangle",
    "annotation",
    "fill3",
    "axis",
    "view",
    "grid",
    "box",
    "xscale",
    "yscale",
    "shading",
    "caxis",
    "colormap",
    "colorbar",
    "legend",
    "sgtitle",
    "title",
    "subtitle",
    "xlabel",
    "ylabel",
    "zlabel",
    "yyaxis",
    "rotate3d",
    "linkaxes",
    "xticks",
    "yticks",
    "zticks",
    "xticklabels",
    "yticklabels",
    "zticklabels",
    "xtickangle",
    "ytickangle",
    "ztickangle",
    "xlim",
    "ylim",
    "zlim",
    "print",
    "saveas",
    "exportgraphics",
];

impl Binder {
    fn new() -> Self {
        Self::new_with_source_file(None)
    }

    fn new_with_source_file(source_file: Option<PathBuf>) -> Self {
        Self {
            scopes: Vec::new(),
            workspaces: Vec::new(),
            symbols: Vec::new(),
            bindings: Vec::new(),
            classes: Vec::new(),
            references: Vec::new(),
            captures: Vec::new(),
            diagnostics: Vec::new(),
            next_scope_id: 0,
            next_workspace_id: 0,
            next_symbol_id: 0,
            next_binding_id: 0,
            scope_symbols: HashMap::new(),
            capture_index: HashMap::new(),
            global_bindings: HashMap::new(),
            persistent_bindings: HashMap::new(),
            source_file,
            property_default_depth: 0,
        }
    }

    fn finish(self) -> AnalysisResult {
        AnalysisResult {
            scopes: self.scopes,
            workspaces: self.workspaces,
            symbols: self.symbols,
            bindings: self.bindings,
            classes: self.classes,
            references: self.references,
            resolved_references: Vec::new(),
            captures: self.captures,
            diagnostics: self.diagnostics,
        }
    }

    fn bind_compilation_unit(&mut self, unit: &CompilationUnit) {
        let root_scope = self.alloc_scope(None, ScopeKind::CompilationUnit, WorkspaceId(0));

        let workspace_kind = match unit.kind {
            CompilationUnitKind::Script => WorkspaceKind::Script,
            CompilationUnitKind::FunctionFile => WorkspaceKind::Function,
            CompilationUnitKind::ClassFile => WorkspaceKind::Class,
        };
        let root_workspace = self.alloc_workspace(None, workspace_kind, root_scope, None);
        self.patch_scope_workspace(root_scope, root_workspace);
        if workspace_kind != WorkspaceKind::Class {
            self.declare_ans_symbol(root_scope, root_workspace);
            self.predeclare_top_level_functions(unit, root_scope, root_workspace);
        }

        match unit.kind {
            CompilationUnitKind::Script => {
                for item in &unit.items {
                    match item {
                        Item::Statement(statement) => {
                            self.bind_statement(statement, root_scope, root_workspace);
                        }
                        Item::Function(function) => {
                            self.bind_function(function, root_scope, root_workspace);
                        }
                        Item::Class(class_def) => {
                            self.diagnostics.push(SemanticDiagnostic::error(
                                "SEM009",
                                format!(
                                    "class definition `{}` is not valid inside a script file",
                                    class_def.name.name
                                ),
                                class_def.span,
                            ));
                        }
                    }
                }
            }
            CompilationUnitKind::FunctionFile => {
                for item in &unit.items {
                    match item {
                        Item::Function(function) => {
                            self.bind_function(function, root_scope, root_workspace)
                        }
                        Item::Statement(statement) => {
                            self.diagnostics.push(SemanticDiagnostic::error(
                                "SEM001",
                                "top-level statements are not expected before the primary function in a function file",
                                statement.span,
                            ));
                        }
                        Item::Class(class_def) => {
                            self.diagnostics.push(SemanticDiagnostic::error(
                                "SEM004",
                                format!(
                                    "class definition `{}` is not valid inside a function file",
                                    class_def.name.name
                                ),
                                class_def.span,
                            ));
                        }
                    }
                }
            }
            CompilationUnitKind::ClassFile => {
                for item in &unit.items {
                    match item {
                        Item::Class(class_def) => {
                            self.bind_class_definition(class_def, root_scope, root_workspace)
                        }
                        Item::Function(function) => {
                            self.diagnostics.push(SemanticDiagnostic::error(
                                "SEM005",
                                format!(
                                    "top-level function `{}` is not valid outside a methods block in a class file",
                                    function.name.name
                                ),
                                function.span,
                            ));
                        }
                        Item::Statement(statement) => {
                            self.diagnostics.push(SemanticDiagnostic::error(
                                "SEM006",
                                "top-level statements are not valid in a class file",
                                statement.span,
                            ));
                        }
                    }
                }
            }
        }
    }

    fn predeclare_top_level_functions(
        &mut self,
        unit: &CompilationUnit,
        scope_id: ScopeId,
        workspace_id: WorkspaceId,
    ) {
        for item in &unit.items {
            if let Item::Function(function) = item {
                self.declare_symbol(
                    scope_id,
                    workspace_id,
                    &function.name.name,
                    SymbolKind::Function,
                    function.name.span,
                );
            }
        }
    }

    fn bind_class_definition(
        &mut self,
        class_def: &ClassDef,
        parent_scope: ScopeId,
        parent_workspace: WorkspaceId,
    ) {
        self.declare_symbol(
            parent_scope,
            parent_workspace,
            &class_def.name.name,
            SymbolKind::Class,
            class_def.name.span,
        );
        let class_scope =
            self.alloc_scope(Some(parent_scope), ScopeKind::ClassBody, WorkspaceId(0));
        let class_workspace = self.alloc_workspace(
            Some(parent_workspace),
            WorkspaceKind::Class,
            class_scope,
            Some(class_def.name.name.clone()),
        );
        self.patch_scope_workspace(class_scope, class_workspace);

        for block in &class_def.property_blocks {
            self.predeclare_class_properties(block, class_scope, class_workspace);
        }
        for block in &class_def.method_blocks {
            self.predeclare_class_methods(block, class_scope, class_workspace);
        }
        for block in &class_def.property_blocks {
            self.bind_class_property_block(block, class_scope);
        }
        for block in &class_def.method_blocks {
            for method in &block.methods {
                self.bind_method(method, class_scope, class_workspace);
            }
        }

        let package = self.class_package_name();
        let inline_methods = class_def
            .method_blocks
            .iter()
            .filter(|block| !block.is_static)
            .flat_map(|block| block.methods.iter())
            .map(|method| method.name.name.clone())
            .collect::<Vec<_>>();
        let static_inline_methods = class_def
            .method_blocks
            .iter()
            .filter(|block| block.is_static)
            .flat_map(|block| block.methods.iter())
            .map(|method| method.name.name.clone())
            .collect::<Vec<_>>();
        let external_methods =
            self.discover_external_class_methods(&class_def.name.name, class_def.name.span);
        let constructor = inline_methods
            .iter()
            .find(|method| method.eq_ignore_ascii_case(&class_def.name.name))
            .cloned();
        let properties = class_def
            .property_blocks
            .iter()
            .flat_map(|block| {
                block.properties.iter().map(|property| ClassPropertyInfo {
                    name: property.name.name.clone(),
                    access: block.access,
                    default: property.default.clone(),
                })
            })
            .collect::<Vec<_>>();
        let private_properties = class_def
            .property_blocks
            .iter()
            .filter(|block| block.access == ClassMemberAccess::Private)
            .flat_map(|block| block.properties.iter())
            .map(|property| property.name.name.clone())
            .collect::<Vec<_>>();
        let private_inline_methods = class_def
            .method_blocks
            .iter()
            .filter(|block| !block.is_static && block.access == ClassMemberAccess::Private)
            .flat_map(|block| block.methods.iter())
            .map(|method| method.name.name.clone())
            .collect::<Vec<_>>();
        let private_static_inline_methods = class_def
            .method_blocks
            .iter()
            .filter(|block| block.is_static && block.access == ClassMemberAccess::Private)
            .flat_map(|block| block.methods.iter())
            .map(|method| method.name.name.clone())
            .collect::<Vec<_>>();
        let superclass_name = class_def.superclass.as_ref().map(|superclass| {
            superclass
                .segments
                .iter()
                .map(|segment| segment.name.clone())
                .collect::<Vec<_>>()
                .join(".")
        });
        let inherits_handle = superclass_name
            .as_deref()
            .is_some_and(|superclass| superclass.eq_ignore_ascii_case("handle"));
        self.classes.push(ClassInfo {
            name: class_def.name.name.clone(),
            package,
            superclass_name,
            superclass_path: None,
            inherits_handle,
            properties,
            inline_methods,
            static_inline_methods,
            private_properties,
            private_inline_methods,
            private_static_inline_methods,
            external_methods,
            constructor,
            source_path: self.source_file.clone(),
        });
    }

    fn predeclare_class_properties(
        &mut self,
        block: &ClassPropertyBlock,
        scope_id: ScopeId,
        workspace_id: WorkspaceId,
    ) {
        for property in &block.properties {
            self.declare_symbol(
                scope_id,
                workspace_id,
                &property.name.name,
                SymbolKind::Property,
                property.name.span,
            );
        }
    }

    fn predeclare_class_methods(
        &mut self,
        block: &ClassMethodBlock,
        scope_id: ScopeId,
        workspace_id: WorkspaceId,
    ) {
        for method in &block.methods {
            self.declare_symbol(
                scope_id,
                workspace_id,
                &method.name.name,
                SymbolKind::Method,
                method.name.span,
            );
        }
    }

    fn bind_class_property_block(&mut self, block: &ClassPropertyBlock, scope_id: ScopeId) {
        for property in &block.properties {
            if let Some(default) = &property.default {
                self.property_default_depth += 1;
                self.bind_expression(default, scope_id);
                self.property_default_depth -= 1;
            }
        }
    }

    fn bind_method(
        &mut self,
        function: &FunctionDef,
        parent_scope: ScopeId,
        parent_workspace: WorkspaceId,
    ) {
        self.bind_function(function, parent_scope, parent_workspace);
    }

    fn class_package_name(&self) -> Option<String> {
        let path = self.source_file.as_deref()?;
        let mut parts = Vec::new();
        let mut current = path.parent();
        while let Some(dir) = current {
            let Some(name) = dir.file_name().and_then(|value| value.to_str()) else {
                break;
            };
            if let Some(package) = name.strip_prefix('+') {
                parts.push(package.to_string());
            }
            current = dir.parent();
        }
        if parts.is_empty() {
            None
        } else {
            parts.reverse();
            Some(parts.join("."))
        }
    }

    fn discover_external_class_methods(
        &mut self,
        class_name: &str,
        span: SourceSpan,
    ) -> Vec<ExternalMethodInfo> {
        let Some(source_file) = self.source_file.as_deref() else {
            return Vec::new();
        };
        let Some(class_dir) = source_file.parent() else {
            return Vec::new();
        };
        let Some(class_dir_name) = class_dir.file_name().and_then(|value| value.to_str()) else {
            return Vec::new();
        };
        let Some(folder_class_name) = class_dir_name.strip_prefix('@') else {
            return Vec::new();
        };
        if !folder_class_name.eq_ignore_ascii_case(class_name) {
            self.diagnostics.push(SemanticDiagnostic::error(
                "SEM008",
                format!(
                    "class file `{}` does not match enclosing class folder `@{folder_class_name}`",
                    source_file.display()
                ),
                span,
            ));
            return Vec::new();
        }

        let mut methods = Vec::new();
        let Ok(entries) = fs::read_dir(class_dir) else {
            return methods;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path == source_file {
                continue;
            }
            if !path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("m"))
            {
                continue;
            }
            let Some(name) = path.file_stem().and_then(|value| value.to_str()) else {
                continue;
            };
            methods.push(ExternalMethodInfo {
                name: name.to_string(),
                path,
            });
        }
        methods.sort_by(|lhs, rhs| lhs.name.cmp(&rhs.name));
        methods
    }

    fn bind_function(
        &mut self,
        function: &FunctionDef,
        parent_scope: ScopeId,
        parent_workspace: WorkspaceId,
    ) {
        let function_scope =
            self.alloc_scope(Some(parent_scope), ScopeKind::FunctionBody, WorkspaceId(0));
        let function_workspace = self.alloc_workspace(
            Some(parent_workspace),
            WorkspaceKind::Function,
            function_scope,
            Some(function.name.name.clone()),
        );
        self.patch_scope_workspace(function_scope, function_workspace);
        self.declare_ans_symbol(function_scope, function_workspace);
        self.predeclare_local_functions(function, function_scope, function_workspace);

        for input in &function.inputs {
            self.declare_symbol(
                function_scope,
                function_workspace,
                &input.name,
                SymbolKind::Parameter,
                input.span,
            );
        }

        for output in &function.outputs {
            self.declare_symbol(
                function_scope,
                function_workspace,
                &output.name,
                SymbolKind::Output,
                output.span,
            );
        }

        for statement in &function.body {
            self.bind_statement(statement, function_scope, function_workspace);
        }

        for nested in &function.local_functions {
            self.bind_function(nested, function_scope, function_workspace);
        }
    }

    fn predeclare_local_functions(
        &mut self,
        function: &FunctionDef,
        scope_id: ScopeId,
        workspace_id: WorkspaceId,
    ) {
        for nested in &function.local_functions {
            self.declare_symbol(
                scope_id,
                workspace_id,
                &nested.name.name,
                SymbolKind::Function,
                nested.name.span,
            );
        }
    }

    fn bind_statement(
        &mut self,
        statement: &Statement,
        scope_id: ScopeId,
        workspace_id: WorkspaceId,
    ) {
        match &statement.kind {
            StatementKind::Assignment { targets, value, .. } => {
                if self.expression_prefers_zero_arg_builtin_call(value, scope_id) {
                    self.bind_apply_target(value, scope_id);
                } else {
                    self.bind_expression(value, scope_id);
                }
                for target in targets {
                    self.bind_assignment_target(target, scope_id, workspace_id);
                }
            }
            StatementKind::Expression(expression) => {
                self.bind_expression_statement(expression, scope_id);
                self.bind_load_side_effects(expression, scope_id, workspace_id);
            }
            StatementKind::If {
                branches,
                else_body,
            } => {
                for branch in branches {
                    self.bind_expression(&branch.condition, scope_id);
                    for statement in &branch.body {
                        self.bind_statement(statement, scope_id, workspace_id);
                    }
                }
                for statement in else_body {
                    self.bind_statement(statement, scope_id, workspace_id);
                }
            }
            StatementKind::Switch {
                expression,
                cases,
                otherwise_body,
            } => {
                self.bind_expression(expression, scope_id);
                for case in cases {
                    self.bind_expression(&case.matcher, scope_id);
                    for statement in &case.body {
                        self.bind_statement(statement, scope_id, workspace_id);
                    }
                }
                for statement in otherwise_body {
                    self.bind_statement(statement, scope_id, workspace_id);
                }
            }
            StatementKind::Try {
                body,
                catch_binding,
                catch_body,
            } => {
                for statement in body {
                    self.bind_statement(statement, scope_id, workspace_id);
                }
                if let Some(binding) = catch_binding {
                    self.declare_symbol(
                        scope_id,
                        workspace_id,
                        &binding.name,
                        SymbolKind::Variable,
                        binding.span,
                    );
                }
                for statement in catch_body {
                    self.bind_statement(statement, scope_id, workspace_id);
                }
            }
            StatementKind::For {
                variable,
                iterable,
                body,
            } => {
                self.bind_expression(iterable, scope_id);
                self.declare_symbol(
                    scope_id,
                    workspace_id,
                    &variable.name,
                    SymbolKind::Variable,
                    variable.span,
                );
                for statement in body {
                    self.bind_statement(statement, scope_id, workspace_id);
                }
            }
            StatementKind::While { condition, body } => {
                self.bind_expression(condition, scope_id);
                for statement in body {
                    self.bind_statement(statement, scope_id, workspace_id);
                }
            }
            StatementKind::Break | StatementKind::Continue | StatementKind::Return => {}
            StatementKind::Global(names) => {
                for name in names {
                    self.declare_symbol(
                        scope_id,
                        workspace_id,
                        &name.name,
                        SymbolKind::Global,
                        name.span,
                    );
                }
            }
            StatementKind::Persistent(names) => {
                if self.workspace(scope_id).kind != WorkspaceKind::Function {
                    self.diagnostics.push(SemanticDiagnostic::error(
                        "SEM003",
                        "`persistent` declarations are only valid inside function workspaces",
                        statement.span,
                    ));
                }
                for name in names {
                    self.declare_symbol(
                        scope_id,
                        workspace_id,
                        &name.name,
                        SymbolKind::Persistent,
                        name.span,
                    );
                }
            }
        }
    }

    fn bind_assignment_target(
        &mut self,
        target: &AssignmentTarget,
        scope_id: ScopeId,
        workspace_id: WorkspaceId,
    ) {
        match target {
            AssignmentTarget::Identifier(identifier) => {
                self.bind_identifier_assignment(identifier, scope_id, workspace_id)
            }
            AssignmentTarget::Index { target, indices } => {
                self.bind_assignment_expression_root(
                    target,
                    scope_id,
                    workspace_id,
                    CaptureAccess::ReadWrite,
                );
                self.bind_index_arguments(indices, scope_id);
            }
            AssignmentTarget::CellIndex { target, indices } => {
                self.bind_assignment_expression_root(
                    target,
                    scope_id,
                    workspace_id,
                    CaptureAccess::ReadWrite,
                );
                self.bind_index_arguments(indices, scope_id);
            }
            AssignmentTarget::Field { target, .. } => self.bind_assignment_expression_root(
                target,
                scope_id,
                workspace_id,
                CaptureAccess::ReadWrite,
            ),
        }
    }

    fn declare_ans_symbol(&mut self, scope_id: ScopeId, workspace_id: WorkspaceId) {
        self.declare_symbol(
            scope_id,
            workspace_id,
            "ans",
            SymbolKind::Variable,
            SourceSpan::new(
                SourceFileId(0),
                SourcePosition::new(0, 0, 0),
                SourcePosition::new(0, 0, 0),
            ),
        );
    }

    fn bind_identifier_assignment(
        &mut self,
        identifier: &Identifier,
        scope_id: ScopeId,
        workspace_id: WorkspaceId,
    ) {
        self.bind_identifier_assignment_with_access(
            identifier,
            scope_id,
            workspace_id,
            CaptureAccess::Write,
            false,
        );
    }

    fn bind_assignment_expression_root(
        &mut self,
        expression: &Expression,
        scope_id: ScopeId,
        workspace_id: WorkspaceId,
        root_access: CaptureAccess,
    ) {
        match &expression.kind {
            ExpressionKind::Identifier(identifier) => self.bind_identifier_assignment_with_access(
                identifier,
                scope_id,
                workspace_id,
                root_access,
                true,
            ),
            ExpressionKind::ParenApply { target, indices } => {
                self.bind_assignment_expression_root(target, scope_id, workspace_id, root_access);
                self.bind_index_arguments(indices, scope_id);
            }
            ExpressionKind::CellIndex { target, indices } => {
                self.bind_assignment_expression_root(target, scope_id, workspace_id, root_access);
                self.bind_index_arguments(indices, scope_id);
            }
            ExpressionKind::FieldAccess { target, .. } => {
                self.bind_assignment_expression_root(target, scope_id, workspace_id, root_access);
            }
            _ => self.bind_expression(expression, scope_id),
        }
    }

    fn bind_identifier_assignment_with_access(
        &mut self,
        identifier: &Identifier,
        scope_id: ScopeId,
        workspace_id: WorkspaceId,
        access: CaptureAccess,
        push_reference: bool,
    ) {
        if self.current_scope_declares_value(scope_id, &identifier.name) {
            if push_reference {
                let resolved = self
                    .lookup_resolvable_value_symbol(scope_id, &identifier.name)
                    .expect("current scope declaration should remain resolvable");
                self.push_reference(
                    identifier,
                    ReferenceRole::Value,
                    ReferenceResolution::WorkspaceValue,
                    Some(resolved),
                    None,
                );
            }
            return;
        }

        if let Some(resolved) =
            self.lookup_capture_value_symbol_in_ancestors(scope_id, &identifier.name)
        {
            self.record_capture(scope_id, &identifier.name, resolved, access);
            if push_reference {
                self.push_reference(
                    identifier,
                    ReferenceRole::Value,
                    ReferenceResolution::WorkspaceValue,
                    Some(resolved),
                    Some(access),
                );
            }
            return;
        }

        self.declare_symbol(
            scope_id,
            workspace_id,
            &identifier.name,
            SymbolKind::Variable,
            identifier.span,
        );

        if push_reference {
            let resolved = self
                .lookup_current_value_symbol(scope_id, &identifier.name)
                .expect("newly declared assignment root should resolve");
            self.push_reference(
                identifier,
                ReferenceRole::Value,
                ReferenceResolution::WorkspaceValue,
                Some(resolved),
                None,
            );
        }
    }

    fn bind_expression_statement(&mut self, expression: &Expression, scope_id: ScopeId) {
        if self.expression_prefers_zero_arg_call(expression, scope_id) {
            self.bind_apply_target(expression, scope_id);
        } else {
            self.bind_expression(expression, scope_id);
        }
    }

    fn bind_load_side_effects(
        &mut self,
        expression: &Expression,
        scope_id: ScopeId,
        workspace_id: WorkspaceId,
    ) {
        let Some((path, selected_names)) = self.extract_literal_load_call(expression, scope_id)
        else {
            return;
        };
        let Ok(workspace) = self.read_load_workspace(&path) else {
            return;
        };
        for name in selected_names.unwrap_or_else(|| workspace.keys().cloned().collect()) {
            if !workspace.contains_key(&name) || self.current_scope_declares_value(scope_id, &name)
            {
                continue;
            }
            self.declare_symbol(
                scope_id,
                workspace_id,
                &name,
                SymbolKind::Variable,
                expression.span,
            );
        }
    }

    fn extract_literal_load_call(
        &self,
        expression: &Expression,
        scope_id: ScopeId,
    ) -> Option<(PathBuf, Option<Vec<String>>)> {
        let ExpressionKind::ParenApply { target, indices } = &expression.kind else {
            return None;
        };
        let ExpressionKind::Identifier(identifier) = &target.kind else {
            return None;
        };
        if identifier.name != "load"
            || self
                .lookup_resolvable_value_symbol(scope_id, "load")
                .is_some()
        {
            return None;
        }
        let (first, rest) = indices.split_first()?;
        let IndexArgument::Expression(first) = first else {
            return None;
        };
        let filename = literal_text_value(first)?;
        let names = self.select_loaded_names(rest, &self.resolve_load_path(&filename))?;
        Some((self.resolve_load_path(&filename), names))
    }

    fn resolve_load_path(&self, filename: &str) -> PathBuf {
        let path = PathBuf::from(filename);
        let path = if path.is_absolute() {
            path
        } else {
            let cwd_candidate = std::env::current_dir().ok().map(|cwd| cwd.join(&path));
            let source_candidate = self
                .source_file
                .as_deref()
                .and_then(Path::parent)
                .map(|dir| dir.join(&path));
            if let Some(candidate) = cwd_candidate
                .as_ref()
                .filter(|candidate| candidate.exists())
            {
                candidate.clone()
            } else if let Some(candidate) = source_candidate
                .as_ref()
                .filter(|candidate| candidate.exists())
            {
                candidate.clone()
            } else if let Some(candidate) = cwd_candidate {
                candidate
            } else if let Some(candidate) = source_candidate {
                candidate
            } else {
                path
            }
        };
        if path.extension().is_some() {
            path
        } else {
            path.with_extension("mat")
        }
    }

    fn read_load_workspace(
        &self,
        path: &Path,
    ) -> Result<std::collections::BTreeMap<String, matlab_runtime::Value>, ()> {
        let extension = path.extension().and_then(|value| value.to_str());
        let result = if matches!(extension, Some(ext) if ext.eq_ignore_ascii_case("matws")) {
            read_workspace_snapshot(path)
        } else {
            read_mat_file(path)
        };
        result.map_err(|_| ())
    }

    fn select_loaded_names(
        &self,
        args: &[IndexArgument],
        path: &Path,
    ) -> Option<Option<Vec<String>>> {
        if args.is_empty() {
            return Some(None);
        }
        let workspace = self.read_load_workspace(path).ok()?;
        let visible_names = workspace.keys().cloned().collect::<Vec<_>>();
        let mut exact = Vec::new();
        let mut regexes = Vec::new();
        let mut regex_mode = false;
        for argument in args {
            let IndexArgument::Expression(expression) = argument else {
                return None;
            };
            let text = literal_text_value(expression)?;
            if text.eq_ignore_ascii_case("-regexp") {
                regex_mode = true;
                continue;
            }
            if regex_mode {
                regexes.push(text);
            } else {
                exact.push(text);
            }
        }
        if exact.is_empty() && regexes.is_empty() {
            return Some(None);
        }
        let mut selected = std::collections::BTreeSet::new();
        for name in exact {
            if has_wildcard_pattern(&name) {
                for visible in &visible_names {
                    if matlab_wildcard_is_match(&name, visible) {
                        selected.insert(visible.clone());
                    }
                }
            } else if workspace.contains_key(&name) {
                selected.insert(name);
            }
        }
        for pattern in regexes {
            for visible in &visible_names {
                if matlab_regexp_is_match(&pattern, visible) {
                    selected.insert(visible.clone());
                }
            }
        }
        Some(Some(selected.into_iter().collect()))
    }

    fn expression_prefers_zero_arg_call(&self, expression: &Expression, scope_id: ScopeId) -> bool {
        let Some(qualified) = self.expression_as_qualified_name(expression) else {
            return false;
        };

        let root = &qualified.segments[0].name;
        self.lookup_resolvable_value_symbol(scope_id, root)
            .is_none()
            && !is_builtin_value(root)
    }

    fn expression_prefers_zero_arg_builtin_call(
        &self,
        expression: &Expression,
        scope_id: ScopeId,
    ) -> bool {
        let Some(qualified) = self.expression_as_qualified_name(expression) else {
            return false;
        };
        if qualified.segments.len() != 1 {
            return false;
        }

        let root = &qualified.segments[0].name;
        self.lookup_resolvable_value_symbol(scope_id, root)
            .is_none()
            && !is_builtin_value(root)
            && is_builtin_function_name(root)
    }

    fn bind_expression(&mut self, expression: &Expression, scope_id: ScopeId) {
        match &expression.kind {
            ExpressionKind::Identifier(identifier) => {
                self.resolve_identifier(scope_id, identifier, ReferenceRole::Value)
            }
            ExpressionKind::NumberLiteral(_)
            | ExpressionKind::CharLiteral(_)
            | ExpressionKind::StringLiteral(_)
            | ExpressionKind::EndKeyword => {}
            ExpressionKind::MatrixLiteral(rows) | ExpressionKind::CellLiteral(rows) => {
                for row in rows {
                    for expression in row {
                        self.bind_expression(expression, scope_id);
                    }
                }
            }
            ExpressionKind::FunctionHandle(target) => match target {
                FunctionHandleTarget::Name(name) => {
                    self.resolve_qualified_name(scope_id, name, ReferenceRole::FunctionHandleTarget)
                }
                FunctionHandleTarget::Expression(expression) => {
                    self.bind_expression(expression, scope_id);
                }
            },
            ExpressionKind::Unary { rhs, .. } => self.bind_expression(rhs, scope_id),
            ExpressionKind::Binary { lhs, rhs, .. } => {
                self.bind_expression(lhs, scope_id);
                self.bind_expression(rhs, scope_id);
            }
            ExpressionKind::Range { start, step, end } => {
                self.bind_expression(start, scope_id);
                if let Some(step) = step {
                    self.bind_expression(step, scope_id);
                }
                self.bind_expression(end, scope_id);
            }
            ExpressionKind::ParenApply { target, indices } => {
                self.bind_apply_target(target, scope_id);
                self.bind_index_arguments(indices, scope_id);
            }
            ExpressionKind::CellIndex { target, indices } => {
                self.bind_expression(target, scope_id);
                self.bind_index_arguments(indices, scope_id);
            }
            ExpressionKind::FieldAccess { target, .. } => self.bind_expression(target, scope_id),
            ExpressionKind::AnonymousFunction { params, body } => {
                self.bind_anonymous_function(scope_id, params, body);
            }
        }
    }

    fn bind_anonymous_function(
        &mut self,
        parent_scope: ScopeId,
        params: &[Identifier],
        body: &Expression,
    ) {
        let closure_scope = self.alloc_scope(
            Some(parent_scope),
            ScopeKind::AnonymousFunctionBody,
            WorkspaceId(0),
        );
        let parent_workspace = self.scope_workspace(parent_scope);
        let closure_workspace = self.alloc_workspace(
            Some(parent_workspace),
            WorkspaceKind::AnonymousFunction,
            closure_scope,
            Some("<anonymous>".to_string()),
        );
        self.patch_scope_workspace(closure_scope, closure_workspace);

        for param in params {
            self.declare_symbol(
                closure_scope,
                closure_workspace,
                &param.name,
                SymbolKind::Parameter,
                param.span,
            );
        }

        self.bind_expression(body, closure_scope);
    }

    fn bind_apply_target(&mut self, target: &Expression, scope_id: ScopeId) {
        match &target.kind {
            ExpressionKind::Identifier(identifier) => {
                self.resolve_identifier(scope_id, identifier, ReferenceRole::CallTarget)
            }
            _ => {
                if let Some(identifier) = self.extract_qualified_call_target(scope_id, target) {
                    self.resolve_identifier(scope_id, &identifier, ReferenceRole::CallTarget);
                } else {
                    self.bind_expression(target, scope_id);
                }
            }
        }
    }

    fn bind_index_arguments(&mut self, args: &[IndexArgument], scope_id: ScopeId) {
        for arg in args {
            if let IndexArgument::Expression(expression) = arg {
                self.bind_expression(expression, scope_id);
            }
        }
    }

    fn resolve_identifier(
        &mut self,
        scope_id: ScopeId,
        identifier: &Identifier,
        role: ReferenceRole,
    ) {
        match role {
            ReferenceRole::Value => self.resolve_value_reference(scope_id, identifier),
            ReferenceRole::CallTarget => self.resolve_call_target(scope_id, identifier),
            ReferenceRole::FunctionHandleTarget => {
                self.resolve_function_handle_target(scope_id, identifier)
            }
        }
    }

    fn resolve_qualified_name(
        &mut self,
        scope_id: ScopeId,
        name: &QualifiedName,
        role: ReferenceRole,
    ) {
        if role == ReferenceRole::FunctionHandleTarget && name.segments.len() >= 2 {
            let root = &name.segments[0];
            if let Some(resolved) = self.lookup_resolvable_value_symbol(scope_id, &root.name) {
                let capture_access = if resolved.scope_id != scope_id {
                    self.record_capture(scope_id, &root.name, resolved, CaptureAccess::Read);
                    Some(CaptureAccess::Read)
                } else {
                    None
                };
                let identifier = qualified_name_identifier(name);
                self.push_reference(
                    &identifier,
                    ReferenceRole::FunctionHandleTarget,
                    ReferenceResolution::WorkspaceValue,
                    Some(resolved),
                    capture_access,
                );
                return;
            }
        }
        let identifier = qualified_name_identifier(name);
        self.resolve_identifier(scope_id, &identifier, role);
    }

    fn resolve_value_reference(&mut self, scope_id: ScopeId, identifier: &Identifier) {
        if let Some(resolved) = self.lookup_resolvable_value_symbol(scope_id, &identifier.name) {
            let capture_access = if resolved.scope_id != scope_id {
                self.record_capture(scope_id, &identifier.name, resolved, CaptureAccess::Read);
                Some(CaptureAccess::Read)
            } else {
                None
            };

            self.push_reference(
                identifier,
                ReferenceRole::Value,
                ReferenceResolution::WorkspaceValue,
                Some(resolved),
                capture_access,
            );
            return;
        }

        if is_builtin_value(&identifier.name) {
            self.push_reference(
                identifier,
                ReferenceRole::Value,
                ReferenceResolution::BuiltinValue,
                None,
                None,
            );
            return;
        }

        self.push_reference(
            identifier,
            ReferenceRole::Value,
            ReferenceResolution::UnresolvedValue,
            None,
            None,
        );
        if self.property_default_depth == 0 {
            self.diagnostics.push(SemanticDiagnostic::error(
                "SEM002",
                format!("unbound identifier `{}`", identifier.name),
                identifier.span,
            ));
        }
    }

    fn resolve_call_target(&mut self, scope_id: ScopeId, identifier: &Identifier) {
        if let Some(resolved) = self.lookup_resolvable_value_symbol(scope_id, &identifier.name) {
            let capture_access = if resolved.scope_id != scope_id {
                self.record_capture(scope_id, &identifier.name, resolved, CaptureAccess::Read);
                Some(CaptureAccess::Read)
            } else {
                None
            };

            self.push_reference(
                identifier,
                ReferenceRole::CallTarget,
                ReferenceResolution::WorkspaceValue,
                Some(resolved),
                capture_access,
            );
            return;
        }

        if let Some(resolved) = self.lookup_function_symbol(scope_id, &identifier.name) {
            let resolution = self.function_reference_resolution(resolved.scope_id);
            self.push_reference(
                identifier,
                ReferenceRole::CallTarget,
                resolution,
                Some(resolved),
                None,
            );
            return;
        }

        let resolution = if is_builtin_function_name(&identifier.name) {
            ReferenceResolution::BuiltinFunction
        } else {
            ReferenceResolution::ExternalFunctionCandidate
        };
        self.push_reference(
            identifier,
            ReferenceRole::CallTarget,
            resolution,
            None,
            None,
        );
    }

    fn resolve_function_handle_target(&mut self, scope_id: ScopeId, identifier: &Identifier) {
        if let Some(resolved) = self.lookup_function_symbol(scope_id, &identifier.name) {
            let resolution = self.function_reference_resolution(resolved.scope_id);
            self.push_reference(
                identifier,
                ReferenceRole::FunctionHandleTarget,
                resolution,
                Some(resolved),
                None,
            );
            return;
        }

        let resolution = if is_builtin_function_name(&identifier.name) {
            ReferenceResolution::BuiltinFunction
        } else {
            ReferenceResolution::ExternalFunctionCandidate
        };
        self.push_reference(
            identifier,
            ReferenceRole::FunctionHandleTarget,
            resolution,
            None,
            None,
        );
    }

    fn extract_qualified_call_target(
        &self,
        scope_id: ScopeId,
        expression: &Expression,
    ) -> Option<Identifier> {
        let qualified = self.expression_as_qualified_name(expression)?;
        if qualified.segments.len() < 2 {
            return None;
        }

        let root = &qualified.segments[0].name;
        if self
            .lookup_resolvable_value_symbol(scope_id, root)
            .is_some()
        {
            return None;
        }

        Some(qualified_name_identifier(&qualified))
    }

    fn expression_as_qualified_name(&self, expression: &Expression) -> Option<QualifiedName> {
        match &expression.kind {
            ExpressionKind::Identifier(identifier) => Some(QualifiedName {
                segments: vec![identifier.clone()],
                span: identifier.span,
            }),
            ExpressionKind::FieldAccess { target, field } => {
                let mut qualified = self.expression_as_qualified_name(target)?;
                qualified.segments.push(field.clone());
                qualified.span = expression.span;
                Some(qualified)
            }
            _ => None,
        }
    }

    fn push_reference(
        &mut self,
        identifier: &Identifier,
        role: ReferenceRole,
        resolution: ReferenceResolution,
        resolved: Option<ResolvedSymbol>,
        capture_access: Option<CaptureAccess>,
    ) {
        self.references.push(SymbolReference {
            name: identifier.name.clone(),
            span: identifier.span,
            role,
            resolution,
            resolved_symbol: resolved.map(|symbol| symbol.id),
            resolved_kind: resolved.map(|symbol| symbol.kind),
            resolved_scope: resolved.map(|symbol| symbol.scope_id),
            resolved_workspace: resolved.map(|symbol| symbol.workspace_id),
            binding_id: resolved.and_then(|symbol| symbol.binding_id),
            capture_access,
        });
    }

    fn lookup_current_value_symbol(&self, scope_id: ScopeId, name: &str) -> Option<ResolvedSymbol> {
        self.scope_symbols
            .get(&scope_id)
            .and_then(|table| table.values.get(name))
            .copied()
            .map(|symbol_id| self.resolved_symbol(symbol_id))
            .filter(|resolved| self.symbol_kind_can_resolve_as_value(resolved.kind))
    }

    fn lookup_capture_value_symbol_in_ancestors(
        &self,
        scope_id: ScopeId,
        name: &str,
    ) -> Option<ResolvedSymbol> {
        let mut current = self.scope_parent(scope_id);
        while let Some(scope_id) = current {
            if let Some(symbol_id) = self
                .scope_symbols
                .get(&scope_id)
                .and_then(|table| table.values.get(name))
                .copied()
            {
                let resolved = self.resolved_symbol(symbol_id);
                if resolved.kind.is_capture_eligible() {
                    return Some(resolved);
                }
            }
            current = self.scope_parent(scope_id);
        }
        None
    }

    fn lookup_resolvable_value_symbol(
        &self,
        scope_id: ScopeId,
        name: &str,
    ) -> Option<ResolvedSymbol> {
        self.lookup_current_value_symbol(scope_id, name)
            .or_else(|| self.lookup_capture_value_symbol_in_ancestors(scope_id, name))
    }

    fn lookup_function_symbol(&self, scope_id: ScopeId, name: &str) -> Option<ResolvedSymbol> {
        let mut current = Some(scope_id);
        while let Some(scope_id) = current {
            if let Some(symbol_id) = self
                .scope_symbols
                .get(&scope_id)
                .and_then(|table| table.functions.get(name))
                .copied()
            {
                return Some(self.resolved_symbol(symbol_id));
            }
            current = self.scope_parent(scope_id);
        }
        None
    }

    fn resolved_symbol(&self, symbol_id: SymbolId) -> ResolvedSymbol {
        let symbol = self.symbol(symbol_id);
        ResolvedSymbol {
            id: symbol.id,
            kind: symbol.kind,
            scope_id: symbol.scope_id,
            workspace_id: symbol.workspace_id,
            binding_id: symbol.binding_id,
        }
    }

    fn function_reference_resolution(&self, declaring_scope: ScopeId) -> ReferenceResolution {
        match self.scope(declaring_scope).kind {
            ScopeKind::CompilationUnit | ScopeKind::ClassBody => ReferenceResolution::FileFunction,
            ScopeKind::FunctionBody | ScopeKind::AnonymousFunctionBody => {
                ReferenceResolution::NestedFunction
            }
        }
    }

    fn declare_symbol(
        &mut self,
        scope_id: ScopeId,
        workspace_id: WorkspaceId,
        name: &str,
        kind: SymbolKind,
        declared_at: matlab_frontend::source::SourceSpan,
    ) -> SymbolId {
        if let Some(existing) = self
            .scope_symbols
            .get(&scope_id)
            .and_then(|table| {
                if kind.is_function() {
                    table.functions.get(name)
                } else {
                    table.values.get(name)
                }
            })
            .copied()
        {
            return existing;
        }

        let id = SymbolId(self.next_symbol_id);
        self.next_symbol_id += 1;

        let binding_id = kind
            .binding_storage()
            .map(|storage| self.binding_for_symbol(name, storage, id, scope_id, workspace_id));

        let symbol = Symbol {
            id,
            name: name.to_string(),
            kind,
            scope_id,
            workspace_id,
            binding_id,
            declared_at,
        };
        self.symbols.push(symbol);
        let table = self.scope_symbols.entry(scope_id).or_default();
        let namespace = if kind.is_function() {
            &mut table.functions
        } else {
            &mut table.values
        };
        namespace.insert(name.to_string(), id);
        id
    }

    fn alloc_binding(
        &mut self,
        name: &str,
        storage: BindingStorage,
        owner_symbol: SymbolId,
        owner_scope: ScopeId,
        owner_workspace: WorkspaceId,
    ) -> BindingId {
        let id = BindingId(self.next_binding_id);
        self.next_binding_id += 1;
        self.bindings.push(Binding {
            id,
            name: name.to_string(),
            storage,
            owner_symbol,
            owner_scope,
            owner_workspace,
            shared_with_closures: false,
        });
        id
    }

    fn binding_for_symbol(
        &mut self,
        name: &str,
        storage: BindingStorage,
        owner_symbol: SymbolId,
        owner_scope: ScopeId,
        owner_workspace: WorkspaceId,
    ) -> BindingId {
        match storage {
            BindingStorage::Local => {
                self.alloc_binding(name, storage, owner_symbol, owner_scope, owner_workspace)
            }
            BindingStorage::Global => {
                if let Some(binding_id) = self.global_bindings.get(name).copied() {
                    binding_id
                } else {
                    let binding_id = self.alloc_binding(
                        name,
                        storage,
                        owner_symbol,
                        owner_scope,
                        owner_workspace,
                    );
                    self.global_bindings.insert(name.to_string(), binding_id);
                    binding_id
                }
            }
            BindingStorage::Persistent => {
                let key = (
                    self.persistent_owner_workspace(owner_workspace),
                    name.to_string(),
                );
                if let Some(binding_id) = self.persistent_bindings.get(&key).copied() {
                    binding_id
                } else {
                    let binding_id =
                        self.alloc_binding(name, storage, owner_symbol, owner_scope, key.0);
                    self.persistent_bindings.insert(key, binding_id);
                    binding_id
                }
            }
        }
    }

    fn alloc_scope(
        &mut self,
        parent: Option<ScopeId>,
        kind: ScopeKind,
        workspace_id: WorkspaceId,
    ) -> ScopeId {
        let id = ScopeId(self.next_scope_id);
        self.next_scope_id += 1;
        self.scopes.push(Scope {
            id,
            parent,
            kind,
            workspace_id,
        });
        id
    }

    fn alloc_workspace(
        &mut self,
        parent: Option<WorkspaceId>,
        kind: WorkspaceKind,
        scope_id: ScopeId,
        name: Option<String>,
    ) -> WorkspaceId {
        let id = WorkspaceId(self.next_workspace_id);
        self.next_workspace_id += 1;
        self.workspaces.push(Workspace {
            id,
            parent,
            kind,
            scope_id,
            name,
        });
        id
    }

    fn patch_scope_workspace(&mut self, scope_id: ScopeId, workspace_id: WorkspaceId) {
        self.scope_mut(scope_id).workspace_id = workspace_id;
    }

    fn current_scope_declares_value(&self, scope_id: ScopeId, name: &str) -> bool {
        self.scope_symbols
            .get(&scope_id)
            .and_then(|table| table.values.get(name))
            .copied()
            .map(|symbol_id| self.symbol_kind_can_resolve_as_value(self.symbol(symbol_id).kind))
            .unwrap_or(false)
    }

    fn symbol_kind_can_resolve_as_value(&self, kind: SymbolKind) -> bool {
        matches!(
            kind,
            SymbolKind::Variable
                | SymbolKind::Parameter
                | SymbolKind::Output
                | SymbolKind::Global
                | SymbolKind::Persistent
        ) || (self.property_default_depth > 0 && matches!(kind, SymbolKind::Property))
    }

    fn scope_workspace(&self, scope_id: ScopeId) -> WorkspaceId {
        self.scope(scope_id).workspace_id
    }

    fn scope_parent(&self, scope_id: ScopeId) -> Option<ScopeId> {
        self.scope(scope_id).parent
    }

    fn record_capture(
        &mut self,
        into_scope: ScopeId,
        name: &str,
        resolved: ResolvedSymbol,
        access: CaptureAccess,
    ) {
        let Some(binding_id) = resolved.binding_id else {
            return;
        };

        let into_workspace = self.scope_workspace(into_scope);
        if let Some(index) = self.capture_index.get(&(into_scope, resolved.id)).copied() {
            let capture = &mut self.captures[index];
            capture.access = capture.access.merge(access);
        } else {
            let index = self.captures.len();
            self.capture_index.insert((into_scope, resolved.id), index);
            self.captures.push(Capture {
                name: name.to_string(),
                binding_id,
                access,
                from_symbol: resolved.id,
                from_scope: resolved.scope_id,
                from_workspace: resolved.workspace_id,
                into_scope,
                into_workspace,
            });
        }

        self.binding_mut(binding_id).shared_with_closures = true;
    }

    fn symbol(&self, symbol_id: SymbolId) -> &Symbol {
        &self.symbols[symbol_id.0 as usize]
    }

    fn binding_mut(&mut self, binding_id: BindingId) -> &mut Binding {
        &mut self.bindings[binding_id.0 as usize]
    }

    fn scope(&self, scope_id: ScopeId) -> &Scope {
        &self.scopes[scope_id.0 as usize]
    }

    fn scope_mut(&mut self, scope_id: ScopeId) -> &mut Scope {
        &mut self.scopes[scope_id.0 as usize]
    }

    fn workspace(&self, scope_id: ScopeId) -> &Workspace {
        let workspace_id = self.scope(scope_id).workspace_id;
        &self.workspaces[workspace_id.0 as usize]
    }

    fn persistent_owner_workspace(&self, workspace_id: WorkspaceId) -> WorkspaceId {
        let mut current = workspace_id;
        loop {
            let workspace = &self.workspaces[current.0 as usize];
            match workspace.kind {
                WorkspaceKind::Function => return current,
                WorkspaceKind::AnonymousFunction | WorkspaceKind::Script | WorkspaceKind::Class => {
                    if let Some(parent) = workspace.parent {
                        current = parent;
                    } else {
                        return workspace_id;
                    }
                }
            }
        }
    }
}

pub fn is_builtin_function_name(name: &str) -> bool {
    BUILTIN_FUNCTIONS.iter().any(|builtin| *builtin == name)
}

pub fn builtin_function_names() -> &'static [&'static str] {
    BUILTIN_FUNCTIONS
}

fn is_builtin_value(name: &str) -> bool {
    matches!(
        name,
        "true"
            | "false"
            | "i"
            | "j"
            | "pi"
            | "eps"
            | "realmin"
            | "realmax"
            | "flintmax"
            | "inf"
            | "Inf"
            | "nan"
            | "NaN"
    )
}

fn literal_text_value(expression: &Expression) -> Option<String> {
    match &expression.kind {
        ExpressionKind::CharLiteral(text) => decode_text_literal(text, '\''),
        ExpressionKind::StringLiteral(text) => decode_text_literal(text, '"'),
        _ => None,
    }
}

fn decode_text_literal(lexeme: &str, delimiter: char) -> Option<String> {
    let inner = lexeme
        .strip_prefix(delimiter)
        .and_then(|text| text.strip_suffix(delimiter))?;
    let mut out = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == delimiter && chars.peek() == Some(&delimiter) {
            out.push(delimiter);
            chars.next();
        } else {
            out.push(ch);
        }
    }
    Some(out)
}

fn has_wildcard_pattern(pattern: &str) -> bool {
    pattern.contains('*') || pattern.contains('?')
}

fn matlab_wildcard_is_match(pattern: &str, text: &str) -> bool {
    wildcard_match_here(
        &pattern.chars().collect::<Vec<_>>(),
        &text.chars().collect::<Vec<_>>(),
    )
}

fn wildcard_match_here(pattern: &[char], text: &[char]) -> bool {
    if pattern.is_empty() {
        return text.is_empty();
    }
    match pattern[0] {
        '*' => {
            for offset in 0..=text.len() {
                if wildcard_match_here(&pattern[1..], &text[offset..]) {
                    return true;
                }
            }
            false
        }
        '?' => {
            if text.is_empty() {
                false
            } else {
                wildcard_match_here(&pattern[1..], &text[1..])
            }
        }
        ch => {
            if text.first().copied() != Some(ch) {
                false
            } else {
                wildcard_match_here(&pattern[1..], &text[1..])
            }
        }
    }
}

fn matlab_regexp_is_match(pattern: &str, text: &str) -> bool {
    if pattern.is_empty() {
        return true;
    }
    let anchored_start = pattern.starts_with('^');
    let anchored_end = pattern.ends_with('$') && !pattern.ends_with("\\$");
    let core = pattern
        .strip_prefix('^')
        .unwrap_or(pattern)
        .strip_suffix('$')
        .unwrap_or(pattern.strip_prefix('^').unwrap_or(pattern));
    if anchored_start {
        return simple_regexp_match_here(core, text, anchored_end);
    }
    for start in text
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(text.len()))
    {
        if simple_regexp_match_here(core, &text[start..], anchored_end) {
            return true;
        }
    }
    false
}

fn simple_regexp_match_here(pattern: &str, text: &str, anchored_end: bool) -> bool {
    if pattern.is_empty() {
        return !anchored_end || text.is_empty();
    }
    let mut chars = pattern.chars();
    let ch = chars.next().unwrap_or_default();
    let rest = chars.as_str();
    if let Some(star_rest) = rest.strip_prefix('*') {
        return simple_regexp_match_star(ch, star_rest, text, anchored_end);
    }
    let Some(first) = text.chars().next() else {
        return false;
    };
    if ch != '.' && ch != first {
        return false;
    }
    simple_regexp_match_here(rest, &text[first.len_utf8()..], anchored_end)
}

fn simple_regexp_match_star(pattern_ch: char, rest: &str, text: &str, anchored_end: bool) -> bool {
    let mut cursor = text;
    loop {
        if simple_regexp_match_here(rest, cursor, anchored_end) {
            return true;
        }
        let Some(first) = cursor.chars().next() else {
            return false;
        };
        if pattern_ch != '.' && pattern_ch != first {
            return false;
        }
        cursor = &cursor[first.len_utf8()..];
    }
}

fn qualified_name_identifier(name: &QualifiedName) -> Identifier {
    Identifier {
        name: name
            .segments
            .iter()
            .map(|segment| segment.name.as_str())
            .collect::<Vec<_>>()
            .join("."),
        span: name.span,
    }
}

#[cfg(test)]
mod tests {
    use matlab_frontend::{
        parser::{parse_source, ParseMode},
        source::SourceFileId,
    };

    use super::analyze_compilation_unit;
    use crate::{
        symbols::{BindingStorage, CaptureAccess, ReferenceResolution, ReferenceRole, SymbolKind},
        workspace::{ScopeKind, WorkspaceKind},
    };

    #[test]
    fn binds_script_assignments_into_script_workspace() {
        let parsed = parse_source("x = 1;\ny = x + 2;\n", SourceFileId(1), ParseMode::Script);
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit(&unit);

        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        assert_eq!(analysis.workspaces.len(), 1);
        assert_eq!(analysis.workspaces[0].kind, WorkspaceKind::Script);
        assert!(analysis
            .symbols
            .iter()
            .any(|symbol| symbol.name == "x" && symbol.kind == SymbolKind::Variable));
        assert!(analysis
            .symbols
            .iter()
            .any(|symbol| symbol.name == "y" && symbol.kind == SymbolKind::Variable));
        assert!(analysis
            .symbols
            .iter()
            .any(|symbol| symbol.name == "ans" && symbol.kind == SymbolKind::Variable));
        assert_eq!(analysis.bindings.len(), 3);
    }

    #[test]
    fn binds_function_parameters_outputs_and_bindings() {
        let parsed = parse_source(
            "function y = add1(x)\ny = x + 1;\nend\n",
            SourceFileId(1),
            ParseMode::AutoDetect,
        );
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit(&unit);

        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        assert!(analysis
            .symbols
            .iter()
            .any(|symbol| symbol.name == "add1" && symbol.kind == SymbolKind::Function));
        assert!(analysis
            .symbols
            .iter()
            .any(|symbol| symbol.name == "x" && symbol.kind == SymbolKind::Parameter));
        assert!(analysis
            .symbols
            .iter()
            .any(|symbol| symbol.name == "y" && symbol.kind == SymbolKind::Output));
        assert!(analysis
            .scopes
            .iter()
            .any(|scope| scope.kind == ScopeKind::FunctionBody));
        assert!(analysis
            .bindings
            .iter()
            .any(|binding| binding.name == "x" && binding.storage == BindingStorage::Local));
    }

    #[test]
    fn reports_unbound_identifier_uses() {
        let parsed = parse_source("y = x + 1;\n", SourceFileId(1), ParseMode::Script);
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit(&unit);

        assert!(analysis.has_errors());
        assert!(analysis
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "SEM002"));
    }

    #[test]
    fn treats_builtin_call_targets_as_builtin_functions() {
        let parsed = parse_source("y = zeros(1, 2);\n", SourceFileId(1), ParseMode::Script);
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit(&unit);

        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        assert!(analysis.references.iter().any(|reference| {
            reference.name == "zeros"
                && reference.role == ReferenceRole::CallTarget
                && reference.resolution == ReferenceResolution::BuiltinFunction
        }));
    }

    #[test]
    fn treats_disp_call_targets_as_builtin_functions() {
        let parsed = parse_source("disp(\"hello\");\n", SourceFileId(1), ParseMode::Script);
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit(&unit);

        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        assert!(analysis.references.iter().any(|reference| {
            reference.name == "disp"
                && reference.role == ReferenceRole::CallTarget
                && reference.resolution == ReferenceResolution::BuiltinFunction
        }));
    }

    #[test]
    fn treats_format_call_targets_as_builtin_functions() {
        let parsed = parse_source("format short;\n", SourceFileId(1), ParseMode::Script);
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit(&unit);

        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        assert!(analysis.references.iter().any(|reference| {
            reference.name == "format"
                && reference.role == ReferenceRole::CallTarget
                && reference.resolution == ReferenceResolution::BuiltinFunction
        }));
    }

    #[test]
    fn treats_builtin_value_references_as_builtin_values() {
        let parsed = parse_source(
            "a = true;\nb = false;\nc = i;\nd = j;\ne = pi;\nf = eps;\ng = Inf;\nh = NaN;\n",
            SourceFileId(1),
            ParseMode::Script,
        );
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit(&unit);

        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        assert!(analysis.references.iter().any(|reference| {
            reference.name == "true"
                && reference.role == ReferenceRole::Value
                && reference.resolution == ReferenceResolution::BuiltinValue
        }));
        assert!(analysis.references.iter().any(|reference| {
            reference.name == "false"
                && reference.role == ReferenceRole::Value
                && reference.resolution == ReferenceResolution::BuiltinValue
        }));
        assert!(analysis.references.iter().any(|reference| {
            reference.name == "i"
                && reference.role == ReferenceRole::Value
                && reference.resolution == ReferenceResolution::BuiltinValue
        }));
        assert!(analysis.references.iter().any(|reference| {
            reference.name == "j"
                && reference.role == ReferenceRole::Value
                && reference.resolution == ReferenceResolution::BuiltinValue
        }));
        assert!(analysis.references.iter().any(|reference| {
            reference.name == "pi"
                && reference.role == ReferenceRole::Value
                && reference.resolution == ReferenceResolution::BuiltinValue
        }));
        assert!(analysis.references.iter().any(|reference| {
            reference.name == "eps"
                && reference.role == ReferenceRole::Value
                && reference.resolution == ReferenceResolution::BuiltinValue
        }));
        assert!(analysis.references.iter().any(|reference| {
            reference.name == "Inf"
                && reference.role == ReferenceRole::Value
                && reference.resolution == ReferenceResolution::BuiltinValue
        }));
        assert!(analysis.references.iter().any(|reference| {
            reference.name == "NaN"
                && reference.role == ReferenceRole::Value
                && reference.resolution == ReferenceResolution::BuiltinValue
        }));
    }

    #[test]
    fn local_i_and_j_shadow_builtin_value_resolution() {
        let parsed = parse_source(
            "i = 7;\nj = 8;\na = i;\nb = j;\n",
            SourceFileId(1),
            ParseMode::Script,
        );
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit(&unit);

        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);

        let i_value_refs = analysis
            .references
            .iter()
            .filter(|reference| reference.name == "i" && reference.role == ReferenceRole::Value)
            .collect::<Vec<_>>();
        let j_value_refs = analysis
            .references
            .iter()
            .filter(|reference| reference.name == "j" && reference.role == ReferenceRole::Value)
            .collect::<Vec<_>>();

        assert!(i_value_refs
            .iter()
            .all(|reference| reference.resolution == ReferenceResolution::WorkspaceValue));
        assert!(j_value_refs
            .iter()
            .all(|reference| reference.resolution == ReferenceResolution::WorkspaceValue));
    }

    #[test]
    fn treats_unknown_call_targets_as_external_function_candidates() {
        let parsed = parse_source("y = userfunc(x);\n", SourceFileId(1), ParseMode::Script);
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit(&unit);

        assert!(analysis.references.iter().any(|reference| {
            reference.name == "userfunc"
                && reference.role == ReferenceRole::CallTarget
                && reference.resolution == ReferenceResolution::ExternalFunctionCandidate
        }));
        assert!(analysis.has_errors());
        assert!(analysis.references.iter().any(|reference| {
            reference.name == "x"
                && reference.role == ReferenceRole::Value
                && reference.resolution == ReferenceResolution::UnresolvedValue
        }));
    }

    #[test]
    fn variable_shadowing_wins_over_builtin_calls_but_not_function_handles() {
        let parsed = parse_source(
            "function y = outer(x)\nsum = x;\na = sum(1);\nb = @sum;\ny = a;\nend\n",
            SourceFileId(1),
            ParseMode::AutoDetect,
        );
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit(&unit);

        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        assert!(analysis.references.iter().any(|reference| {
            reference.name == "sum"
                && reference.role == ReferenceRole::CallTarget
                && reference.resolution == ReferenceResolution::WorkspaceValue
        }));
        assert!(analysis.references.iter().any(|reference| {
            reference.name == "sum"
                && reference.role == ReferenceRole::FunctionHandleTarget
                && reference.resolution == ReferenceResolution::BuiltinFunction
        }));
    }

    #[test]
    fn dotted_function_handles_with_workspace_roots_stay_in_workspace_value_space() {
        let parsed = parse_source(
            "function y = outer(objs)\nf = @objs.child.total;\ny = f;\nend\n",
            SourceFileId(1),
            ParseMode::AutoDetect,
        );
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit(&unit);

        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        assert!(analysis.references.iter().any(|reference| {
            reference.name == "objs.child.total"
                && reference.role == ReferenceRole::FunctionHandleTarget
                && reference.resolution == ReferenceResolution::WorkspaceValue
        }));
    }

    #[test]
    fn bare_expression_statements_can_resolve_zero_arg_builtin_calls() {
        let parsed = parse_source("figure;\n", SourceFileId(1), ParseMode::Script);
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit(&unit);

        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        assert!(analysis.references.iter().any(|reference| {
            reference.name == "figure"
                && reference.role == ReferenceRole::CallTarget
                && reference.resolution == ReferenceResolution::BuiltinFunction
        }));
    }

    #[test]
    fn assignment_rhs_can_resolve_zero_arg_builtin_calls() {
        let parsed = parse_source(
            "token = tic;\nnames = who;\nsummary = whos;\n",
            SourceFileId(1),
            ParseMode::Script,
        );
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit(&unit);

        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        for builtin_name in ["tic", "who", "whos"] {
            assert!(analysis.references.iter().any(|reference| {
                reference.name == builtin_name
                    && reference.role == ReferenceRole::CallTarget
                    && reference.resolution == ReferenceResolution::BuiltinFunction
            }));
        }
    }

    #[test]
    fn bare_value_references_do_not_resolve_visible_function_symbols() {
        let parsed = parse_source(
            "function y = outer()\ny = helper;\nfunction z = helper()\nz = 1;\nend\nend\n",
            SourceFileId(1),
            ParseMode::AutoDetect,
        );
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit(&unit);

        assert!(analysis.has_errors());
        assert!(analysis.references.iter().any(|reference| {
            reference.name == "helper"
                && reference.role == ReferenceRole::Value
                && reference.resolution == ReferenceResolution::UnresolvedValue
        }));
    }

    #[test]
    fn anonymous_functions_capture_outer_bindings() {
        let parsed = parse_source(
            "y = 2;\nf = @(x) x + y;\n",
            SourceFileId(1),
            ParseMode::Script,
        );
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit(&unit);

        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        assert!(analysis
            .captures
            .iter()
            .any(|capture| capture.name == "y" && capture.access == CaptureAccess::Read));
        assert!(analysis
            .bindings
            .iter()
            .any(|binding| binding.name == "y" && binding.shared_with_closures));
    }

    #[test]
    fn local_functions_are_predeclared_and_classified_as_nested_functions() {
        let parsed = parse_source(
            "function y = outer(x)\nhelper = @(z) z + x;\nfunction z = inner(v)\nz = v + x;\nend\ny = inner(1);\nend\n",
            SourceFileId(1),
            ParseMode::AutoDetect,
        );
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit(&unit);

        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        assert!(analysis
            .symbols
            .iter()
            .any(|symbol| symbol.name == "inner" && symbol.kind == SymbolKind::Function));
        assert!(analysis.references.iter().any(|reference| {
            reference.name == "inner"
                && reference.role == ReferenceRole::CallTarget
                && reference.resolution == ReferenceResolution::NestedFunction
        }));
        assert!(analysis
            .captures
            .iter()
            .any(|capture| capture.name == "x" && capture.access == CaptureAccess::Read));
    }

    #[test]
    fn nested_function_assignment_reuses_outer_binding_as_write_capture() {
        let parsed = parse_source(
            "function y = outer()\nx = 1;\nfunction inner()\nx = 2;\nend\ninner();\ny = x;\nend\n",
            SourceFileId(1),
            ParseMode::AutoDetect,
        );
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit(&unit);

        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        let x_variables = analysis
            .symbols
            .iter()
            .filter(|symbol| symbol.name == "x" && symbol.kind == SymbolKind::Variable)
            .count();
        assert_eq!(
            x_variables, 1,
            "nested assignment should not create a second local `x`"
        );
        assert!(analysis
            .captures
            .iter()
            .any(|capture| capture.name == "x" && capture.access == CaptureAccess::Write));
    }

    #[test]
    fn nested_read_then_write_upgrades_capture_to_read_write() {
        let parsed = parse_source(
            "function y = outer()\nx = 1;\nfunction inner(step)\nx = x + step;\nend\ninner(1);\ny = x;\nend\n",
            SourceFileId(1),
            ParseMode::AutoDetect,
        );
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit(&unit);

        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        assert!(analysis
            .captures
            .iter()
            .any(|capture| capture.name == "x" && capture.access == CaptureAccess::ReadWrite));
    }

    #[test]
    fn nested_function_new_assignment_stays_local_when_no_outer_binding_exists() {
        let parsed = parse_source(
            "function y = outer()\nfunction inner()\nz = 2;\nend\ninner();\ny = 1;\nend\n",
            SourceFileId(1),
            ParseMode::AutoDetect,
        );
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit(&unit);

        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        let z_variables = analysis
            .symbols
            .iter()
            .filter(|symbol| symbol.name == "z" && symbol.kind == SymbolKind::Variable)
            .count();
        assert_eq!(z_variables, 1);
        assert!(!analysis.captures.iter().any(|capture| capture.name == "z"));
    }

    #[test]
    fn global_declarations_share_binding_identity_across_functions() {
        let parsed = parse_source(
            "function y = outer()\nglobal g;\ng = 1;\nfunction inner()\nglobal g;\ng = 2;\nend\ninner();\ny = g;\nend\n",
            SourceFileId(1),
            ParseMode::AutoDetect,
        );
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit(&unit);

        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        let global_symbols = analysis
            .symbols
            .iter()
            .filter(|symbol| symbol.name == "g" && symbol.kind == SymbolKind::Global)
            .collect::<Vec<_>>();
        assert_eq!(global_symbols.len(), 2);
        assert_eq!(global_symbols[0].binding_id, global_symbols[1].binding_id);
        let global_bindings = analysis
            .bindings
            .iter()
            .filter(|binding| binding.name == "g" && binding.storage == BindingStorage::Global)
            .count();
        assert_eq!(global_bindings, 1);
        assert!(!analysis.captures.iter().any(|capture| capture.name == "g"));
    }

    #[test]
    fn outer_global_is_not_implicitly_captured_by_nested_function() {
        let parsed = parse_source(
            "function y = outer()\nglobal g;\ng = 1;\nfunction inner()\ny = g;\nend\ninner();\ny = 1;\nend\n",
            SourceFileId(1),
            ParseMode::AutoDetect,
        );
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit(&unit);

        assert!(analysis.has_errors());
        assert!(analysis.references.iter().any(|reference| {
            reference.name == "g"
                && reference.role == ReferenceRole::Value
                && reference.resolution == ReferenceResolution::UnresolvedValue
        }));
        assert!(!analysis.captures.iter().any(|capture| capture.name == "g"));
    }

    #[test]
    fn persistent_binding_is_capture_eligible_for_nested_functions() {
        let parsed = parse_source(
            "function y = outer(step)\npersistent p;\np = step;\nfunction inner(delta)\np = p + delta;\nend\ninner(1);\ny = p;\nend\n",
            SourceFileId(1),
            ParseMode::AutoDetect,
        );
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit(&unit);

        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        let persistent_symbol = analysis
            .symbols
            .iter()
            .find(|symbol| symbol.name == "p" && symbol.kind == SymbolKind::Persistent)
            .expect("persistent symbol");
        assert!(analysis.captures.iter().any(|capture| {
            capture.name == "p"
                && capture.binding_id == persistent_symbol.binding_id.expect("persistent binding")
                && capture.access == CaptureAccess::ReadWrite
        }));
    }

    #[test]
    fn persistent_in_script_reports_semantic_error() {
        let parsed = parse_source(
            "persistent cache;\ncache = 1;\n",
            SourceFileId(1),
            ParseMode::Script,
        );
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit(&unit);

        assert!(analysis.has_errors());
        assert!(analysis
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "SEM003"));
    }

    #[test]
    fn separate_value_and_function_namespaces_allow_shadowing_without_losing_function_handles() {
        let parsed = parse_source(
            "function y = outer(x)\nhelper = x;\na = helper(1);\nb = @helper;\nfunction z = helper(v)\nz = v + 1;\nend\ny = a;\nend\n",
            SourceFileId(1),
            ParseMode::AutoDetect,
        );
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit(&unit);

        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        assert_eq!(
            analysis
                .symbols
                .iter()
                .filter(|symbol| symbol.name == "helper")
                .count(),
            2
        );
        assert!(analysis.references.iter().any(|reference| {
            reference.name == "helper"
                && reference.role == ReferenceRole::CallTarget
                && reference.resolution == ReferenceResolution::WorkspaceValue
        }));
        assert!(analysis.references.iter().any(|reference| {
            reference.name == "helper"
                && reference.role == ReferenceRole::FunctionHandleTarget
                && reference.resolution == ReferenceResolution::NestedFunction
        }));
    }

    #[test]
    fn package_qualified_call_targets_do_not_report_unbound_package_roots() {
        let parsed = parse_source(
            "function y = outer(x)\ny = pkg.helper(x);\nend\n",
            SourceFileId(1),
            ParseMode::AutoDetect,
        );
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit(&unit);

        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        assert!(analysis.references.iter().any(|reference| {
            reference.name == "pkg.helper"
                && reference.role == ReferenceRole::CallTarget
                && reference.resolution == ReferenceResolution::ExternalFunctionCandidate
        }));
        assert!(!analysis.references.iter().any(|reference| {
            reference.name == "pkg"
                && reference.role == ReferenceRole::Value
                && reference.resolution == ReferenceResolution::UnresolvedValue
        }));
    }

    #[test]
    fn package_qualified_function_handles_are_external_candidates() {
        let parsed = parse_source(
            "function y = outer()\nf = @pkg.helper;\ny = 1;\nend\n",
            SourceFileId(1),
            ParseMode::AutoDetect,
        );
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit(&unit);

        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        assert!(analysis.references.iter().any(|reference| {
            reference.name == "pkg.helper"
                && reference.role == ReferenceRole::FunctionHandleTarget
                && reference.resolution == ReferenceResolution::ExternalFunctionCandidate
        }));
    }

    #[test]
    fn workspace_values_keep_dotted_apply_targets_in_value_space() {
        let parsed = parse_source(
            "function y = outer(obj)\ny = obj.helper(1);\nend\n",
            SourceFileId(1),
            ParseMode::AutoDetect,
        );
        let unit = parsed.unit.expect("parsed unit");
        let analysis = analyze_compilation_unit(&unit);

        assert!(!analysis.has_errors(), "{:?}", analysis.diagnostics);
        assert!(analysis.references.iter().any(|reference| {
            reference.name == "obj"
                && reference.role == ReferenceRole::Value
                && reference.resolution == ReferenceResolution::WorkspaceValue
        }));
        assert!(!analysis.references.iter().any(|reference| {
            reference.name == "obj.helper" && reference.role == ReferenceRole::CallTarget
        }));
    }
}
