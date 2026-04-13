use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};

use crate::RenderedFigure;
use flate2::{write::ZlibEncoder, Compression};
use matlab_runtime::{CellValue, MatrixValue, RuntimeError, StructValue, Value};
use matlab_stdlib::invoke_builtin_outputs as invoke_stdlib_builtin_outputs;
use pdf_writer::{Content, Filter, Finish, Name, Pdf, Rect, Ref};
use resvg::{tiny_skia, usvg};

const DEFAULT_RENDER_WIDTH: f64 = 900.0;
const DEFAULT_RENDER_HEIGHT: f64 = 650.0;
const EXPORT_BASE_DPI: u32 = 96;
const PDF_MARGIN_INCHES: f64 = 0.25;

#[derive(Debug, Clone, Default)]
pub(crate) struct GraphicsState {
    figures: BTreeMap<u32, FigureState>,
    current_figure: Option<u32>,
    next_auto_figure_handle: u32,
    next_axes_handle: u32,
    next_series_handle: u32,
    next_annotation_handle: u32,
}

#[derive(Debug, Clone)]
struct FigureState {
    name: String,
    number_title: bool,
    visible: bool,
    position: [f64; 4],
    window_style: FigureWindowStyle,
    close_request_fcn: Option<Value>,
    resize_fcn: Option<Value>,
    layout_rows: usize,
    layout_cols: usize,
    current_axes: usize,
    current_object: Option<u32>,
    rotate3d_enabled: bool,
    super_title: String,
    axes: BTreeMap<usize, AxesSlot>,
    linked_axes: Vec<LinkedAxesGroup>,
    annotations: Vec<AnnotationObject>,
    paper_units: PaperUnits,
    paper_type: PaperType,
    paper_size_in: [f64; 2],
    paper_position_in: [f64; 4],
    paper_position_mode: PaperPositionMode,
    paper_orientation: PaperOrientation,
}

impl Default for FigureState {
    fn default() -> Self {
        Self {
            name: String::new(),
            number_title: true,
            visible: true,
            position: [80.0, 80.0, 1360.0, 960.0],
            window_style: FigureWindowStyle::Normal,
            close_request_fcn: None,
            resize_fcn: None,
            layout_rows: 1,
            layout_cols: 1,
            current_axes: 1,
            current_object: None,
            rotate3d_enabled: false,
            super_title: String::new(),
            axes: BTreeMap::new(),
            linked_axes: Vec::new(),
            annotations: Vec::new(),
            paper_units: PaperUnits::Inches,
            paper_type: PaperType::UsLetter,
            paper_size_in: standard_paper_size_in(PaperType::UsLetter, PaperOrientation::Portrait),
            paper_position_in: default_auto_paper_position_in(standard_paper_size_in(
                PaperType::UsLetter,
                PaperOrientation::Portrait,
            )),
            paper_position_mode: PaperPositionMode::Auto,
            paper_orientation: PaperOrientation::Portrait,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FigureWindowStyle {
    Normal,
    Docked,
}

impl FigureWindowStyle {
    fn as_text(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Docked => "docked",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaperUnits {
    Inches,
    Centimeters,
    Points,
    Normalized,
}

impl PaperUnits {
    fn as_text(self) -> &'static str {
        match self {
            Self::Inches => "inches",
            Self::Centimeters => "centimeters",
            Self::Points => "points",
            Self::Normalized => "normalized",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaperType {
    UsLetter,
    UsLegal,
    Tabloid,
    A3,
    A4,
    Custom,
}

impl PaperType {
    fn as_text(self) -> &'static str {
        match self {
            Self::UsLetter => "usletter",
            Self::UsLegal => "uslegal",
            Self::Tabloid => "tabloid",
            Self::A3 => "a3",
            Self::A4 => "a4",
            Self::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaperPositionMode {
    Auto,
    Manual,
}

impl PaperPositionMode {
    fn as_text(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Manual => "manual",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PaperOrientation {
    Portrait,
    Landscape,
}

impl PaperOrientation {
    fn as_text(self) -> &'static str {
        match self {
            Self::Portrait => "portrait",
            Self::Landscape => "landscape",
        }
    }
}

#[derive(Debug, Clone)]
struct AxesSlot {
    handle: u32,
    axes: AxesState,
}

#[derive(Debug, Clone)]
struct AxesState {
    position: Option<[f64; 4]>,
    hold_enabled: bool,
    axis_visible: bool,
    box_enabled: bool,
    grid_enabled: bool,
    shading_mode: ShadingMode,
    view_azimuth: f64,
    view_elevation: f64,
    active_y_axis: YAxisSide,
    title: String,
    subtitle: String,
    xlabel: String,
    ylabel: String,
    ylabel_right: String,
    zlabel: String,
    xticks: Option<Vec<f64>>,
    yticks: Option<Vec<f64>>,
    yticks_right: Option<Vec<f64>>,
    zticks: Option<Vec<f64>>,
    xtick_labels: Option<Vec<String>>,
    ytick_labels: Option<Vec<String>>,
    ytick_labels_right: Option<Vec<String>>,
    ztick_labels: Option<Vec<String>>,
    xtick_angle: f64,
    ytick_angle: f64,
    ytick_angle_right: f64,
    ztick_angle: f64,
    xlim: Option<(f64, f64)>,
    ylim: Option<(f64, f64)>,
    ylim_right: Option<(f64, f64)>,
    zlim: Option<(f64, f64)>,
    x_scale: AxisScale,
    y_scale: AxisScale,
    y_scale_right: AxisScale,
    aspect_mode: AxisAspectMode,
    caxis: Option<(f64, f64)>,
    colormap: ColormapKind,
    colorbar_enabled: bool,
    legend: Option<Vec<String>>,
    legend_location: LegendLocation,
    legend_orientation: LegendOrientation,
    series: Vec<PlotSeries>,
}

impl Default for AxesState {
    fn default() -> Self {
        Self {
            position: None,
            hold_enabled: false,
            axis_visible: true,
            box_enabled: true,
            grid_enabled: false,
            shading_mode: ShadingMode::Faceted,
            view_azimuth: -37.5,
            view_elevation: 30.0,
            active_y_axis: YAxisSide::Left,
            title: String::new(),
            subtitle: String::new(),
            xlabel: String::new(),
            ylabel: String::new(),
            ylabel_right: String::new(),
            zlabel: String::new(),
            xticks: None,
            yticks: None,
            yticks_right: None,
            zticks: None,
            xtick_labels: None,
            ytick_labels: None,
            ytick_labels_right: None,
            ztick_labels: None,
            xtick_angle: 0.0,
            ytick_angle: 0.0,
            ytick_angle_right: 0.0,
            ztick_angle: 0.0,
            xlim: None,
            ylim: None,
            ylim_right: None,
            zlim: None,
            x_scale: AxisScale::Linear,
            y_scale: AxisScale::Linear,
            y_scale_right: AxisScale::Linear,
            aspect_mode: AxisAspectMode::Auto,
            caxis: None,
            colormap: ColormapKind::Parula,
            colorbar_enabled: false,
            legend: None,
            legend_location: LegendLocation::Northeast,
            legend_orientation: LegendOrientation::Vertical,
            series: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
struct PlotSeries {
    handle: u32,
    kind: SeriesKind,
    y_axis_side: YAxisSide,
    x: Vec<f64>,
    y: Vec<f64>,
    color: &'static str,
    marker_edge_color: MarkerColorMode,
    marker_face_color: MarkerColorMode,
    line_width: f64,
    line_style: LineStyle,
    marker: MarkerStyle,
    marker_size: f64,
    maximum_num_points: Option<usize>,
    visible: bool,
    display_name: Option<String>,
    scatter: Option<ScatterSeriesData>,
    quiver: Option<QuiverSeriesData>,
    error_bar: Option<ErrorBarSeriesData>,
    histogram: Option<HistogramSeriesData>,
    histogram2: Option<Histogram2SeriesData>,
    pie: Option<PieSeriesData>,
    image: Option<ImageSeriesData>,
    contour: Option<ContourSeriesData>,
    contour_fill: Option<ContourFillSeriesData>,
    surface: Option<SurfaceSeriesData>,
    three_d: Option<ThreeDSeriesData>,
    text: Option<TextSeriesData>,
    reference_line: Option<ReferenceLineData>,
    rectangle: Option<RectangleSeriesData>,
    patch: Option<PatchSeriesData>,
}

fn make_series(handle: u32, kind: SeriesKind, color: &'static str) -> PlotSeries {
    PlotSeries {
        handle,
        kind,
        y_axis_side: YAxisSide::Left,
        x: Vec::new(),
        y: Vec::new(),
        color,
        marker_edge_color: MarkerColorMode::Auto,
        marker_face_color: MarkerColorMode::None,
        line_width: 2.5,
        line_style: LineStyle::Solid,
        marker: MarkerStyle::None,
        marker_size: 5.0,
        maximum_num_points: None,
        visible: true,
        display_name: None,
        scatter: None,
        quiver: None,
        error_bar: None,
        histogram: None,
        histogram2: None,
        pie: None,
        image: None,
        contour: None,
        contour_fill: None,
        surface: None,
        three_d: None,
        text: None,
        reference_line: None,
        rectangle: None,
        patch: None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum YAxisSide {
    Left,
    Right,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LineStyle {
    Solid,
    Dashed,
    Dotted,
    DashDot,
    None,
}

impl LineStyle {
    fn as_text(self) -> &'static str {
        match self {
            Self::Solid => "-",
            Self::Dashed => "--",
            Self::Dotted => ":",
            Self::DashDot => "-.",
            Self::None => "none",
        }
    }

    fn stroke_dasharray(self) -> Option<&'static str> {
        match self {
            Self::Solid => None,
            Self::Dashed => Some("8 5"),
            Self::Dotted => Some("2.5 4"),
            Self::DashDot => Some("8 5 2.5 5"),
            Self::None => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkerStyle {
    None,
    Point,
    Circle,
    XMark,
    Plus,
    Star,
    Square,
    Diamond,
    TriangleDown,
    TriangleUp,
    TriangleLeft,
    TriangleRight,
    Pentagram,
    Hexagram,
}

impl MarkerStyle {
    fn as_text(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Point => ".",
            Self::Circle => "o",
            Self::XMark => "x",
            Self::Plus => "+",
            Self::Star => "*",
            Self::Square => "s",
            Self::Diamond => "d",
            Self::TriangleDown => "v",
            Self::TriangleUp => "^",
            Self::TriangleLeft => "<",
            Self::TriangleRight => ">",
            Self::Pentagram => "p",
            Self::Hexagram => "h",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarkerColorMode {
    Auto,
    Flat,
    None,
    Fixed(&'static str),
}

impl MarkerColorMode {
    fn property_value(self) -> Result<Value, RuntimeError> {
        match self {
            Self::Auto => Ok(Value::CharArray("auto".to_string())),
            Self::Flat => Ok(Value::CharArray("flat".to_string())),
            Self::None => Ok(Value::CharArray("none".to_string())),
            Self::Fixed(color) => color_property_value(color),
        }
    }

    fn resolve<'a>(self, default: &'a str) -> Option<&'a str> {
        match self {
            Self::Auto | Self::Flat => Some(default),
            Self::None => None,
            Self::Fixed(color) => Some(color),
        }
    }
}

#[derive(Debug, Clone)]
struct TextSeriesData {
    x: f64,
    y: f64,
    label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReferenceLineOrientation {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone)]
struct ReferenceLineData {
    orientation: ReferenceLineOrientation,
    value: f64,
    label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnnotationKind {
    Line,
    Arrow,
    DoubleArrow,
    TextArrow,
    TextBox,
    Rectangle,
    Ellipse,
}

impl AnnotationKind {
    fn type_name(self) -> &'static str {
        match self {
            Self::Line => "line",
            Self::Arrow => "arrow",
            Self::DoubleArrow => "doublearrow",
            Self::TextArrow => "textarrow",
            Self::TextBox => "textbox",
            Self::Rectangle => "rectangle",
            Self::Ellipse => "ellipse",
        }
    }
}

#[derive(Debug, Clone)]
struct AnnotationObject {
    handle: u32,
    kind: AnnotationKind,
    x: Vec<f64>,
    y: Vec<f64>,
    position: Option<[f64; 4]>,
    text: String,
    color: &'static str,
    line_width: f64,
    line_style: LineStyle,
    visible: bool,
    face_color: Option<&'static str>,
    font_size: f64,
}

#[derive(Debug, Clone)]
struct RectangleSeriesData {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    face_color: Option<&'static str>,
}

#[derive(Debug, Clone)]
struct PatchSeriesData {
    face_color: Option<&'static str>,
}

#[derive(Debug, Clone)]
struct ScatterSeriesData {
    marker_sizes: Vec<f64>,
    colors: ScatterColors,
    filled: bool,
    uses_default_color: bool,
    marker: Option<MarkerStyle>,
}

#[derive(Debug, Clone)]
enum ScatterColors {
    Uniform(&'static str),
    Colormapped(Vec<f64>),
    Rgb(Vec<[f64; 3]>),
}

#[derive(Debug, Clone)]
struct QuiverSeriesData {
    bases: Vec<(f64, f64)>,
    tips: Vec<(f64, f64)>,
}

#[derive(Debug, Clone)]
struct ErrorBarSeriesData {
    vertical_lower: Option<Vec<f64>>,
    vertical_upper: Option<Vec<f64>>,
    horizontal_lower: Option<Vec<f64>>,
    horizontal_upper: Option<Vec<f64>>,
}

#[derive(Debug, Clone)]
struct HistogramSeriesData {
    edges: Vec<f64>,
    counts: Vec<f64>,
}

#[derive(Debug, Clone)]
struct Histogram2SeriesData {
    x_edges: Vec<f64>,
    y_edges: Vec<f64>,
    counts: Vec<f64>,
    count_range: (f64, f64),
}

#[derive(Debug, Clone)]
struct PieSeriesData {
    slices: Vec<PieSlice>,
}

#[derive(Debug, Clone)]
struct PieSlice {
    start_angle: f64,
    end_angle: f64,
    exploded: bool,
    label: String,
    color: &'static str,
}

#[derive(Debug, Clone)]
struct ImageSeriesData {
    rows: usize,
    cols: usize,
    values: Vec<f64>,
    rgb_values: Option<Vec<[f64; 3]>>,
    alpha_data: ImageAlphaData,
    alpha_mapping: AlphaDataMapping,
    x_data: Vec<f64>,
    y_data: Vec<f64>,
    display_range: (f64, f64),
    mapping: ImageMapping,
}

#[derive(Debug, Clone)]
enum ImageAlphaData {
    Scalar(f64),
    Matrix(Vec<f64>),
}

#[derive(Debug, Clone)]
struct ContourSeriesData {
    segments: Vec<ContourSegment>,
    x_domain: (f64, f64),
    y_domain: (f64, f64),
    level_range: (f64, f64),
}

#[derive(Debug, Clone)]
struct ContourFillSeriesData {
    patches: Vec<ContourFillPatch>,
    x_domain: (f64, f64),
    y_domain: (f64, f64),
    level_range: (f64, f64),
}

#[derive(Debug, Clone)]
struct SurfaceSeriesData {
    patches: Vec<SurfacePatch>,
    grid: Option<SurfaceGridData>,
    x_range: (f64, f64),
    y_range: (f64, f64),
    z_range: (f64, f64),
}

#[derive(Debug, Clone)]
struct SurfaceGridData {
    rows: usize,
    cols: usize,
    x_values: Vec<f64>,
    y_values: Vec<f64>,
    z_values: Vec<f64>,
}

#[derive(Debug, Clone)]
struct ThreeDSeriesData {
    points: Vec<(f64, f64, f64)>,
    x_range: (f64, f64),
    y_range: (f64, f64),
    z_range: (f64, f64),
}

#[derive(Debug, Clone, Copy)]
struct ThreeDRange {
    x_range: (f64, f64),
    y_range: (f64, f64),
    z_range: (f64, f64),
}

#[derive(Debug, Clone, Copy)]
struct ContourSegment {
    start: (f64, f64),
    end: (f64, f64),
    level: f64,
}

#[derive(Debug, Clone, Copy)]
struct ContourFillPatch {
    points: [(f64, f64); 4],
    color_value: f64,
}

#[derive(Debug, Clone, Copy)]
struct SurfacePatch {
    points: [(f64, f64, f64); 4],
    color_value: f64,
}

#[derive(Debug, Clone, Copy)]
struct ViewerPoint {
    screen_x: f64,
    screen_y: f64,
    data_x: f64,
    data_y: f64,
    data_z: Option<f64>,
}

#[derive(Debug, Clone)]
struct LinkedAxesGroup {
    handles: Vec<u32>,
    mode: LinkAxesMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkAxesMode {
    X,
    Y,
    XY,
}

impl LinkAxesMode {
    fn links_x(self) -> bool {
        matches!(self, Self::X | Self::XY)
    }

    fn links_y(self) -> bool {
        matches!(self, Self::Y | Self::XY)
    }

    fn as_text(self) -> &'static str {
        match self {
            Self::X => "x",
            Self::Y => "y",
            Self::XY => "xy",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SeriesKind {
    Line,
    ReferenceLine,
    Line3D,
    ErrorBar,
    Stem3D,
    Scatter,
    Scatter3D,
    Quiver,
    Quiver3D,
    Pie,
    Pie3,
    Histogram,
    Histogram2,
    Area,
    Stairs,
    Bar,
    BarHorizontal,
    Stem,
    Contour,
    Contour3,
    ContourFill,
    Waterfall,
    Ribbon,
    Mesh,
    Surface,
    Image,
    Text,
    Rectangle,
    Patch,
}

impl SeriesKind {
    fn property_type_name(self) -> &'static str {
        match self {
            Self::Line | Self::Line3D => "line",
            Self::ReferenceLine => "constantline",
            Self::ErrorBar => "errorbar",
            Self::Stem3D => "stem",
            Self::Scatter | Self::Scatter3D => "scatter",
            Self::Quiver | Self::Quiver3D => "quiver",
            Self::Pie | Self::Pie3 => "pie",
            Self::Histogram | Self::Histogram2 => "histogram",
            Self::Area => "area",
            Self::Stairs => "stairs",
            Self::Bar | Self::BarHorizontal => "bar",
            Self::Stem => "stem",
            Self::Contour | Self::Contour3 | Self::ContourFill => "contour",
            Self::Waterfall => "patch",
            Self::Ribbon => "surface",
            Self::Mesh => "mesh",
            Self::Surface => "surface",
            Self::Image => "image",
            Self::Text => "text",
            Self::Rectangle => "rectangle",
            Self::Patch => "patch",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ColormapKind {
    #[default]
    Parula,
    Gray,
    Hot,
    Jet,
    Cool,
    Spring,
    Summer,
    Autumn,
    Winter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImageMode {
    Scaled,
    UnitRange,
    Direct,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImageMapping {
    Scaled,
    Direct,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AlphaDataMapping {
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum AxisAspectMode {
    #[default]
    Auto,
    Equal,
    Square,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ShadingMode {
    #[default]
    Faceted,
    Flat,
    Interp,
}

impl ColormapKind {
    fn as_text(self) -> &'static str {
        match self {
            Self::Parula => "parula",
            Self::Gray => "gray",
            Self::Hot => "hot",
            Self::Jet => "jet",
            Self::Cool => "cool",
            Self::Spring => "spring",
            Self::Summer => "summer",
            Self::Autumn => "autumn",
            Self::Winter => "winter",
        }
    }
}

impl ShadingMode {
    fn as_text(self) -> &'static str {
        match self {
            Self::Faceted => "faceted",
            Self::Flat => "flat",
            Self::Interp => "interp",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AxisScale {
    Linear,
    Log,
}

impl AxisScale {
    fn transform(self, value: f64) -> Option<f64> {
        match self {
            Self::Linear => Some(value),
            Self::Log if value > 0.0 && value.is_finite() => Some(value.log10()),
            Self::Log => None,
        }
    }

    fn as_text(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::Log => "log",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegendLocation {
    Best,
    North,
    South,
    East,
    West,
    Northeast,
    Northwest,
    Southeast,
    Southwest,
}

impl LegendLocation {
    fn from_text(text: &str) -> Option<Self> {
        match text.to_ascii_lowercase().as_str() {
            "best" => Some(Self::Best),
            "north" => Some(Self::North),
            "south" => Some(Self::South),
            "east" => Some(Self::East),
            "west" => Some(Self::West),
            "northeast" => Some(Self::Northeast),
            "northwest" => Some(Self::Northwest),
            "southeast" => Some(Self::Southeast),
            "southwest" => Some(Self::Southwest),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegendOrientation {
    Vertical,
    Horizontal,
}

impl LegendOrientation {
    fn from_text(text: &str) -> Option<Self> {
        match text.to_ascii_lowercase().as_str() {
            "vertical" => Some(Self::Vertical),
            "horizontal" => Some(Self::Horizontal),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct AxesFrame {
    left: f64,
    top: f64,
    width: f64,
    height: f64,
    x_scale: AxisScale,
    y_scale: AxisScale,
}

impl AxesFrame {
    fn right(self) -> f64 {
        self.left + self.width
    }

    fn bottom(self) -> f64 {
        self.top + self.height
    }

    fn with_y_scale(mut self, scale: AxisScale) -> Self {
        self.y_scale = scale;
        self
    }
}

const SERIES_COLORS: &[&str] = &[
    "#1f77b4", "#d62728", "#2ca02c", "#ff7f0e", "#9467bd", "#17becf", "#8c564b",
];

fn default_annotation_object(handle: u32, kind: AnnotationKind) -> AnnotationObject {
    AnnotationObject {
        handle,
        kind,
        x: vec![0.3, 0.4],
        y: vec![0.3, 0.4],
        position: Some([0.3, 0.3, 0.1, 0.1]),
        text: String::new(),
        color: "#1f77b4",
        line_width: 1.5,
        line_style: LineStyle::Solid,
        visible: true,
        face_color: None,
        font_size: 12.0,
    }
}

pub(crate) fn invoke_graphics_builtin_outputs(
    state: &mut GraphicsState,
    name: &str,
    args: &[Value],
    output_arity: usize,
) -> Option<Result<Vec<Value>, RuntimeError>> {
    let result = match name {
        "figure" => builtin_figure(state, args, output_arity),
        "gcf" => builtin_gcf(state, args, output_arity),
        "gca" => builtin_gca(state, args, output_arity),
        "gco" => builtin_gco(state, args, output_arity),
        "clf" => builtin_clf(state, args, output_arity),
        "cla" => builtin_cla(state, args, output_arity),
        "closereq" => builtin_closereq(state, args, output_arity),
        "close" => builtin_close(state, args, output_arity),
        "allchild" => builtin_allchild(state, args, output_arity),
        "ancestor" => builtin_ancestor(state, args, output_arity),
        "ishghandle" => builtin_ishghandle(state, args, output_arity),
        "isgraphics" => builtin_isgraphics(state, args, output_arity),
        "delete" => builtin_delete(state, args, output_arity),
        "copyobj" => builtin_copyobj(state, args, output_arity),
        "reset" => builtin_reset(state, args, output_arity),
        "findobj" => builtin_findobj(state, args, output_arity),
        "findall" => builtin_findall(state, args, output_arity),
        "subplot" => builtin_subplot(state, args, output_arity),
        "tiledlayout" => builtin_tiledlayout(state, args, output_arity),
        "nexttile" => builtin_nexttile(state, args, output_arity),
        "hold" => builtin_hold(state, args, output_arity),
        "ishold" => builtin_ishold(state, args, output_arity),
        "get" => builtin_get(state, args, output_arity),
        "set" => builtin_set(state, args, output_arity),
        "animatedline" => builtin_animatedline(state, args, output_arity),
        "addpoints" => builtin_addpoints(state, args, output_arity),
        "clearpoints" => builtin_clearpoints(state, args, output_arity),
        "getpoints" => builtin_getpoints(state, args, output_arity),
        "line" => builtin_line(state, args, output_arity),
        "annotation" => builtin_annotation(state, args, output_arity),
        "xline" => builtin_xline(state, args, output_arity),
        "yline" => builtin_yline(state, args, output_arity),
        "plot" => builtin_plot(state, args, output_arity),
        "semilogx" => builtin_semilogx(state, args, output_arity),
        "semilogy" => builtin_semilogy(state, args, output_arity),
        "loglog" => builtin_loglog(state, args, output_arity),
        "plot3" => builtin_plot3(state, args, output_arity),
        "plotyy" => builtin_plotyy(state, args, output_arity),
        "errorbar" => builtin_errorbar(state, args, output_arity),
        "scatter" => builtin_scatter(state, args, output_arity),
        "scatter3" => builtin_scatter3(state, args, output_arity),
        "quiver" => builtin_quiver(state, args, output_arity),
        "quiver3" => builtin_quiver3(state, args, output_arity),
        "pie" => builtin_pie(state, args, output_arity),
        "pie3" => builtin_pie3(state, args, output_arity),
        "histogram" => builtin_histogram(state, args, output_arity),
        "histogram2" => builtin_histogram2(state, args, output_arity),
        "area" => builtin_area(state, args, output_arity),
        "stairs" => builtin_stairs(state, args, output_arity),
        "bar" => builtin_bar(state, args, output_arity),
        "barh" => builtin_barh(state, args, output_arity),
        "stem" => builtin_stem(state, args, output_arity),
        "stem3" => builtin_stem3(state, args, output_arity),
        "contour" => builtin_contour(state, args, output_arity),
        "contour3" => builtin_contour3(state, args, output_arity),
        "contourf" => builtin_contourf(state, args, output_arity),
        "mesh" => builtin_mesh(state, args, output_arity),
        "meshc" => builtin_meshc(state, args, output_arity),
        "meshz" => builtin_meshz(state, args, output_arity),
        "waterfall" => builtin_waterfall(state, args, output_arity),
        "ribbon" => builtin_ribbon(state, args, output_arity),
        "bar3" => builtin_bar3(state, args, output_arity),
        "bar3h" => builtin_bar3h(state, args, output_arity),
        "surf" => builtin_surf(state, args, output_arity),
        "surfc" => builtin_surfc(state, args, output_arity),
        "image" => builtin_image(state, args, output_arity),
        "imagesc" => builtin_imagesc(state, args, output_arity),
        "text" => builtin_text(state, args, output_arity),
        "rectangle" => builtin_rectangle(state, args, output_arity),
        "patch" => builtin_patch(state, args, output_arity),
        "fill" => builtin_fill(state, args, output_arity),
        "fill3" => builtin_fill3(state, args, output_arity),
        "axes" => builtin_axes(state, args, output_arity),
        "axis" => builtin_axis(state, args, output_arity),
        "view" => builtin_view(state, args, output_arity),
        "linkaxes" => builtin_linkaxes(state, args, output_arity),
        "rotate3d" => builtin_rotate3d(state, args, output_arity),
        "grid" => builtin_grid(state, args, output_arity),
        "box" => builtin_box(state, args, output_arity),
        "xscale" => builtin_axis_scale(state, args, output_arity, ScaleKind::X),
        "yscale" => builtin_axis_scale(state, args, output_arity, ScaleKind::Y),
        "shading" => builtin_shading(state, args, output_arity),
        "imshow" => builtin_imshow(state, args, output_arity),
        "caxis" => builtin_caxis(state, args, output_arity),
        "colormap" => builtin_colormap(state, args, output_arity),
        "colorbar" => builtin_colorbar(state, args, output_arity),
        "legend" => builtin_legend(state, args, output_arity),
        "sgtitle" => builtin_sgtitle(state, args, output_arity),
        "title" => builtin_label(state, args, output_arity, LabelKind::Title),
        "subtitle" => builtin_label(state, args, output_arity, LabelKind::Subtitle),
        "xlabel" => builtin_label(state, args, output_arity, LabelKind::XLabel),
        "ylabel" => builtin_label(state, args, output_arity, LabelKind::YLabel),
        "zlabel" => builtin_label(state, args, output_arity, LabelKind::ZLabel),
        "yyaxis" => builtin_yyaxis(state, args, output_arity),
        "xticks" => builtin_ticks(state, args, output_arity, TickKind::X),
        "yticks" => builtin_ticks(state, args, output_arity, TickKind::Y),
        "zticks" => builtin_ticks(state, args, output_arity, TickKind::Z),
        "xticklabels" => builtin_tick_labels(state, args, output_arity, TickKind::X),
        "yticklabels" => builtin_tick_labels(state, args, output_arity, TickKind::Y),
        "zticklabels" => builtin_tick_labels(state, args, output_arity, TickKind::Z),
        "xtickangle" => builtin_tick_angle(state, args, output_arity, TickKind::X),
        "ytickangle" => builtin_tick_angle(state, args, output_arity, TickKind::Y),
        "ztickangle" => builtin_tick_angle(state, args, output_arity, TickKind::Z),
        "xlim" => builtin_limits(state, args, output_arity, LimitKind::X),
        "ylim" => builtin_limits(state, args, output_arity, LimitKind::Y),
        "zlim" => builtin_limits(state, args, output_arity, LimitKind::Z),
        "print" => builtin_print(state, args, output_arity),
        "saveas" => builtin_saveas(state, args, output_arity),
        "exportgraphics" => builtin_exportgraphics(state, args, output_arity),
        _ => return None,
    };
    Some(result)
}

fn builtin_figure(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let request = parse_figure_creation_request(args)?;
    let handle = match request.handle {
        Some(requested_handle) => {
            if graphics_handle_in_use(state, requested_handle)
                && !state.figures.contains_key(&requested_handle)
            {
                return Err(RuntimeError::Unsupported(format!(
                    "figure handle `{requested_handle}` is already used by a non-figure graphics object"
                )));
            }
            let existed = state.figures.contains_key(&requested_handle);
            let handle = select_or_create_figure(state, requested_handle);
            if existed {
                let figure = state
                    .figures
                    .get_mut(&handle)
                    .expect("selected figure should exist");
                figure.visible = true;
            }
            handle
        }
        None => create_new_figure(state),
    };

    state.current_figure = Some(handle);
    if !request.property_pairs.is_empty() {
        apply_graphics_property_pairs(state, handle, &request.property_pairs)?;
    }
    one_or_zero_outputs(Value::Scalar(handle as f64), output_arity, "figure")
}

fn builtin_gcf(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    if !args.is_empty() {
        return Err(RuntimeError::Unsupported(
            "gcf currently supports no input arguments".to_string(),
        ));
    }
    let handle = ensure_current_figure(state);
    one_or_zero_outputs(Value::Scalar(handle as f64), output_arity, "gcf")
}

fn builtin_gca(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    if !args.is_empty() {
        return Err(RuntimeError::Unsupported(
            "gca currently supports no input arguments".to_string(),
        ));
    }

    let handle = current_axes_handle(state);
    one_or_zero_outputs(Value::Scalar(handle as f64), output_arity, "gca")
}

fn builtin_gco(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    if !args.is_empty() {
        return Err(RuntimeError::Unsupported(
            "gco currently supports no input arguments".to_string(),
        ));
    }

    let value = current_object_value(state)?;
    one_or_zero_outputs(value, output_arity, "gco")
}

fn builtin_clf(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let (handle, reset) = match args {
        [] => (ensure_current_figure(state), false),
        [mode] if is_text_keyword(mode, "reset")? => (ensure_current_figure(state), true),
        [requested] => (scalar_handle(requested, "clf")?, false),
        [requested, mode] if is_text_keyword(mode, "reset")? => {
            (scalar_handle(requested, "clf")?, true)
        }
        _ => {
            return Err(RuntimeError::Unsupported(
                "clf currently supports `clf`, `clf(fig)`, `clf('reset')`, or `clf(fig, 'reset')`"
                    .to_string(),
            ))
        }
    };

    let figure = state.figures.get_mut(&handle).ok_or_else(|| {
        RuntimeError::MissingVariable(format!("figure handle `{handle}` does not exist"))
    })?;
    let preserved = figure.clone();
    *figure = if reset {
        FigureState {
            position: preserved.position,
            window_style: preserved.window_style,
            paper_units: preserved.paper_units,
            paper_position_in: preserved.paper_position_in,
            paper_position_mode: preserved.paper_position_mode,
            ..FigureState::default()
        }
    } else {
        FigureState {
            name: preserved.name,
            number_title: preserved.number_title,
            visible: preserved.visible,
            position: preserved.position,
            window_style: preserved.window_style,
            paper_units: preserved.paper_units,
            paper_type: preserved.paper_type,
            paper_size_in: preserved.paper_size_in,
            paper_position_in: preserved.paper_position_in,
            paper_position_mode: preserved.paper_position_mode,
            paper_orientation: preserved.paper_orientation,
            ..FigureState::default()
        }
    };
    state.current_figure = Some(handle);
    one_or_zero_outputs(Value::Scalar(handle as f64), output_arity, "clf")
}

fn builtin_cla(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let (handle, reset) = match args {
        [] => (current_axes_handle(state), false),
        [mode] if is_text_keyword(mode, "reset")? => (current_axes_handle(state), true),
        [requested] => (scalar_handle(requested, "cla")?, false),
        [requested, mode] if is_text_keyword(mode, "reset")? => {
            (scalar_handle(requested, "cla")?, true)
        }
        _ => {
            return Err(RuntimeError::Unsupported(
                "cla currently supports `cla`, `cla(ax)`, `cla('reset')`, or `cla(ax, 'reset')`"
                    .to_string(),
            ))
        }
    };

    let axes = &mut axes_slot_mut_by_handle(state, handle)?.axes;
    if reset {
        let position = axes.position;
        *axes = AxesState {
            position,
            ..AxesState::default()
        };
    } else {
        axes.series.clear();
        axes.legend = None;
    }
    one_or_zero_outputs(Value::Scalar(handle as f64), output_arity, "cla")
}

fn builtin_close(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let request = close_request_handles(state, args, "close")?;
    close_figures_now(state, &request.handles)?;
    let status = request.status_if_empty || !request.handles.is_empty();

    one_or_zero_outputs(
        Value::Scalar(if status { 1.0 } else { 0.0 }),
        output_arity,
        "close",
    )
}

fn builtin_closereq(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    if !args.is_empty() {
        return Err(RuntimeError::Unsupported(
            "closereq currently supports no input arguments".to_string(),
        ));
    }
    if output_arity > 0 {
        return Err(RuntimeError::Unsupported(
            "closereq currently does not return outputs".to_string(),
        ));
    }

    if let Some(handle) = state.current_figure {
        close_figures_now(state, &[handle])?;
    }
    Ok(Vec::new())
}

fn builtin_allchild(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let [requested] = args else {
        return Err(RuntimeError::Unsupported(
            "allchild currently supports exactly one graphics handle argument".to_string(),
        ));
    };

    let handle = scalar_handle(requested, "allchild")?;
    let children = match graphics_handle_kind(state, handle).ok_or_else(|| {
        RuntimeError::MissingVariable(format!("graphics handle `{handle}` does not exist"))
    })? {
        GraphicsHandleKind::Figure => figure_children_handles(
            state
                .figures
                .get(&handle)
                .expect("figure handle should exist"),
        ),
        GraphicsHandleKind::Axes => axes_children_handles(axes_slot_by_handle(state, handle)?),
        GraphicsHandleKind::Annotation => Vec::new(),
        GraphicsHandleKind::Series => Vec::new(),
    };

    one_or_zero_outputs(
        graphics_handle_vector_value(children)?,
        output_arity,
        "allchild",
    )
}

fn builtin_ancestor(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let [requested_handle, requested_kind] = args else {
        return Err(RuntimeError::Unsupported(
            "ancestor currently supports exactly two arguments: a graphics handle and one type name"
                .to_string(),
        ));
    };

    let handle = scalar_handle(requested_handle, "ancestor")?;
    let kind = text_arg(requested_kind, "ancestor")?.to_ascii_lowercase();
    let ancestor = ancestor_handle(state, handle, &kind)?;
    let value = match ancestor {
        Some(handle) => Value::Scalar(handle as f64),
        None => empty_matrix_value()?,
    };
    one_or_zero_outputs(value, output_arity, "ancestor")
}

fn builtin_ishghandle(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let [requested] = args else {
        return Err(RuntimeError::Unsupported(
            "ishghandle currently supports exactly one numeric handle array argument".to_string(),
        ));
    };

    let value = graphics_handle_query_value(state, requested, None, "ishghandle")?;
    one_or_zero_outputs(value, output_arity, "ishghandle")
}

fn builtin_isgraphics(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let (requested, expected_type) = match args {
        [requested] => (requested.clone(), None),
        [requested, expected_type] => {
            (requested.clone(), Some(text_arg(expected_type, "isgraphics")?))
        }
        _ => {
            return Err(RuntimeError::Unsupported(
                "isgraphics currently supports one handle array argument or a handle array plus one type name".to_string(),
            ))
        }
    };

    let value =
        graphics_handle_query_value(state, &requested, expected_type.as_deref(), "isgraphics")?;
    one_or_zero_outputs(value, output_arity, "isgraphics")
}

fn builtin_delete(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let [requested] = args else {
        return Err(RuntimeError::Unsupported(
            "delete currently supports exactly one numeric graphics handle array".to_string(),
        ));
    };

    let handles = graphics_handle_inputs(requested, "delete")?;
    let mut ordered = handles
        .handles
        .into_iter()
        .map(|handle| {
            let kind = graphics_handle_kind(state, handle).ok_or_else(|| {
                RuntimeError::MissingVariable(format!("graphics handle `{handle}` does not exist"))
            })?;
            Ok((delete_priority(kind), handle))
        })
        .collect::<Result<Vec<_>, RuntimeError>>()?;
    ordered.sort_unstable();
    ordered.dedup();

    for (_, handle) in ordered {
        delete_graphics_handle(state, handle)?;
    }

    one_or_zero_outputs(empty_matrix_value()?, output_arity, "delete")
}

fn builtin_copyobj(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let [sources, target] = args else {
        return Err(RuntimeError::Unsupported(
            "copyobj currently supports exactly two arguments: source handles and one parent handle"
                .to_string(),
        ));
    };

    let source_handles = graphics_handle_inputs(sources, "copyobj")?;
    let target_handle = scalar_handle(target, "copyobj")?;
    let target_kind = graphics_handle_kind(state, target_handle).ok_or_else(|| {
        RuntimeError::MissingVariable(format!("graphics handle `{target_handle}` does not exist"))
    })?;

    let copied = source_handles
        .handles
        .iter()
        .copied()
        .map(|source| copy_graphics_handle(state, source, target_handle, target_kind))
        .collect::<Result<Vec<_>, _>>()?;
    let value = if source_handles.scalar_input {
        Value::Scalar(copied[0] as f64)
    } else {
        Value::Matrix(MatrixValue::new(
            source_handles.rows,
            source_handles.cols,
            copied
                .into_iter()
                .map(|handle| Value::Scalar(handle as f64))
                .collect(),
        )?)
    };
    one_or_zero_outputs(value, output_arity, "copyobj")
}

fn builtin_reset(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let [requested] = args else {
        return Err(RuntimeError::Unsupported(
            "reset currently supports exactly one graphics handle or handle array".to_string(),
        ));
    };

    let handles = graphics_handle_inputs(requested, "reset")?;
    for handle in &handles.handles {
        reset_graphics_handle(state, *handle)?;
    }

    one_or_zero_outputs(
        graphics_handle_inputs_value(&handles)?,
        output_arity,
        "reset",
    )
}

fn builtin_findobj(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let value = find_graphics_objects_value(state, args, "findobj")?;
    one_or_zero_outputs(value, output_arity, "findobj")
}

fn builtin_findall(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let value = find_graphics_objects_value(state, args, "findall")?;
    one_or_zero_outputs(value, output_arity, "findall")
}

fn builtin_subplot(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let (rows, cols, index) = parse_subplot_args(args)?;
    let figure_handle = ensure_current_figure(state);
    {
        let figure = state
            .figures
            .get_mut(&figure_handle)
            .expect("current figure should exist");
        if figure.layout_rows != rows || figure.layout_cols != cols {
            figure.layout_rows = rows;
            figure.layout_cols = cols;
            figure.axes.clear();
            figure.linked_axes.clear();
        }
        figure.current_axes = index;
    }
    let handle = ensure_axes_slot(state, figure_handle, index);
    set_current_object_for_handle(state, handle);
    one_or_zero_outputs(Value::Scalar(handle as f64), output_arity, "subplot")
}

fn builtin_tiledlayout(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let (rows, cols) = parse_tiledlayout_args(args)?;
    let figure_handle = ensure_current_figure(state);
    {
        let figure = state
            .figures
            .get_mut(&figure_handle)
            .expect("current figure should exist");
        if figure.layout_rows != rows || figure.layout_cols != cols {
            figure.layout_rows = rows;
            figure.layout_cols = cols;
            figure.current_axes = 1;
            figure.axes.clear();
            figure.linked_axes.clear();
        }
    }
    one_or_zero_outputs(
        Value::Scalar(figure_handle as f64),
        output_arity,
        "tiledlayout",
    )
}

fn builtin_nexttile(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let figure_handle = ensure_current_figure(state);
    let tile_index =
        {
            let figure = state
                .figures
                .get(&figure_handle)
                .expect("current figure should exist");
            match args {
                [] => next_tile_index(figure),
                [requested] => {
                    let index = scalar_usize(requested, "nexttile")?;
                    let max_tiles = figure.layout_rows.max(1) * figure.layout_cols.max(1);
                    if index == 0 || index > max_tiles {
                        return Err(RuntimeError::ShapeError(format!(
                            "nexttile requires a tile index within 1..={}, found {index}",
                            max_tiles
                        )));
                    }
                    index
                }
                _ => return Err(RuntimeError::Unsupported(
                    "nexttile currently supports no arguments or one positive scalar tile index"
                        .to_string(),
                )),
            }
        };
    {
        let figure = state
            .figures
            .get_mut(&figure_handle)
            .expect("current figure should exist");
        figure.current_axes = tile_index;
    }
    let handle = ensure_axes_slot(state, figure_handle, tile_index);
    set_current_object_for_handle(state, handle);
    one_or_zero_outputs(Value::Scalar(handle as f64), output_arity, "nexttile")
}

fn builtin_hold(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let (target_axes, enabled) = match args {
        [] => {
            ensure_current_figure(state);
            let enabled = !current_axes_snapshot(current_figure(state)).hold_enabled;
            (None, enabled)
        }
        [mode] if matches!(mode, Value::CharArray(_) | Value::String(_)) => {
            (None, parse_hold_mode(mode)?)
        }
        [axes] => {
            let handle = scalar_handle(axes, "hold")?;
            let enabled = !axes_slot_by_handle(state, handle)?.axes.hold_enabled;
            (Some(handle), enabled)
        }
        [axes, mode] => (Some(scalar_handle(axes, "hold")?), parse_hold_mode(mode)?),
        _ => {
            return Err(RuntimeError::Unsupported(
                "hold currently supports `hold`, `hold(state)`, `hold(ax)`, or `hold(ax, state)`"
                    .to_string(),
            ))
        }
    };

    let output_value = if let Some(handle) = target_axes {
        axes_slot_mut_by_handle(state, handle)?.axes.hold_enabled = enabled;
        Value::Scalar(handle as f64)
    } else {
        let figure_handle = ensure_current_figure(state);
        current_axes_mut(state).hold_enabled = enabled;
        Value::Scalar(figure_handle as f64)
    };
    one_or_zero_outputs(output_value, output_arity, "hold")
}

fn parse_hold_mode(value: &Value) -> Result<bool, RuntimeError> {
    match text_arg(value, "hold")?.to_ascii_lowercase().as_str() {
        "on" | "all" => Ok(true),
        "off" => Ok(false),
        other => Err(RuntimeError::Unsupported(format!(
            "hold currently supports only `on`, `off`, or `all`, found `{other}`"
        ))),
    }
}

fn builtin_ishold(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let enabled = match args {
        [] => {
            let figure = current_figure(state);
            current_axes_snapshot(figure).hold_enabled
        }
        [handle] => {
            axes_slot_by_handle(state, scalar_handle(handle, "ishold")?)?
                .axes
                .hold_enabled
        }
        _ => {
            return Err(RuntimeError::Unsupported(
                "ishold currently supports no arguments or one axes handle".to_string(),
            ))
        }
    };

    one_or_zero_outputs(Value::Logical(enabled), output_arity, "ishold")
}

fn builtin_get(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let value = match args {
        [handle_value] => {
            let handles = graphics_handle_inputs(handle_value, "get")?;
            if handles.scalar_input {
                graphics_property_struct_value(state, handles.handles[0])?
            } else {
                Value::Cell(CellValue::new(
                    handles.rows,
                    handles.cols,
                    handles
                        .handles
                        .into_iter()
                        .map(|handle| graphics_property_struct_value(state, handle))
                        .collect::<Result<Vec<_>, _>>()?,
                )?)
            }
        }
        [handle_value, property_value] => {
            let handles = graphics_handle_inputs(handle_value, "get")?;
            if handles.scalar_input {
                graphics_property_value_for_handle(state, handles.handles[0], property_value, "get")?
            } else {
                Value::Cell(CellValue::new(
                    handles.rows,
                    handles.cols,
                    handles
                        .handles
                        .into_iter()
                        .map(|handle| graphics_property_value_for_handle(state, handle, property_value, "get"))
                        .collect::<Result<Vec<_>, _>>()?,
                )?)
            }
        }
        _ => {
            return Err(RuntimeError::Unsupported(
                "get currently supports one graphics handle argument for full property query or two arguments for a specific property query".to_string(),
            ))
        }
    };

    one_or_zero_outputs(value, output_arity, "get")
}

fn builtin_set(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let handles = graphics_handle_inputs(&args[0], "set")?;
    match args {
        [_, Value::Struct(props)] => {
            for handle in &handles.handles {
                apply_graphics_property_struct(state, *handle, props)?;
            }
        }
        [_, ..] if args.len() >= 3 && args.len() % 2 == 1 => {
            for handle in &handles.handles {
                apply_graphics_property_pairs(state, *handle, &args[1..])?;
            }
        }
        _ => {
            return Err(RuntimeError::Unsupported(
                "set currently supports a graphics handle plus one property struct or one or more property/value pairs".to_string(),
            ))
        }
    }

    one_or_zero_outputs(graphics_handle_inputs_value(&handles)?, output_arity, "set")
}

fn builtin_animatedline(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    if args.len() % 2 != 0 {
        return Err(RuntimeError::Unsupported(
            "animatedline currently supports no arguments or property/value pairs".to_string(),
        ));
    }

    let series_handle = next_series_handle(state);
    {
        let axes = current_axes_mut(state);
        let color = SERIES_COLORS[axes.series.len() % SERIES_COLORS.len()];
        let mut series = make_series(series_handle, SeriesKind::Line, color);
        apply_series_property_pairs(&mut series, args, "animatedline")?;
        axes.series.push(series);
    }
    set_current_object_for_handle(state, series_handle);
    one_or_zero_outputs(
        Value::Scalar(series_handle as f64),
        output_arity,
        "animatedline",
    )
}

fn builtin_addpoints(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    match args {
        [handle, x, y] => {
            let handle = scalar_handle(handle, "addpoints")?;
            let x_values = numeric_vector(x, "addpoints")?;
            let y_values = numeric_vector(y, "addpoints")?;
            if x_values.len() != y_values.len() {
                return Err(RuntimeError::ShapeError(format!(
                    "addpoints requires x and y vectors with matching lengths, found {} and {}",
                    x_values.len(),
                    y_values.len()
                )));
            }
            {
                let series = series_mut_by_handle(state, handle)?;
                if series.kind != SeriesKind::Line {
                    return Err(RuntimeError::Unsupported(
                        "addpoints currently supports 2-D line/animatedline handles".to_string(),
                    ));
                }
                series.x.extend(x_values);
                series.y.extend(y_values);
                trim_series_to_maximum_num_points(series);
            }
            set_current_object_for_handle(state, handle);
            if output_arity > 0 {
                Err(RuntimeError::Unsupported(
                    "addpoints currently does not return outputs".to_string(),
                ))
            } else {
                Ok(Vec::new())
            }
        }
        [handle, x, y, z] => {
            let handle = scalar_handle(handle, "addpoints")?;
            let x_values = numeric_vector(x, "addpoints")?;
            let y_values = numeric_vector(y, "addpoints")?;
            let z_values = numeric_vector(z, "addpoints")?;
            if x_values.len() != y_values.len() || x_values.len() != z_values.len() {
                return Err(RuntimeError::ShapeError(format!(
                    "addpoints requires x, y, and z vectors with matching lengths, found {}, {}, and {}",
                    x_values.len(),
                    y_values.len(),
                    z_values.len()
                )));
            }
            {
                let series = series_mut_by_handle(state, handle)?;
                if series.kind != SeriesKind::Line3D && series.kind != SeriesKind::Line {
                    return Err(RuntimeError::Unsupported(
                        "addpoints currently supports line-like handles only".to_string(),
                    ));
                }
                let mut points = series
                    .three_d
                    .as_ref()
                    .map(|three_d| three_d.points.clone())
                    .unwrap_or_default();
                points.extend(
                    x_values
                        .into_iter()
                        .zip(y_values)
                        .zip(z_values)
                        .map(|((x, y), z)| (x, y, z)),
                );
                series.kind = SeriesKind::Line3D;
                series.three_d = Some(three_d_series_from_points(points));
                series.x.clear();
                series.y.clear();
                trim_series_to_maximum_num_points(series);
            }
            set_current_object_for_handle(state, handle);
            if output_arity > 0 {
                Err(RuntimeError::Unsupported(
                    "addpoints currently does not return outputs".to_string(),
                ))
            } else {
                Ok(Vec::new())
            }
        }
        _ => Err(RuntimeError::Unsupported(
            "addpoints currently supports `addpoints(h, x, y)` or `addpoints(h, x, y, z)`"
                .to_string(),
        )),
    }
}

fn builtin_clearpoints(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let [handle] = args else {
        return Err(RuntimeError::Unsupported(
            "clearpoints currently supports exactly one line/animatedline handle".to_string(),
        ));
    };
    let handle = scalar_handle(handle, "clearpoints")?;
    {
        let series = series_mut_by_handle(state, handle)?;
        if series.kind != SeriesKind::Line && series.kind != SeriesKind::Line3D {
            return Err(RuntimeError::Unsupported(
                "clearpoints currently supports line-like handles only".to_string(),
            ));
        }
        series.x.clear();
        series.y.clear();
        series.three_d = None;
    }
    set_current_object_for_handle(state, handle);
    if output_arity > 0 {
        Err(RuntimeError::Unsupported(
            "clearpoints currently does not return outputs".to_string(),
        ))
    } else {
        Ok(Vec::new())
    }
}

fn builtin_getpoints(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let [handle] = args else {
        return Err(RuntimeError::Unsupported(
            "getpoints currently supports exactly one line/animatedline handle".to_string(),
        ));
    };
    let handle = scalar_handle(handle, "getpoints")?;
    let series = series_by_handle(state, handle)?;
    match (&series.kind, &series.three_d) {
        (SeriesKind::Line, _) => match output_arity {
            2 => Ok(vec![
                tick_values_value(&series.x)?,
                tick_values_value(&series.y)?,
            ]),
            _ => Err(RuntimeError::Unsupported(
                "getpoints currently expects exactly two outputs for 2-D animated lines"
                    .to_string(),
            )),
        },
        (SeriesKind::Line3D, Some(three_d)) => {
            let x = three_d
                .points
                .iter()
                .map(|(x, _, _)| *x)
                .collect::<Vec<_>>();
            let y = three_d
                .points
                .iter()
                .map(|(_, y, _)| *y)
                .collect::<Vec<_>>();
            let z = three_d
                .points
                .iter()
                .map(|(_, _, z)| *z)
                .collect::<Vec<_>>();
            match output_arity {
                3 => Ok(vec![
                    tick_values_value(&x)?,
                    tick_values_value(&y)?,
                    tick_values_value(&z)?,
                ]),
                _ => Err(RuntimeError::Unsupported(
                    "getpoints currently expects exactly three outputs for 3-D animated lines"
                        .to_string(),
                )),
            }
        }
        _ => Err(RuntimeError::Unsupported(
            "getpoints currently supports line-like animated handles only".to_string(),
        )),
    }
}

fn builtin_plot(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    builtin_xy_series(
        state,
        args,
        output_arity,
        "plot",
        SeriesKind::Line,
        true,
        true,
    )
}

fn builtin_semilogx(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    builtin_xy_series_with_scales(
        state,
        args,
        output_arity,
        "semilogx",
        AxisScale::Log,
        Some(AxisScale::Linear),
    )
}

fn builtin_semilogy(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    builtin_xy_series_with_scales(
        state,
        args,
        output_arity,
        "semilogy",
        AxisScale::Linear,
        Some(AxisScale::Log),
    )
}

fn builtin_loglog(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    builtin_xy_series_with_scales(
        state,
        args,
        output_arity,
        "loglog",
        AxisScale::Log,
        Some(AxisScale::Log),
    )
}

fn builtin_plotyy(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let [x1, y1, x2, y2] = args else {
        return Err(RuntimeError::Unsupported(
            "plotyy currently supports exactly four numeric vector arguments: x1, y1, x2, y2"
                .to_string(),
        ));
    };

    let left_x = numeric_vector(x1, "plotyy")?;
    let left_y = numeric_vector(y1, "plotyy")?;
    let right_x = numeric_vector(x2, "plotyy")?;
    let right_y = numeric_vector(y2, "plotyy")?;
    if left_x.len() != left_y.len() || right_x.len() != right_y.len() {
        return Err(RuntimeError::ShapeError(
            "plotyy requires matching x/y vector lengths on both left and right axes".to_string(),
        ));
    }

    ensure_current_figure(state);
    let axes_handle = current_axes_handle(state);
    {
        let axes = current_axes_mut(state);
        if !axes.hold_enabled {
            axes.series.clear();
            axes.legend = None;
        }
        axes.active_y_axis = YAxisSide::Left;
    }
    let left_handle = {
        let series_handle = next_series_handle(state);
        let axes = current_axes_mut(state);
        let color = SERIES_COLORS[axes.series.len() % SERIES_COLORS.len()];
        let mut series = make_series(series_handle, SeriesKind::Line, color);
        series.y_axis_side = YAxisSide::Left;
        series.x = left_x;
        series.y = left_y;
        axes.series.push(series);
        series_handle
    };
    let right_handle = {
        let series_handle = next_series_handle(state);
        let axes = current_axes_mut(state);
        let color = SERIES_COLORS[axes.series.len() % SERIES_COLORS.len()];
        let mut series = make_series(series_handle, SeriesKind::Line, color);
        series.y_axis_side = YAxisSide::Right;
        series.x = right_x;
        series.y = right_y;
        axes.series.push(series);
        series_handle
    };
    current_axes_mut(state).active_y_axis = YAxisSide::Left;
    set_current_object_for_handle(state, right_handle);

    match output_arity {
        0 => Ok(Vec::new()),
        1 => Ok(vec![Value::Matrix(MatrixValue::new(
            1,
            2,
            vec![
                Value::Scalar(axes_handle as f64),
                Value::Scalar(axes_handle as f64),
            ],
        )?)]),
        2 => Ok(vec![
            Value::Matrix(MatrixValue::new(
                1,
                2,
                vec![
                    Value::Scalar(axes_handle as f64),
                    Value::Scalar(axes_handle as f64),
                ],
            )?),
            Value::Matrix(MatrixValue::new(
                1,
                2,
                vec![
                    Value::Scalar(left_handle as f64),
                    Value::Scalar(right_handle as f64),
                ],
            )?),
        ]),
        3 => Ok(vec![
            Value::Matrix(MatrixValue::new(
                1,
                2,
                vec![
                    Value::Scalar(axes_handle as f64),
                    Value::Scalar(axes_handle as f64),
                ],
            )?),
            Value::Scalar(left_handle as f64),
            Value::Scalar(right_handle as f64),
        ]),
        _ => Err(RuntimeError::Unsupported(
            "plotyy currently supports at most three outputs".to_string(),
        )),
    }
}

fn builtin_line(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let spec = parse_line_spec(args)?;
    let series_handle = next_series_handle(state);
    let axes = current_axes_mut(state);
    let color = spec
        .color
        .unwrap_or(SERIES_COLORS[axes.series.len() % SERIES_COLORS.len()]);
    let mut series = make_series(series_handle, SeriesKind::Line, color);
    series.y_axis_side = axes.active_y_axis;
    series.x = spec.x;
    series.y = spec.y;
    series.display_name = spec.display_name;
    series.visible = spec.visible;
    series.line_width = spec.line_width;
    series.line_style = spec.line_style;
    series.marker = spec.marker;
    series.marker_size = spec.marker_size;
    series.marker_edge_color = spec.marker_edge_color;
    series.marker_face_color = spec.marker_face_color;
    axes.series.push(series);
    one_or_zero_outputs(Value::Scalar(series_handle as f64), output_arity, "line")
}

fn builtin_annotation(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let spec = parse_annotation_spec(args)?;
    let figure_handle = ensure_current_figure(state);
    let handle = next_annotation_handle(state);
    let mut annotation = default_annotation_object(handle, spec.kind);
    annotation.x = spec.x;
    annotation.y = spec.y;
    annotation.position = spec.position;
    annotation.text = spec.text;
    annotation.color = spec.color;
    annotation.line_width = spec.line_width;
    annotation.line_style = spec.line_style;
    annotation.visible = spec.visible;
    annotation.face_color = spec.face_color;
    annotation.font_size = spec.font_size;
    state
        .figures
        .get_mut(&figure_handle)
        .expect("current figure should exist")
        .annotations
        .push(annotation);
    set_current_object_for_handle(state, handle);
    one_or_zero_outputs(Value::Scalar(handle as f64), output_arity, "annotation")
}

fn builtin_xline(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    builtin_reference_line(
        state,
        args,
        output_arity,
        "xline",
        ReferenceLineOrientation::Vertical,
    )
}

fn builtin_yline(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    builtin_reference_line(
        state,
        args,
        output_arity,
        "yline",
        ReferenceLineOrientation::Horizontal,
    )
}

fn builtin_reference_line(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
    builtin_name: &str,
    orientation: ReferenceLineOrientation,
) -> Result<Vec<Value>, RuntimeError> {
    let spec = parse_reference_line_spec(args, builtin_name)?;
    let mut series_handles = Vec::with_capacity(spec.values.len());
    for (index, value) in spec.values.iter().copied().enumerate() {
        let series_handle = next_series_handle(state);
        {
            let axes = current_axes_mut(state);
            let mut series = make_series(series_handle, SeriesKind::ReferenceLine, "#222222");
            series.y_axis_side = axes.active_y_axis;
            series.line_width = 1.5;
            if let Some(style) = spec.style.as_ref() {
                apply_line_spec_to_series(&mut series, style);
            }
            if let Some(label) = spec.labels.get(index).cloned() {
                if !label.is_empty() {
                    series.display_name = Some(label.clone());
                    series.reference_line = Some(ReferenceLineData {
                        orientation,
                        value,
                        label,
                    });
                } else {
                    series.reference_line = Some(ReferenceLineData {
                        orientation,
                        value,
                        label: String::new(),
                    });
                }
            } else {
                series.reference_line = Some(ReferenceLineData {
                    orientation,
                    value,
                    label: String::new(),
                });
            }
            apply_series_property_pairs(&mut series, &spec.property_pairs, builtin_name)?;
            axes.series.push(series);
        }
        series_handles.push(series_handle);
    }
    if let Some(&series_handle) = series_handles.last() {
        set_current_object_for_handle(state, series_handle);
    }
    one_or_zero_outputs(
        series_handle_array_value(&series_handles)?,
        output_arity,
        builtin_name,
    )
}

fn builtin_plot3(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    builtin_xyz_series(
        state,
        args,
        output_arity,
        "plot3",
        SeriesKind::Line3D,
        true,
        true,
    )
}

fn builtin_errorbar(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let spec = parse_errorbar_spec(args)?;
    let series_handle = next_series_handle(state);
    {
        let axes = current_axes_mut(state);
        if !axes.hold_enabled {
            axes.series.clear();
            axes.legend = None;
        }
    }
    {
        let axes = current_axes_mut(state);
        let color = SERIES_COLORS[axes.series.len() % SERIES_COLORS.len()];
        let mut series = make_series(series_handle, SeriesKind::ErrorBar, color);
        series.y_axis_side = axes.active_y_axis;
        series.x = spec.x;
        series.y = spec.y;
        series.error_bar = Some(ErrorBarSeriesData {
            vertical_lower: spec.vertical_lower,
            vertical_upper: spec.vertical_upper,
            horizontal_lower: spec.horizontal_lower,
            horizontal_upper: spec.horizontal_upper,
        });
        if let Some(style) = spec.style.as_ref() {
            apply_line_spec_to_series(&mut series, style);
        }
        apply_series_property_pairs(&mut series, &spec.property_pairs, "errorbar")?;
        axes.series.push(series);
    }
    set_current_object_for_handle(state, series_handle);
    one_or_zero_outputs(
        Value::Scalar(series_handle as f64),
        output_arity,
        "errorbar",
    )
}

fn builtin_scatter(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let (groups, property_pairs) = parse_scatter_args(args, "scatter")?;
    {
        let axes = current_axes_mut(state);
        if !axes.hold_enabled {
            axes.series.clear();
            axes.legend = None;
        }
    }
    let mut series_handles = Vec::with_capacity(groups.len());
    for (points, scatter) in groups {
        let series_handle = next_series_handle(state);
        {
            let axes = current_axes_mut(state);
            let default_color = scatter
                .default_color()
                .unwrap_or(SERIES_COLORS[axes.series.len() % SERIES_COLORS.len()]);
            let mut series = make_series(series_handle, SeriesKind::Scatter, default_color);
            series.y_axis_side = axes.active_y_axis;
            series.x = points.x;
            series.y = points.y;
            series.line_style = LineStyle::None;
            series.marker = scatter.marker.unwrap_or(MarkerStyle::Circle);
            series.marker_size = scatter.marker_sizes.first().copied().unwrap_or(6.0);
            series.marker_edge_color = MarkerColorMode::Flat;
            if scatter.filled {
                series.marker_edge_color = MarkerColorMode::None;
                series.marker_face_color = MarkerColorMode::Flat;
            }
            let mut scatter = scatter;
            if scatter.uses_default_color {
                scatter.colors = ScatterColors::Uniform(default_color);
                scatter.uses_default_color = false;
            }
            series.scatter = Some(scatter);
            apply_series_property_pairs(&mut series, property_pairs, "scatter")?;
            axes.series.push(series);
        }
        series_handles.push(series_handle);
    }
    if let Some(&series_handle) = series_handles.last() {
        set_current_object_for_handle(state, series_handle);
    }
    one_or_zero_outputs(
        series_handle_array_value(&series_handles)?,
        output_arity,
        "scatter",
    )
}

fn builtin_scatter3(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let (groups, property_pairs) = parse_scatter3_args(args, "scatter3")?;
    {
        let axes = current_axes_mut(state);
        if !axes.hold_enabled {
            axes.series.clear();
            axes.legend = None;
        }
    }
    let mut series_handles = Vec::with_capacity(groups.len());
    for (three_d, scatter) in groups {
        let series_handle = next_series_handle(state);
        {
            let axes = current_axes_mut(state);
            let default_color = scatter
                .default_color()
                .unwrap_or(SERIES_COLORS[axes.series.len() % SERIES_COLORS.len()]);
            let mut series = make_series(series_handle, SeriesKind::Scatter3D, default_color);
            series.three_d = Some(three_d);
            series.line_style = LineStyle::None;
            series.marker = scatter.marker.unwrap_or(MarkerStyle::Circle);
            series.marker_size = scatter.marker_sizes.first().copied().unwrap_or(6.0);
            series.marker_edge_color = MarkerColorMode::Flat;
            if scatter.filled {
                series.marker_edge_color = MarkerColorMode::None;
                series.marker_face_color = MarkerColorMode::Flat;
            }
            let mut scatter = scatter;
            if scatter.uses_default_color {
                scatter.colors = ScatterColors::Uniform(default_color);
                scatter.uses_default_color = false;
            }
            series.scatter = Some(scatter);
            apply_series_property_pairs(&mut series, property_pairs, "scatter3")?;
            axes.series.push(series);
        }
        series_handles.push(series_handle);
    }
    if let Some(&series_handle) = series_handles.last() {
        set_current_object_for_handle(state, series_handle);
    }
    one_or_zero_outputs(
        series_handle_array_value(&series_handles)?,
        output_arity,
        "scatter3",
    )
}

fn builtin_quiver(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let quiver = parse_quiver_args(args)?;
    let series_handle = next_series_handle(state);
    let axes = current_axes_mut(state);
    if !axes.hold_enabled {
        axes.series.clear();
        axes.legend = None;
    }
    let color = SERIES_COLORS[axes.series.len() % SERIES_COLORS.len()];
    let mut series = make_series(series_handle, SeriesKind::Quiver, color);
    series.y_axis_side = axes.active_y_axis;
    series.quiver = Some(quiver);
    axes.series.push(series);
    set_current_object_for_handle(state, series_handle);
    one_or_zero_outputs(Value::Scalar(series_handle as f64), output_arity, "quiver")
}

fn builtin_quiver3(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let three_d = parse_quiver3_args(args)?;
    let series_handle = next_series_handle(state);
    let axes = current_axes_mut(state);
    if !axes.hold_enabled {
        axes.series.clear();
        axes.legend = None;
    }
    let color = SERIES_COLORS[axes.series.len() % SERIES_COLORS.len()];
    let mut series = make_series(series_handle, SeriesKind::Quiver3D, color);
    series.three_d = Some(three_d);
    axes.series.push(series);
    set_current_object_for_handle(state, series_handle);
    one_or_zero_outputs(Value::Scalar(series_handle as f64), output_arity, "quiver3")
}

fn builtin_pie(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let pie = parse_pie_args(args, "pie")?;
    let series_handle = next_series_handle(state);
    let axes = current_axes_mut(state);
    if !axes.hold_enabled {
        axes.series.clear();
        axes.legend = None;
    }
    axes.axis_visible = false;
    axes.aspect_mode = AxisAspectMode::Equal;
    let mut series = make_series(series_handle, SeriesKind::Pie, "#000000");
    series.pie = Some(pie);
    axes.series.push(series);
    set_current_object_for_handle(state, series_handle);
    one_or_zero_outputs(Value::Scalar(series_handle as f64), output_arity, "pie")
}

fn builtin_pie3(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let pie = parse_pie_args(args, "pie3")?;
    let series_handle = next_series_handle(state);
    let axes = current_axes_mut(state);
    if !axes.hold_enabled {
        axes.series.clear();
        axes.legend = None;
    }
    axes.axis_visible = false;
    axes.aspect_mode = AxisAspectMode::Equal;
    let mut series = make_series(series_handle, SeriesKind::Pie3, "#000000");
    series.pie = Some(pie);
    axes.series.push(series);
    set_current_object_for_handle(state, series_handle);
    one_or_zero_outputs(Value::Scalar(series_handle as f64), output_arity, "pie3")
}

fn builtin_histogram(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let histogram = parse_histogram_args(args)?;
    let series_handle = next_series_handle(state);
    let axes = current_axes_mut(state);
    if !axes.hold_enabled {
        axes.series.clear();
        axes.legend = None;
    }
    let color = SERIES_COLORS[axes.series.len() % SERIES_COLORS.len()];
    let mut series = make_series(series_handle, SeriesKind::Histogram, color);
    series.histogram = Some(histogram);
    axes.series.push(series);
    set_current_object_for_handle(state, series_handle);
    one_or_zero_outputs(
        Value::Scalar(series_handle as f64),
        output_arity,
        "histogram",
    )
}

fn builtin_histogram2(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let histogram2 = parse_histogram2_args(args)?;
    let series_handle = next_series_handle(state);
    let axes = current_axes_mut(state);
    if !axes.hold_enabled {
        axes.series.clear();
        axes.legend = None;
    }
    let color = SERIES_COLORS[axes.series.len() % SERIES_COLORS.len()];
    let mut series = make_series(series_handle, SeriesKind::Histogram2, color);
    series.histogram2 = Some(histogram2);
    axes.series.push(series);
    set_current_object_for_handle(state, series_handle);
    one_or_zero_outputs(
        Value::Scalar(series_handle as f64),
        output_arity,
        "histogram2",
    )
}

fn builtin_area(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    builtin_xy_series(
        state,
        args,
        output_arity,
        "area",
        SeriesKind::Area,
        true,
        false,
    )
}

fn builtin_stairs(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    builtin_xy_series(
        state,
        args,
        output_arity,
        "stairs",
        SeriesKind::Stairs,
        true,
        false,
    )
}

fn builtin_bar(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    builtin_xy_series(
        state,
        args,
        output_arity,
        "bar",
        SeriesKind::Bar,
        true,
        false,
    )
}

fn builtin_barh(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let allow_line_spec = false;
    let (series_groups, property_pairs) =
        parse_xy_series_args(args, "barh", allow_line_spec, true, false)?;
    if series_groups.is_empty() {
        return Err(RuntimeError::Unsupported(
            "barh currently requires at least one data point".to_string(),
        ));
    }

    {
        let axes = current_axes_mut(state);
        if !axes.hold_enabled {
            axes.series.clear();
            axes.legend = None;
        }
    }

    let mut series_handles = Vec::new();
    for group in series_groups {
        for input in group.series_inputs {
            if input.x.len() != input.y.len() || input.x.is_empty() {
                return Err(RuntimeError::ShapeError(
                    "barh requires matching nonempty position/value vectors".to_string(),
                ));
            }
            let series_handle = next_series_handle(state);
            {
                let axes = current_axes_mut(state);
                let color = SERIES_COLORS[axes.series.len() % SERIES_COLORS.len()];
                let mut series = make_series(series_handle, SeriesKind::BarHorizontal, color);
                series.x = input.y;
                series.y = input.x;
                apply_series_property_pairs(&mut series, property_pairs, "barh")?;
                axes.series.push(series);
            }
            series_handles.push(series_handle);
        }
    }

    if let Some(&series_handle) = series_handles.last() {
        set_current_object_for_handle(state, series_handle);
    }

    one_or_zero_outputs(
        series_handle_array_value(&series_handles)?,
        output_arity,
        "barh",
    )
}

fn builtin_stem(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    builtin_xy_series(
        state,
        args,
        output_arity,
        "stem",
        SeriesKind::Stem,
        true,
        false,
    )
}

fn builtin_stem3(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let allow_line_spec = true;
    let allow_property_pairs = true;
    let allow_multiple_groups = false;
    let (series_groups, property_pairs) = parse_xyz_series_args(
        args,
        "stem3",
        allow_line_spec,
        allow_property_pairs,
        allow_multiple_groups,
    )?;
    {
        let axes = current_axes_mut(state);
        if !axes.hold_enabled {
            axes.series.clear();
            axes.legend = None;
        }
    }

    let mut series_handles = Vec::new();
    for group in series_groups {
        let series_handle = next_series_handle(state);
        {
            let axes = current_axes_mut(state);
            let color = group
                .style
                .as_ref()
                .and_then(|spec| spec.color)
                .unwrap_or(SERIES_COLORS[axes.series.len() % SERIES_COLORS.len()]);
            let mut series = make_series(series_handle, SeriesKind::Stem3D, color);
            series.three_d = Some(stem3_series_from_points(group.three_d.points));
            series.marker = MarkerStyle::Circle;
            series.marker_size = 5.0;
            if let Some(style) = group.style.as_ref() {
                apply_line_spec_to_series(&mut series, style);
            }
            apply_series_property_pairs(&mut series, property_pairs, "stem3")?;
            axes.series.push(series);
        }
        series_handles.push(series_handle);
    }

    if let Some(&series_handle) = series_handles.last() {
        set_current_object_for_handle(state, series_handle);
    }

    one_or_zero_outputs(
        series_handle_array_value(&series_handles)?,
        output_arity,
        "stem3",
    )
}

fn builtin_contour(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let contour = parse_contour_args(args)?;
    let series_handle = next_series_handle(state);
    let axes = current_axes_mut(state);
    if !axes.hold_enabled {
        axes.series.clear();
        axes.legend = None;
    }
    let color = SERIES_COLORS[axes.series.len() % SERIES_COLORS.len()];
    let mut series = make_series(series_handle, SeriesKind::Contour, color);
    series.contour = Some(contour);
    axes.series.push(series);
    set_current_object_for_handle(state, series_handle);
    one_or_zero_outputs(Value::Scalar(series_handle as f64), output_arity, "contour")
}

fn builtin_contour3(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let contour = parse_contour_args(args)?;
    let series_handle = next_series_handle(state);
    let axes = current_axes_mut(state);
    if !axes.hold_enabled {
        axes.series.clear();
        axes.legend = None;
    }
    let color = SERIES_COLORS[axes.series.len() % SERIES_COLORS.len()];
    let mut series = make_series(series_handle, SeriesKind::Contour3, color);
    series.contour = Some(contour);
    axes.series.push(series);
    set_current_object_for_handle(state, series_handle);
    one_or_zero_outputs(
        Value::Scalar(series_handle as f64),
        output_arity,
        "contour3",
    )
}

fn builtin_contourf(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let contour_fill = parse_contourf_args(args)?;
    let series_handle = next_series_handle(state);
    let axes = current_axes_mut(state);
    if !axes.hold_enabled {
        axes.series.clear();
        axes.legend = None;
    }
    let mut series = make_series(series_handle, SeriesKind::ContourFill, "#000000");
    series.contour_fill = Some(contour_fill);
    axes.series.push(series);
    set_current_object_for_handle(state, series_handle);
    one_or_zero_outputs(
        Value::Scalar(series_handle as f64),
        output_arity,
        "contourf",
    )
}

fn builtin_surf(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let surface = parse_surface_args(args)?;
    let series_handle = next_series_handle(state);
    let axes = current_axes_mut(state);
    if !axes.hold_enabled {
        axes.series.clear();
        axes.legend = None;
    }
    let mut series = make_series(series_handle, SeriesKind::Surface, "#4c4c4c");
    series.surface = Some(surface);
    axes.series.push(series);
    set_current_object_for_handle(state, series_handle);
    one_or_zero_outputs(Value::Scalar(series_handle as f64), output_arity, "surf")
}

fn builtin_mesh(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let surface = parse_surface_args(args)?;
    let series_handle = next_series_handle(state);
    let axes = current_axes_mut(state);
    if !axes.hold_enabled {
        axes.series.clear();
        axes.legend = None;
    }
    let mut series = make_series(series_handle, SeriesKind::Mesh, "#3f3f3f");
    series.surface = Some(surface);
    axes.series.push(series);
    set_current_object_for_handle(state, series_handle);
    one_or_zero_outputs(Value::Scalar(series_handle as f64), output_arity, "mesh")
}

fn builtin_meshc(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let (surface, contour) = parse_surface_combo_args(args, "meshc")?;
    let mesh_handle = next_series_handle(state);
    let _contour_handle = next_series_handle(state);
    let axes = current_axes_mut(state);
    if !axes.hold_enabled {
        axes.series.clear();
        axes.legend = None;
    }
    let contour_color = SERIES_COLORS[axes.series.len() % SERIES_COLORS.len()];
    let mut contour_series = make_series(_contour_handle, SeriesKind::Contour, contour_color);
    contour_series.contour = Some(contour);
    axes.series.push(contour_series);
    let mut mesh_series = make_series(mesh_handle, SeriesKind::Mesh, "#3f3f3f");
    mesh_series.surface = Some(surface);
    axes.series.push(mesh_series);
    set_current_object_for_handle(state, mesh_handle);
    one_or_zero_outputs(Value::Scalar(mesh_handle as f64), output_arity, "meshc")
}

fn builtin_meshz(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let surface = parse_surface_args(args)?;
    let curtain = meshz_curtain_surface(&surface);
    let mesh_handle = next_series_handle(state);
    let curtain_handle = next_series_handle(state);
    let axes = current_axes_mut(state);
    if !axes.hold_enabled {
        axes.series.clear();
        axes.legend = None;
    }
    let mut mesh_series = make_series(mesh_handle, SeriesKind::Mesh, "#3f3f3f");
    mesh_series.surface = Some(surface);
    axes.series.push(mesh_series);
    let mut curtain_series = make_series(curtain_handle, SeriesKind::Surface, "#4c4c4c");
    curtain_series.surface = Some(curtain);
    axes.series.push(curtain_series);
    set_current_object_for_handle(state, mesh_handle);
    one_or_zero_outputs(Value::Scalar(mesh_handle as f64), output_arity, "meshz")
}

fn builtin_waterfall(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let surface = parse_surface_args(args)?;
    let series_handle = next_series_handle(state);
    let axes = current_axes_mut(state);
    if !axes.hold_enabled {
        axes.series.clear();
        axes.legend = None;
    }
    let mut series = make_series(series_handle, SeriesKind::Waterfall, "#1f77b4");
    series.surface = Some(surface);
    axes.series.push(series);
    set_current_object_for_handle(state, series_handle);
    one_or_zero_outputs(
        Value::Scalar(series_handle as f64),
        output_arity,
        "waterfall",
    )
}

fn builtin_ribbon(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let surface = ribbon_surface(args)?;
    let series_handle = next_series_handle(state);
    let axes = current_axes_mut(state);
    if !axes.hold_enabled {
        axes.series.clear();
        axes.legend = None;
    }
    let mut series = make_series(series_handle, SeriesKind::Ribbon, "#4c4c4c");
    series.surface = Some(surface);
    axes.series.push(series);
    set_current_object_for_handle(state, series_handle);
    one_or_zero_outputs(Value::Scalar(series_handle as f64), output_arity, "ribbon")
}

fn builtin_bar3(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let [z] = args else {
        return Err(RuntimeError::Unsupported(
            "bar3 currently supports exactly one numeric matrix or vector argument".to_string(),
        ));
    };
    let (rows, cols, values) = numeric_matrix(z, "bar3")?;
    let surface = bar3_surface(rows, cols, &values);
    let series_handle = next_series_handle(state);
    let axes = current_axes_mut(state);
    if !axes.hold_enabled {
        axes.series.clear();
        axes.legend = None;
    }
    let mut series = make_series(series_handle, SeriesKind::Surface, "#4c4c4c");
    series.surface = Some(surface);
    axes.series.push(series);
    set_current_object_for_handle(state, series_handle);
    one_or_zero_outputs(Value::Scalar(series_handle as f64), output_arity, "bar3")
}

fn builtin_bar3h(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let spec = parse_bar3h_args(args)?;
    let surface = bar3h_surface(&spec.z_positions, spec.rows, spec.cols, &spec.values);
    let series_handle = next_series_handle(state);
    let axes = current_axes_mut(state);
    if !axes.hold_enabled {
        axes.series.clear();
        axes.legend = None;
    }
    let mut series = make_series(series_handle, SeriesKind::Surface, "#4c4c4c");
    series.surface = Some(surface);
    axes.series.push(series);
    set_current_object_for_handle(state, series_handle);
    one_or_zero_outputs(Value::Scalar(series_handle as f64), output_arity, "bar3h")
}

fn builtin_surfc(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let (surface, contour) = parse_surface_combo_args(args, "surfc")?;
    let surface_handle = next_series_handle(state);
    let _contour_handle = next_series_handle(state);
    let axes = current_axes_mut(state);
    if !axes.hold_enabled {
        axes.series.clear();
        axes.legend = None;
    }
    let contour_color = SERIES_COLORS[axes.series.len() % SERIES_COLORS.len()];
    let mut contour_series = make_series(_contour_handle, SeriesKind::Contour, contour_color);
    contour_series.contour = Some(contour);
    axes.series.push(contour_series);
    let mut surface_series = make_series(surface_handle, SeriesKind::Surface, "#4c4c4c");
    surface_series.surface = Some(surface);
    axes.series.push(surface_series);
    set_current_object_for_handle(state, surface_handle);
    one_or_zero_outputs(Value::Scalar(surface_handle as f64), output_arity, "surfc")
}

fn builtin_image(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    builtin_image_series(state, args, output_arity, "image", ImageMode::Direct)
}

fn builtin_imagesc(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    builtin_image_series(state, args, output_arity, "imagesc", ImageMode::Scaled)
}

fn builtin_imshow(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let outputs = builtin_image_series(state, args, output_arity, "imshow", ImageMode::UnitRange)?;
    let axes = current_axes_mut(state);
    axes.axis_visible = false;
    axes.aspect_mode = AxisAspectMode::Equal;
    Ok(outputs)
}

fn builtin_text(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let text_data = parse_text_args(args)?;
    let series_handle = next_series_handle(state);
    let axes = current_axes_mut(state);
    let mut series = make_series(series_handle, SeriesKind::Text, "#222222");
    series.text = Some(text_data);
    axes.series.push(series);
    set_current_object_for_handle(state, series_handle);
    one_or_zero_outputs(Value::Scalar(series_handle as f64), output_arity, "text")
}

fn builtin_rectangle(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let spec = parse_rectangle_spec(args)?;
    let series_handle = next_series_handle(state);
    let axes = current_axes_mut(state);
    let mut series = make_series(
        series_handle,
        SeriesKind::Rectangle,
        spec.edge_color.unwrap_or("#1f77b4"),
    );
    series.line_width = spec.line_width;
    series.line_style = spec.line_style;
    series.visible = spec.visible;
    series.rectangle = Some(RectangleSeriesData {
        x: spec.position[0],
        y: spec.position[1],
        width: spec.position[2],
        height: spec.position[3],
        face_color: spec.face_color,
    });
    axes.series.push(series);
    set_current_object_for_handle(state, series_handle);
    one_or_zero_outputs(
        Value::Scalar(series_handle as f64),
        output_arity,
        "rectangle",
    )
}

fn builtin_patch(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let spec = parse_patch_spec(args, "patch", true)?;
    let series_handle = next_series_handle(state);
    let axes = current_axes_mut(state);
    let mut series = make_series(series_handle, SeriesKind::Patch, spec.edge_color);
    series.x = spec.x;
    series.y = spec.y;
    series.line_width = spec.line_width;
    series.line_style = spec.line_style;
    series.visible = spec.visible;
    series.display_name = spec.display_name;
    series.patch = Some(PatchSeriesData {
        face_color: spec.face_color,
    });
    axes.series.push(series);
    set_current_object_for_handle(state, series_handle);
    one_or_zero_outputs(Value::Scalar(series_handle as f64), output_arity, "patch")
}

fn builtin_fill(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let spec = parse_patch_spec(args, "fill", false)?;
    let series_handle = next_series_handle(state);
    let axes = current_axes_mut(state);
    let mut series = make_series(series_handle, SeriesKind::Patch, spec.edge_color);
    series.x = spec.x;
    series.y = spec.y;
    series.line_width = spec.line_width;
    series.line_style = spec.line_style;
    series.visible = spec.visible;
    series.display_name = spec.display_name;
    series.patch = Some(PatchSeriesData {
        face_color: spec.face_color,
    });
    axes.series.push(series);
    set_current_object_for_handle(state, series_handle);
    one_or_zero_outputs(Value::Scalar(series_handle as f64), output_arity, "fill")
}

fn builtin_fill3(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let spec = parse_fill3_spec(args)?;
    let series_handle = next_series_handle(state);
    let axes = current_axes_mut(state);
    let mut series = make_series(series_handle, SeriesKind::Patch, spec.edge_color);
    series.x = spec.x;
    series.y = spec.y;
    series.line_width = spec.line_width;
    series.line_style = spec.line_style;
    series.visible = spec.visible;
    series.display_name = spec.display_name;
    series.three_d = Some(three_d_series_from_points(spec.zipped_points));
    series.patch = Some(PatchSeriesData {
        face_color: spec.face_color,
    });
    axes.series.push(series);
    set_current_object_for_handle(state, series_handle);
    one_or_zero_outputs(Value::Scalar(series_handle as f64), output_arity, "fill3")
}

fn builtin_axes(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    match args {
        [] => {
            let handle = create_freeform_axes(state);
            set_current_object_for_handle(state, handle);
            one_or_zero_outputs(Value::Scalar(handle as f64), output_arity, "axes")
        }
        [requested] => {
            let handle = scalar_handle(requested, "axes")?;
            let (figure_handle, axes_index) =
                axes_location_by_handle(state, handle).ok_or_else(|| {
                    RuntimeError::MissingVariable(format!("axes handle `{handle}` does not exist"))
                })?;
            state.current_figure = Some(figure_handle);
            state
                .figures
                .get_mut(&figure_handle)
                .expect("figure should exist")
                .current_axes = axes_index;
            set_current_object_for_handle(state, handle);
            one_or_zero_outputs(Value::Scalar(handle as f64), output_arity, "axes")
        }
        _ => {
            if args.len() % 2 != 0 {
                return Err(RuntimeError::Unsupported(
                    "axes currently supports `axes()`, `axes(handle)`, or property/value pairs"
                        .to_string(),
                ));
            }
            let handle = create_freeform_axes(state);
            apply_graphics_property_pairs(state, handle, args)?;
            set_current_object_for_handle(state, handle);
            one_or_zero_outputs(Value::Scalar(handle as f64), output_arity, "axes")
        }
    }
}

fn builtin_axis(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let (target_axes, args) = leading_axes_handle_arg(state, args, "axis")?;
    if target_axes.is_none() {
        ensure_current_figure(state);
    }
    match args {
        [] => {
            let axes = target_axes_snapshot(state, target_axes)?;
            one_or_zero_outputs(axis_value(&axes)?, output_arity, "axis")
        }
        [mode] if is_text_keyword(mode, "tight")? => {
            let axes_handle = resolved_target_axes_handle(state, target_axes)?;
            let value = {
                let axes = target_axes_mut(state, target_axes)?;
                let three_d_range = axes_three_d_range(axes);
                let ((x_min, x_max), (y_min, y_max)) =
                    data_limits(axes, three_d_range.as_ref()).unwrap_or(((0.0, 1.0), (0.0, 1.0)));
                axes.xlim = Some((x_min, x_max));
                axes.ylim = Some((y_min, y_max));
                axis_value(axes)?
            };
            sync_linked_axes_for_handle(state, axes_handle, true, true)?;
            one_or_zero_outputs(value, output_arity, "axis")
        }
        [mode] if is_text_keyword(mode, "equal")? => {
            let axes = target_axes_mut(state, target_axes)?;
            axes.aspect_mode = AxisAspectMode::Equal;
            one_or_zero_outputs(axis_value(axes)?, output_arity, "axis")
        }
        [mode] if is_text_keyword(mode, "square")? => {
            let axes = target_axes_mut(state, target_axes)?;
            axes.aspect_mode = AxisAspectMode::Square;
            one_or_zero_outputs(axis_value(axes)?, output_arity, "axis")
        }
        [mode] if is_text_keyword(mode, "image")? => {
            let axes_handle = resolved_target_axes_handle(state, target_axes)?;
            let value = {
                let axes = target_axes_mut(state, target_axes)?;
                let three_d_range = axes_three_d_range(axes);
                let ((x_min, x_max), (y_min, y_max)) =
                    data_limits(axes, three_d_range.as_ref()).unwrap_or(((0.0, 1.0), (0.0, 1.0)));
                axes.xlim = Some((x_min, x_max));
                axes.ylim = Some((y_min, y_max));
                axes.aspect_mode = AxisAspectMode::Equal;
                axis_value(axes)?
            };
            sync_linked_axes_for_handle(state, axes_handle, true, true)?;
            one_or_zero_outputs(value, output_arity, "axis")
        }
        [mode] if is_text_keyword(mode, "normal")? => {
            let axes = target_axes_mut(state, target_axes)?;
            axes.aspect_mode = AxisAspectMode::Auto;
            one_or_zero_outputs(axis_value(axes)?, output_arity, "axis")
        }
        [mode] if is_text_keyword(mode, "auto")? => {
            let axes_handle = resolved_target_axes_handle(state, target_axes)?;
            let value = {
                let axes = target_axes_mut(state, target_axes)?;
                axes.xlim = None;
                axes.ylim = None;
                axis_value(axes)?
            };
            sync_linked_axes_for_handle(state, axes_handle, true, true)?;
            one_or_zero_outputs(value, output_arity, "axis")
        }
        [mode] if matches!(mode, Value::Logical(_) | Value::CharArray(_) | Value::String(_)) => {
            let axes = target_axes_mut(state, target_axes)?;
            axes.axis_visible = on_off_flag(mode, "axis")?;
            one_or_zero_outputs(axis_value(axes)?, output_arity, "axis")
        }
        [requested] => {
            let values = numeric_vector(requested, "axis")?;
            if values.len() != 4 {
                return Err(RuntimeError::ShapeError(
                    "axis currently expects a numeric vector with exactly four elements".to_string(),
                ));
            }
            let axes_handle = resolved_target_axes_handle(state, target_axes)?;
            let value = {
                let axes = target_axes_mut(state, target_axes)?;
                axes.xlim = Some((values[0], values[1]));
                axes.ylim = Some((values[2], values[3]));
                axis_value(axes)?
            };
            sync_linked_axes_for_handle(state, axes_handle, true, true)?;
            one_or_zero_outputs(value, output_arity, "axis")
        }
        _ => Err(RuntimeError::Unsupported(
            "axis currently supports `axis`, `axis(ax)`, a 1x4 numeric vector, or the text/logical modes `on`, `off`, `tight`, `auto`, `equal`, `square`, `image`, and `normal`, with optional leading axes handles"
                .to_string(),
        )),
    }
}

fn builtin_view(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let (target_axes, args) = leading_axes_handle_arg(state, args, "view")?;
    if target_axes.is_none() {
        ensure_current_figure(state);
    }
    match args {
        [] => {
            let axes = target_axes_snapshot(state, target_axes)?;
            one_or_zero_outputs(view_value(&axes)?, output_arity, "view")
        }
        [preset] if is_view_preset(preset, 2.0)? => {
            let axes = target_axes_mut(state, target_axes)?;
            axes.view_azimuth = 0.0;
            axes.view_elevation = 90.0;
            one_or_zero_outputs(view_value(axes)?, output_arity, "view")
        }
        [preset] if is_view_preset(preset, 3.0)? => {
            let axes = target_axes_mut(state, target_axes)?;
            axes.view_azimuth = -37.5;
            axes.view_elevation = 30.0;
            one_or_zero_outputs(view_value(axes)?, output_arity, "view")
        }
        [requested] => {
            let values = numeric_vector(requested, "view")?;
            if values.len() != 2 {
                return Err(RuntimeError::ShapeError(
                    "view currently expects a numeric vector with exactly two elements".to_string(),
                ));
            }
            let axes = target_axes_mut(state, target_axes)?;
            axes.view_azimuth = values[0];
            axes.view_elevation = values[1];
            one_or_zero_outputs(view_value(axes)?, output_arity, "view")
        }
        [azimuth, elevation] => {
            let axes = target_axes_mut(state, target_axes)?;
            axes.view_azimuth = azimuth.as_scalar()?;
            axes.view_elevation = elevation.as_scalar()?;
            one_or_zero_outputs(view_value(axes)?, output_arity, "view")
        }
        _ => Err(RuntimeError::Unsupported(
            "view currently supports `view`, `view(ax)`, preset scalars `2`/`3`, a numeric 1x2 vector, or two numeric scalar arguments, with optional leading axes handles"
                .to_string(),
        )),
    }
}

fn leading_axes_handle_arg<'a>(
    state: &GraphicsState,
    args: &'a [Value],
    builtin_name: &str,
) -> Result<(Option<u32>, &'a [Value]), RuntimeError> {
    let Some((first, rest)) = args.split_first() else {
        return Ok((None, args));
    };
    if let Ok(handle) = scalar_handle(first, builtin_name) {
        if axes_location_by_handle(state, handle).is_some() {
            return Ok((Some(handle), rest));
        }
    }
    Ok((None, args))
}

fn resolved_target_axes_handle(
    state: &mut GraphicsState,
    target_axes: Option<u32>,
) -> Result<u32, RuntimeError> {
    Ok(match target_axes {
        Some(handle) => handle,
        None => current_axes_handle(state),
    })
}

fn target_axes_snapshot(
    state: &mut GraphicsState,
    target_axes: Option<u32>,
) -> Result<AxesState, RuntimeError> {
    match target_axes {
        Some(handle) => Ok(axes_slot_by_handle(state, handle)?.axes.clone()),
        None => {
            ensure_current_figure(state);
            Ok(current_axes_snapshot(current_figure(state)))
        }
    }
}

fn target_axes_mut(
    state: &mut GraphicsState,
    target_axes: Option<u32>,
) -> Result<&mut AxesState, RuntimeError> {
    match target_axes {
        Some(handle) => Ok(&mut axes_slot_mut_by_handle(state, handle)?.axes),
        None => {
            ensure_current_figure(state);
            Ok(current_axes_mut(state))
        }
    }
}

fn leading_figure_handle_arg<'a>(
    state: &GraphicsState,
    args: &'a [Value],
    builtin_name: &str,
) -> Result<(Option<u32>, &'a [Value]), RuntimeError> {
    let Some((first, rest)) = args.split_first() else {
        return Ok((None, args));
    };
    if let Ok(handle) = scalar_handle(first, builtin_name) {
        if state.figures.contains_key(&handle) {
            return Ok((Some(handle), rest));
        }
    }
    Ok((None, args))
}

fn builtin_grid(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let (target_axes, enabled) = match args {
        [] => {
            ensure_current_figure(state);
            let enabled = !current_axes_snapshot(current_figure(state)).grid_enabled;
            (None, enabled)
        }
        [mode] if matches!(mode, Value::CharArray(_) | Value::String(_) | Value::Logical(_)) => {
            (None, on_off_flag(mode, "grid")?)
        }
        [axes] => {
            let handle = scalar_handle(axes, "grid")?;
            let enabled = !axes_slot_by_handle(state, handle)?.axes.grid_enabled;
            (Some(handle), enabled)
        }
        [axes, mode] => (Some(scalar_handle(axes, "grid")?), on_off_flag(mode, "grid")?),
        _ => {
            return Err(RuntimeError::Unsupported(
                "grid currently supports `grid`, `grid(state)`, `grid(ax)`, or `grid(ax, state)`"
                    .to_string(),
            ))
        }
    };

    let output_value = if let Some(handle) = target_axes {
        axes_slot_mut_by_handle(state, handle)?.axes.grid_enabled = enabled;
        Value::Scalar(handle as f64)
    } else {
        let figure_handle = ensure_current_figure(state);
        current_axes_mut(state).grid_enabled = enabled;
        Value::Scalar(figure_handle as f64)
    };
    one_or_zero_outputs(output_value, output_arity, "grid")
}

fn builtin_box(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let (target_axes, enabled) = match args {
        [] => {
            ensure_current_figure(state);
            let enabled = !current_axes_snapshot(current_figure(state)).box_enabled;
            (None, enabled)
        }
        [mode] if matches!(mode, Value::CharArray(_) | Value::String(_) | Value::Logical(_)) => {
            (None, on_off_flag(mode, "box")?)
        }
        [axes] => {
            let handle = scalar_handle(axes, "box")?;
            let enabled = !axes_slot_by_handle(state, handle)?.axes.box_enabled;
            (Some(handle), enabled)
        }
        [axes, mode] => (Some(scalar_handle(axes, "box")?), on_off_flag(mode, "box")?),
        _ => {
            return Err(RuntimeError::Unsupported(
                "box currently supports `box`, `box(state)`, `box(ax)`, or `box(ax, state)`"
                    .to_string(),
            ))
        }
    };

    let output_value = if let Some(handle) = target_axes {
        axes_slot_mut_by_handle(state, handle)?.axes.box_enabled = enabled;
        Value::Scalar(handle as f64)
    } else {
        let figure_handle = ensure_current_figure(state);
        current_axes_mut(state).box_enabled = enabled;
        Value::Scalar(figure_handle as f64)
    };
    one_or_zero_outputs(output_value, output_arity, "box")
}

fn builtin_axis_scale(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
    kind: ScaleKind,
) -> Result<Vec<Value>, RuntimeError> {
    let (target_axes, args) = leading_axes_handle_arg(state, args, kind.builtin_name())?;
    if target_axes.is_none() {
        ensure_current_figure(state);
    }
    if args.is_empty() {
        let axes = target_axes_snapshot(state, target_axes)?;
        let scale = match kind {
            ScaleKind::X => axes.x_scale,
            ScaleKind::Y => current_y_scale_for_side(&axes, axes.active_y_axis),
        };
        return one_or_zero_outputs(
            Value::CharArray(scale.as_text().to_string()),
            output_arity,
            kind.builtin_name(),
        );
    }

    let [requested] = args else {
        return Err(RuntimeError::Unsupported(format!(
            "{} currently supports zero arguments for query or one text scale mode, with an optional leading axes handle",
            kind.builtin_name()
        )));
    };
    let scale = parse_axis_scale(requested, kind.builtin_name())?;
    let axes = target_axes_mut(state, target_axes)?;
    match kind {
        ScaleKind::X => axes.x_scale = scale,
        ScaleKind::Y => *current_y_scale_mut(axes, axes.active_y_axis) = scale,
    }
    one_or_zero_outputs(
        Value::CharArray(scale.as_text().to_string()),
        output_arity,
        kind.builtin_name(),
    )
}

fn builtin_shading(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let (target_axes, args) = leading_axes_handle_arg(state, args, "shading")?;
    if target_axes.is_none() {
        ensure_current_figure(state);
    }
    if args.is_empty() {
        let axes = target_axes_snapshot(state, target_axes)?;
        return one_or_zero_outputs(
            Value::String(axes.shading_mode.as_text().to_string()),
            output_arity,
            "shading",
        );
    }

    let [requested] = args else {
        return Err(RuntimeError::Unsupported(
            "shading currently supports zero arguments for query or one text mode (`faceted`, `flat`, or `interp`), with an optional leading axes handle"
                .to_string(),
        ));
    };

    let mode = parse_shading_mode(requested)?;
    target_axes_mut(state, target_axes)?.shading_mode = mode;
    one_or_zero_outputs(
        Value::String(mode.as_text().to_string()),
        output_arity,
        "shading",
    )
}

fn builtin_caxis(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let (target_axes, args) = leading_axes_handle_arg(state, args, "caxis")?;
    if target_axes.is_none() {
        ensure_current_figure(state);
    }
    if args.is_empty() {
        let axes = target_axes_snapshot(state, target_axes)?;
        let (lower, upper) = effective_caxis(&axes);
        return one_or_zero_outputs(limit_value(lower, upper)?, output_arity, "caxis");
    }

    match args {
        [mode] if is_text_keyword(mode, "auto")? => {
            target_axes_mut(state, target_axes)?.caxis = None;
            let axes = target_axes_snapshot(state, target_axes)?;
            let (lower, upper) = effective_caxis(&axes);
            one_or_zero_outputs(limit_value(lower, upper)?, output_arity, "caxis")
        }
        [requested] => {
            let (lower, upper) = numeric_limit_pair(requested, "caxis")?;
            target_axes_mut(state, target_axes)?.caxis = Some((lower, upper));
            one_or_zero_outputs(limit_value(lower, upper)?, output_arity, "caxis")
        }
        _ => Err(RuntimeError::Unsupported(
            "caxis currently supports zero arguments, one numeric 1x2 vector, or the text mode `auto`, with an optional leading axes handle"
                .to_string(),
        )),
    }
}

fn builtin_xy_series(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
    builtin_name: &str,
    kind: SeriesKind,
    allow_matrix_series: bool,
    allow_multiple_groups: bool,
) -> Result<Vec<Value>, RuntimeError> {
    let allow_line_spec = matches!(kind, SeriesKind::Line);
    let (series_groups, property_pairs) = parse_xy_series_args(
        args,
        builtin_name,
        allow_line_spec,
        allow_matrix_series,
        allow_multiple_groups,
    )?;
    if series_groups.is_empty() {
        return Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently requires at least one data point"
        )));
    }

    {
        let axes = current_axes_mut(state);
        if !axes.hold_enabled {
            axes.series.clear();
            axes.legend = None;
        }
    }

    let mut series_handles = Vec::new();
    for group in series_groups {
        for input in group.series_inputs {
            if input.x.len() != input.y.len() {
                return Err(RuntimeError::ShapeError(format!(
                    "{builtin_name} requires x and y vectors with matching lengths, found {} and {}",
                    input.x.len(),
                    input.y.len()
                )));
            }
            if input.x.is_empty() {
                return Err(RuntimeError::Unsupported(format!(
                    "{builtin_name} currently requires at least one data point"
                )));
            }

            let series_handle = next_series_handle(state);
            {
                let axes = current_axes_mut(state);
                let color = group
                    .style
                    .as_ref()
                    .and_then(|spec| spec.color)
                    .unwrap_or(SERIES_COLORS[axes.series.len() % SERIES_COLORS.len()]);
                let mut series = make_series(series_handle, kind, color);
                series.y_axis_side = axes.active_y_axis;
                series.x = input.x;
                series.y = input.y;
                if let Some(style) = group.style.as_ref() {
                    apply_line_spec_to_series(&mut series, style);
                }
                apply_series_property_pairs(&mut series, property_pairs, builtin_name)?;
                axes.series.push(series);
            }
            series_handles.push(series_handle);
        }
    }

    if let Some(&series_handle) = series_handles.last() {
        set_current_object_for_handle(state, series_handle);
    }

    one_or_zero_outputs(
        series_handle_array_value(&series_handles)?,
        output_arity,
        builtin_name,
    )
}

fn builtin_xy_series_with_scales(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
    builtin_name: &str,
    x_scale: AxisScale,
    y_scale: Option<AxisScale>,
) -> Result<Vec<Value>, RuntimeError> {
    let allow_line_spec = true;
    let (series_groups, property_pairs) =
        parse_xy_series_args(args, builtin_name, allow_line_spec, true, true)?;
    if series_groups.is_empty() {
        return Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently requires at least one data point"
        )));
    }

    {
        let axes = current_axes_mut(state);
        if !axes.hold_enabled {
            axes.series.clear();
            axes.legend = None;
        }
        axes.x_scale = x_scale;
        if let Some(y_scale) = y_scale {
            *current_y_scale_mut(axes, axes.active_y_axis) = y_scale;
        }
    }

    let mut series_handles = Vec::new();
    for group in series_groups {
        for input in group.series_inputs {
            if input.x.len() != input.y.len() {
                return Err(RuntimeError::ShapeError(format!(
                    "{builtin_name} requires x and y vectors with matching lengths, found {} and {}",
                    input.x.len(),
                    input.y.len()
                )));
            }
            if input.x.is_empty() {
                return Err(RuntimeError::Unsupported(format!(
                    "{builtin_name} currently requires at least one data point"
                )));
            }
            if x_scale == AxisScale::Log
                && input
                    .x
                    .iter()
                    .any(|value| !value.is_finite() || *value <= 0.0)
            {
                return Err(RuntimeError::TypeError(format!(
                    "{builtin_name} currently expects positive finite x values for log-scaled axes"
                )));
            }
            if y_scale == Some(AxisScale::Log)
                && input
                    .y
                    .iter()
                    .any(|value| !value.is_finite() || *value <= 0.0)
            {
                return Err(RuntimeError::TypeError(format!(
                    "{builtin_name} currently expects positive finite y values for log-scaled axes"
                )));
            }

            let series_handle = next_series_handle(state);
            {
                let axes = current_axes_mut(state);
                let color = group
                    .style
                    .as_ref()
                    .and_then(|spec| spec.color)
                    .unwrap_or(SERIES_COLORS[axes.series.len() % SERIES_COLORS.len()]);
                let mut series = make_series(series_handle, SeriesKind::Line, color);
                series.y_axis_side = axes.active_y_axis;
                series.x = input.x;
                series.y = input.y;
                if let Some(style) = group.style.as_ref() {
                    apply_line_spec_to_series(&mut series, style);
                }
                apply_series_property_pairs(&mut series, property_pairs, builtin_name)?;
                axes.series.push(series);
            }
            series_handles.push(series_handle);
        }
    }

    if let Some(&series_handle) = series_handles.last() {
        set_current_object_for_handle(state, series_handle);
    }

    one_or_zero_outputs(
        series_handle_array_value(&series_handles)?,
        output_arity,
        builtin_name,
    )
}

fn builtin_xyz_series(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
    builtin_name: &str,
    kind: SeriesKind,
    allow_property_pairs: bool,
    allow_multiple_groups: bool,
) -> Result<Vec<Value>, RuntimeError> {
    let allow_line_spec = matches!(kind, SeriesKind::Line3D);
    let (series_groups, property_pairs) = parse_xyz_series_args(
        args,
        builtin_name,
        allow_line_spec,
        allow_property_pairs,
        allow_multiple_groups,
    )?;
    {
        let axes = current_axes_mut(state);
        if !axes.hold_enabled {
            axes.series.clear();
            axes.legend = None;
        }
    }

    let mut series_handles = Vec::new();
    for group in series_groups {
        let series_handle = next_series_handle(state);
        {
            let axes = current_axes_mut(state);
            let color = group
                .style
                .as_ref()
                .and_then(|spec| spec.color)
                .unwrap_or(SERIES_COLORS[axes.series.len() % SERIES_COLORS.len()]);
            let mut series = make_series(series_handle, kind, color);
            series.y_axis_side = axes.active_y_axis;
            series.three_d = Some(group.three_d);
            if let Some(style) = group.style.as_ref() {
                apply_line_spec_to_series(&mut series, style);
            }
            apply_series_property_pairs(&mut series, property_pairs, builtin_name)?;
            axes.series.push(series);
        }
        series_handles.push(series_handle);
    }

    if let Some(&series_handle) = series_handles.last() {
        set_current_object_for_handle(state, series_handle);
    }

    one_or_zero_outputs(
        series_handle_array_value(&series_handles)?,
        output_arity,
        builtin_name,
    )
}

fn builtin_image_series(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
    builtin_name: &str,
    mode: ImageMode,
) -> Result<Vec<Value>, RuntimeError> {
    let image = parse_image_matrix_args(args, builtin_name, mode)?;
    let series_handle = next_series_handle(state);
    let axes = current_axes_mut(state);
    if !axes.hold_enabled {
        axes.series.clear();
        axes.legend = None;
    }
    let mut series = make_series(series_handle, SeriesKind::Image, "#000000");
    series.image = Some(image);
    axes.series.push(series);
    set_current_object_for_handle(state, series_handle);
    one_or_zero_outputs(
        Value::Scalar(series_handle as f64),
        output_arity,
        builtin_name,
    )
}

fn builtin_colormap(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let (target_axes, args) = leading_axes_handle_arg(state, args, "colormap")?;
    if target_axes.is_none() {
        ensure_current_figure(state);
    }
    let kind =
        match args {
            [] => target_axes_snapshot(state, target_axes)?.colormap,
            [name] => parse_colormap_kind(name)?,
            _ => return Err(RuntimeError::Unsupported(
                "colormap currently supports zero arguments for query or one named colormap string, with an optional leading axes handle"
                    .to_string(),
            )),
        };

    if !args.is_empty() {
        target_axes_mut(state, target_axes)?.colormap = kind;
    }

    colormap_outputs(kind, output_arity)
}

fn builtin_colorbar(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let (target_axes, enabled) = match args {
        [] => (None, true),
        [mode] if matches!(mode, Value::CharArray(_) | Value::String(_) | Value::Logical(_)) => {
            (None, on_off_flag(mode, "colorbar")?)
        }
        [axes] => (Some(scalar_handle(axes, "colorbar")?), true),
        [axes, mode] => (
            Some(scalar_handle(axes, "colorbar")?),
            on_off_flag(mode, "colorbar")?,
        ),
        _ => {
            return Err(RuntimeError::Unsupported(
                "colorbar currently supports `colorbar`, `colorbar(state)`, `colorbar(ax)`, or `colorbar(ax, state)`"
                    .to_string(),
            ))
        }
    };
    let output_value = if let Some(handle) = target_axes {
        axes_slot_mut_by_handle(state, handle)?.axes.colorbar_enabled = enabled;
        Value::Scalar(handle as f64)
    } else {
        let figure_handle = ensure_current_figure(state);
        current_axes_mut(state).colorbar_enabled = enabled;
        Value::Scalar(figure_handle as f64)
    };
    one_or_zero_outputs(
        output_value,
        output_arity,
        "colorbar",
    )
}

fn builtin_legend(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let (target_axes, args) = leading_axes_handle_arg(state, args, "legend")?;
    let output_handle = match target_axes {
        Some(handle) => handle,
        None => ensure_current_figure(state),
    };

    let axes_snapshot = target_axes_snapshot(state, target_axes)?;
    let legend_spec = parse_legend_spec(args, &axes_snapshot)?;

    let axes = target_axes_mut(state, target_axes)?;
    if let Some(location) = legend_spec.location {
        axes.legend_location = location;
    }
    if let Some(orientation) = legend_spec.orientation {
        axes.legend_orientation = orientation;
    }

    if let Some(labels) = legend_spec.labels {
        if axes.series.is_empty() {
            return Err(RuntimeError::Unsupported(
                "legend currently requires at least one plotted series".to_string(),
            ));
        }
        if labels.len() != axes.series.len() {
            return Err(RuntimeError::ShapeError(format!(
                "legend currently requires exactly one label per plotted series, found {} labels for {} series",
                labels.len(),
                axes.series.len()
            )));
        }
        axes.legend = Some(labels);
    } else {
        axes.legend = None;
    }

    one_or_zero_outputs(Value::Scalar(output_handle as f64), output_arity, "legend")
}

fn builtin_sgtitle(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let (target_figure, args) = leading_figure_handle_arg(state, args, "sgtitle")?;
    let [text] = args else {
        return Err(RuntimeError::Unsupported(
            "sgtitle currently supports exactly one text argument, with an optional leading figure handle"
                .to_string(),
        ));
    };

    let label = text_arg(text, "sgtitle")?;
    let figure_handle = target_figure.unwrap_or_else(|| ensure_current_figure(state));
    state
        .figures
        .get_mut(&figure_handle)
        .expect("current figure should exist")
        .super_title = label;
    one_or_zero_outputs(Value::Scalar(figure_handle as f64), output_arity, "sgtitle")
}

fn builtin_label(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
    kind: LabelKind,
) -> Result<Vec<Value>, RuntimeError> {
    let (target_axes, args) = leading_axes_handle_arg(state, args, kind.builtin_name())?;
    let [text] = args else {
        return Err(RuntimeError::Unsupported(format!(
            "{} currently supports exactly one text argument, with an optional leading axes handle",
            kind.builtin_name()
        )));
    };

    let label = text_arg(text, kind.builtin_name())?;
    let output_handle = match target_axes {
        Some(handle) => handle,
        None => ensure_current_figure(state),
    };
    let axes = target_axes_mut(state, target_axes)?;
    match kind {
        LabelKind::Title => axes.title = label,
        LabelKind::Subtitle => axes.subtitle = label,
        LabelKind::XLabel => axes.xlabel = label,
        LabelKind::YLabel => match axes.active_y_axis {
            YAxisSide::Left => axes.ylabel = label,
            YAxisSide::Right => axes.ylabel_right = label,
        },
        LabelKind::ZLabel => axes.zlabel = label,
    }
    one_or_zero_outputs(
        Value::Scalar(output_handle as f64),
        output_arity,
        kind.builtin_name(),
    )
}

fn builtin_yyaxis(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    if output_arity > 0 {
        return Err(RuntimeError::Unsupported(
            "yyaxis currently does not return outputs".to_string(),
        ));
    }
    let (target_axes, args) = leading_axes_handle_arg(state, args, "yyaxis")?;
    let [side] = args else {
        return Err(RuntimeError::Unsupported(
            "yyaxis currently supports exactly one text argument: `left` or `right`, with an optional leading axes handle"
                .to_string(),
        ));
    };
    let side = match text_arg(side, "yyaxis")?.to_ascii_lowercase().as_str() {
        "left" => YAxisSide::Left,
        "right" => YAxisSide::Right,
        other => {
            return Err(RuntimeError::Unsupported(format!(
                "yyaxis currently supports only `left` or `right`, found `{other}`"
            )))
        }
    };
    target_axes_mut(state, target_axes)?.active_y_axis = side;
    Ok(Vec::new())
}

fn builtin_ticks(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
    kind: TickKind,
) -> Result<Vec<Value>, RuntimeError> {
    let (target_axes, args) = leading_axes_handle_arg(state, args, kind.builtin_name())?;
    if target_axes.is_none() {
        ensure_current_figure(state);
    }
    if args.is_empty() {
        let axes = target_axes_snapshot(state, target_axes)?;
        return one_or_zero_outputs(
            tick_values_value(&resolved_ticks_active_side(&axes, kind))?,
            output_arity,
            kind.builtin_name(),
        );
    }

    match args {
        [mode] if is_text_keyword(mode, "auto")? => {
            let axes = target_axes_mut(state, target_axes)?;
            match kind {
                TickKind::X => {
                    axes.xticks = None;
                    let tick_count = resolved_ticks(axes, kind).len();
                    sync_tick_label_override(axes, kind, tick_count);
                }
                TickKind::Y => {
                    *current_y_ticks_mut(axes, axes.active_y_axis) = None;
                    let tick_count = resolved_ticks_for_side(axes, kind, axes.active_y_axis).len();
                    sync_tick_label_override(axes, kind, tick_count);
                }
                TickKind::Z => {
                    axes.zticks = None;
                    let tick_count = resolved_ticks(axes, kind).len();
                    sync_tick_label_override(axes, kind, tick_count);
                }
            }
            let axes = target_axes_snapshot(state, target_axes)?;
            one_or_zero_outputs(
                tick_values_value(&resolved_ticks_active_side(&axes, kind))?,
                output_arity,
                kind.builtin_name(),
            )
        }
        [requested] => {
            let ticks = tick_vector(requested, kind.builtin_name())?;
            let axes = target_axes_mut(state, target_axes)?;
            match kind {
                TickKind::X => axes.xticks = Some(ticks.clone()),
                TickKind::Y => *current_y_ticks_mut(axes, axes.active_y_axis) = Some(ticks.clone()),
                TickKind::Z => axes.zticks = Some(ticks.clone()),
            }
            sync_tick_label_override(axes, kind, ticks.len());
            one_or_zero_outputs(tick_values_value(&ticks)?, output_arity, kind.builtin_name())
        }
        _ => Err(RuntimeError::Unsupported(format!(
            "{} currently supports zero arguments for query, one numeric vector, or the text mode `auto`, with an optional leading axes handle",
            kind.builtin_name()
        ))),
    }
}

fn builtin_rotate3d(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let figure_handle = ensure_current_figure(state);
    let (target_figure, mode) = match args {
        [] => (figure_handle, None),
        [mode] => {
            let text = text_arg(mode, "rotate3d")?;
            (figure_handle, Some(text))
        }
        [handle, mode] => {
            let handle = scalar_handle(handle, "rotate3d")?;
            let text = text_arg(mode, "rotate3d")?;
            (handle, Some(text))
        }
        _ => {
            return Err(RuntimeError::Unsupported(
                "rotate3d currently supports no arguments, `rotate3d on|off|toggle`, or `rotate3d(fig, on|off|toggle)`"
                    .to_string(),
            ))
        }
    };
    let figure = state.figures.get_mut(&target_figure).ok_or_else(|| {
        RuntimeError::MissingVariable(format!("figure handle `{target_figure}` does not exist"))
    })?;
    let enabled = match mode
        .as_deref()
        .map(|text| text.to_ascii_lowercase())
        .as_deref()
    {
        None | Some("toggle") => {
            figure.rotate3d_enabled = !figure.rotate3d_enabled;
            figure.rotate3d_enabled
        }
        Some("on") => {
            figure.rotate3d_enabled = true;
            true
        }
        Some("off") => {
            figure.rotate3d_enabled = false;
            false
        }
        Some(other) => {
            return Err(RuntimeError::Unsupported(format!(
                "rotate3d currently supports only `on`, `off`, or `toggle`, found `{other}`"
            )))
        }
    };
    one_or_zero_outputs(on_off_value(enabled), output_arity, "rotate3d")
}

fn builtin_linkaxes(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let (inputs, mode) = match args {
        [handles] => (graphics_handle_inputs(handles, "linkaxes")?, Some(LinkAxesMode::XY)),
        [handles, mode] if is_text_keyword(mode, "off")? => {
            (graphics_handle_inputs(handles, "linkaxes")?, None)
        }
        [handles, mode] => (
            graphics_handle_inputs(handles, "linkaxes")?,
            Some(parse_linkaxes_mode(mode)?),
        ),
        _ => {
            return Err(RuntimeError::Unsupported(
                "linkaxes currently supports `linkaxes(ax)`, `linkaxes(ax, 'x')`, `linkaxes(ax, 'y')`, `linkaxes(ax, 'xy')`, or `linkaxes(ax, 'off')`"
                    .to_string(),
            ))
        }
    };

    let mut handles = inputs.handles.clone();
    handles.sort_unstable();
    handles.dedup();
    if handles.is_empty() {
        return one_or_zero_outputs(
            graphics_handle_inputs_value(&inputs)?,
            output_arity,
            "linkaxes",
        );
    }

    let (figure_handle, _) = axes_location_by_handle(state, handles[0]).ok_or_else(|| {
        RuntimeError::TypeError(format!(
            "linkaxes currently expects axes handles, found graphics handle `{}` that is not an axes handle",
            handles[0]
        ))
    })?;
    for handle in &handles[1..] {
        let (candidate_figure, _) = axes_location_by_handle(state, *handle).ok_or_else(|| {
            RuntimeError::TypeError(format!(
                "linkaxes currently expects axes handles, found graphics handle `{handle}` that is not an axes handle"
            ))
        })?;
        if candidate_figure != figure_handle {
            return Err(RuntimeError::Unsupported(
                "linkaxes currently expects all axes handles to belong to the same figure"
                    .to_string(),
            ));
        }
    }

    {
        let figure = state
            .figures
            .get_mut(&figure_handle)
            .expect("figure should exist");
        unlink_axes_handles(figure, &handles);
        if let Some(mode) = mode {
            if handles.len() >= 2 {
                figure.linked_axes.push(LinkedAxesGroup {
                    handles: handles.clone(),
                    mode,
                });
            }
        }
    }

    if let Some(mode) = mode {
        apply_linkaxes_mode(state, figure_handle, &handles, mode)?;
    }

    one_or_zero_outputs(
        graphics_handle_inputs_value(&inputs)?,
        output_arity,
        "linkaxes",
    )
}

fn builtin_tick_labels(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
    kind: TickKind,
) -> Result<Vec<Value>, RuntimeError> {
    let (target_axes, args) = leading_axes_handle_arg(state, args, kind.labels_builtin_name())?;
    if target_axes.is_none() {
        ensure_current_figure(state);
    }
    if args.is_empty() {
        let axes = target_axes_snapshot(state, target_axes)?;
        return one_or_zero_outputs(
            tick_labels_value(&resolved_tick_labels_active_side(&axes, kind))?,
            output_arity,
            kind.labels_builtin_name(),
        );
    }

    match args {
        [mode] if is_text_keyword(mode, "auto")? => {
            let axes = target_axes_mut(state, target_axes)?;
            match kind {
                TickKind::X => axes.xtick_labels = None,
                TickKind::Y => *current_y_tick_labels_mut(axes, axes.active_y_axis) = None,
                TickKind::Z => axes.ztick_labels = None,
            }
            let axes = target_axes_snapshot(state, target_axes)?;
            one_or_zero_outputs(
                tick_labels_value(&resolved_tick_labels_active_side(&axes, kind))?,
                output_arity,
                kind.labels_builtin_name(),
            )
        }
        [requested] => {
            let labels = text_labels_from_value(requested, kind.labels_builtin_name())?;
            let axes_snapshot = target_axes_snapshot(state, target_axes)?;
            let tick_count = resolved_ticks_active_side(&axes_snapshot, kind).len();
            if !labels.is_empty() && labels.len() != tick_count {
                return Err(RuntimeError::ShapeError(format!(
                    "{} currently expects either an empty text vector or exactly {} labels to match the current tick count",
                    kind.labels_builtin_name(),
                    tick_count
                )));
            }

            let axes = target_axes_mut(state, target_axes)?;
            match kind {
                TickKind::X => axes.xtick_labels = Some(labels.clone()),
                TickKind::Y => *current_y_tick_labels_mut(axes, axes.active_y_axis) = Some(labels.clone()),
                TickKind::Z => axes.ztick_labels = Some(labels.clone()),
            }
            one_or_zero_outputs(
                tick_labels_value(&labels)?,
                output_arity,
                kind.labels_builtin_name(),
            )
        }
        _ => Err(RuntimeError::Unsupported(format!(
            "{} currently supports zero arguments for query, one text vector/cell array, or the text mode `auto`, with an optional leading axes handle",
            kind.labels_builtin_name()
        ))),
    }
}

fn builtin_tick_angle(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
    kind: TickKind,
) -> Result<Vec<Value>, RuntimeError> {
    let (target_axes, args) = leading_axes_handle_arg(state, args, kind.angle_builtin_name())?;
    if target_axes.is_none() {
        ensure_current_figure(state);
    }
    if args.is_empty() {
        let axes = target_axes_snapshot(state, target_axes)?;
        return one_or_zero_outputs(
            Value::Scalar(resolved_tick_angle_active_side(&axes, kind)),
            output_arity,
            kind.angle_builtin_name(),
        );
    }

    let [requested] = args else {
        return Err(RuntimeError::Unsupported(format!(
            "{} currently supports zero arguments for query or one numeric scalar angle, with an optional leading axes handle",
            kind.angle_builtin_name()
        )));
    };

    let angle = finite_scalar_arg(requested, kind.angle_builtin_name())?;
    let axes = target_axes_mut(state, target_axes)?;
    match kind {
        TickKind::X => axes.xtick_angle = angle,
        TickKind::Y => *current_y_tick_angle_mut(axes, axes.active_y_axis) = angle,
        TickKind::Z => axes.ztick_angle = angle,
    }
    one_or_zero_outputs(
        Value::Scalar(angle),
        output_arity,
        kind.angle_builtin_name(),
    )
}

fn builtin_limits(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
    kind: LimitKind,
) -> Result<Vec<Value>, RuntimeError> {
    let (target_axes, args) = leading_axes_handle_arg(state, args, kind.builtin_name())?;
    if target_axes.is_none() {
        ensure_current_figure(state);
    }
    if args.is_empty() {
        let axes = target_axes_snapshot(state, target_axes)?;
        let (lower, upper) = match kind {
            LimitKind::X => resolved_limits(&axes).0,
            LimitKind::Y => resolved_y_limits_for_side(&axes, axes.active_y_axis),
            LimitKind::Z => resolved_z_limits(&axes),
        };
        return one_or_zero_outputs(
            limit_value(lower, upper)?,
            output_arity,
            kind.builtin_name(),
        );
    }

    let [requested] = args else {
        return Err(RuntimeError::Unsupported(format!(
            "{} currently supports zero arguments for query or one numeric 1x2 vector, with an optional leading axes handle",
            kind.builtin_name()
        )));
    };
    let (lower, upper) = numeric_limit_pair(requested, kind.builtin_name())?;
    let axes_handle = resolved_target_axes_handle(state, target_axes)?;
    {
        let axes = target_axes_mut(state, target_axes)?;
        match kind {
            LimitKind::X => axes.xlim = Some((lower, upper)),
            LimitKind::Y => *current_y_limit_mut(axes, axes.active_y_axis) = Some((lower, upper)),
            LimitKind::Z => axes.zlim = Some((lower, upper)),
        }
    }
    match kind {
        LimitKind::X => sync_linked_axes_for_handle(state, axes_handle, true, false)?,
        LimitKind::Y => sync_linked_axes_for_handle(state, axes_handle, false, true)?,
        LimitKind::Z => {}
    }
    one_or_zero_outputs(
        limit_value(lower, upper)?,
        output_arity,
        kind.builtin_name(),
    )
}

fn builtin_saveas(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let spec = parse_export_arguments(state, args, "saveas")?;
    export_figure(state, &spec, output_arity, "saveas")
}

fn builtin_exportgraphics(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let spec = parse_export_arguments(state, args, "exportgraphics")?;
    export_figure(state, &spec, output_arity, "exportgraphics")
}

fn builtin_print(
    state: &mut GraphicsState,
    args: &[Value],
    output_arity: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let spec = parse_print_arguments(state, args)?;
    export_figure(state, &spec, output_arity, "print")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportFormat {
    Svg,
    Png,
    Pdf,
}

impl ExportFormat {
    fn extension(self) -> &'static str {
        match self {
            Self::Svg => "svg",
            Self::Png => "png",
            Self::Pdf => "pdf",
        }
    }

    fn parse(text: &str, builtin_name: &str) -> Result<Self, RuntimeError> {
        let normalized = text.trim();
        let normalized = normalized.strip_prefix('-').unwrap_or(normalized);
        let normalized = normalized.strip_prefix('d').unwrap_or(normalized);
        match normalized.to_ascii_lowercase().as_str() {
            "svg" => Ok(Self::Svg),
            "png" => Ok(Self::Png),
            "pdf" => Ok(Self::Pdf),
            other => Err(RuntimeError::Unsupported(format!(
                "{builtin_name} currently supports only SVG, PNG, or PDF output, found `{other}`"
            ))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportResizeMode {
    None,
    BestFit,
    FillPage,
}

#[derive(Debug, Clone)]
struct ExportSpec {
    handle: u32,
    path: PathBuf,
    format: ExportFormat,
    resolution_dpi: Option<u32>,
    append_extension_if_missing: bool,
    resize_mode: ExportResizeMode,
}

#[derive(Debug, Clone, Copy)]
struct ExportCanvasSize {
    width_px: u32,
    height_px: u32,
}

#[derive(Debug, Clone, Copy)]
struct PdfContentRect {
    x_pt: f32,
    y_pt: f32,
    width_pt: f32,
    height_pt: f32,
}

#[derive(Debug, Clone, Copy)]
struct PdfExportLayout {
    page_width_pt: f32,
    page_height_pt: f32,
    content_rect: PdfContentRect,
}

fn parse_export_arguments(
    state: &mut GraphicsState,
    args: &[Value],
    builtin_name: &str,
) -> Result<ExportSpec, RuntimeError> {
    let (handle, path, explicit_format) = match args {
        [path] => (
            ensure_current_figure(state),
            PathBuf::from(text_arg(path, builtin_name)?),
            None,
        ),
        [handle, path] => (
            scalar_handle(handle, builtin_name)?,
            PathBuf::from(text_arg(path, builtin_name)?),
            None,
        ),
        [handle, path, format] => (
            scalar_handle(handle, builtin_name)?,
            PathBuf::from(text_arg(path, builtin_name)?),
            Some(text_arg(format, builtin_name)?),
        ),
        _ => {
            return Err(RuntimeError::Unsupported(format!(
                "{builtin_name} currently supports `(path)`, `(figure, path)`, or `(figure, path, format)`"
            )))
        }
    };

    let format = resolve_export_format(&path, explicit_format.as_deref(), builtin_name)?;
    Ok(ExportSpec {
        handle,
        path,
        format,
        resolution_dpi: None,
        append_extension_if_missing: explicit_format.is_some(),
        resize_mode: ExportResizeMode::None,
    })
}

fn parse_subplot_args(args: &[Value]) -> Result<(usize, usize, usize), RuntimeError> {
    let (rows, cols, index) = match args {
        [packed] => {
            let packed = scalar_usize(packed, "subplot")?;
            if !(111..=999).contains(&packed) {
                return Err(RuntimeError::Unsupported(
                    "subplot currently supports compact scalar forms like `subplot(211)`"
                        .to_string(),
                ));
            }
            (packed / 100, (packed / 10) % 10, packed % 10)
        }
        [rows, cols, index] => (
            scalar_usize(rows, "subplot")?,
            scalar_usize(cols, "subplot")?,
            scalar_usize(index, "subplot")?,
        ),
        _ => {
            return Err(RuntimeError::Unsupported(
                "subplot currently supports `subplot(m, n, p)` or compact `subplot(mnp)` forms"
                    .to_string(),
            ))
        }
    };

    if rows == 0 || cols == 0 || index == 0 || index > rows * cols {
        return Err(RuntimeError::ShapeError(format!(
            "subplot requires positive layout counts and an index within 1..={}, found ({rows}, {cols}, {index})",
            rows * cols
        )));
    }
    Ok((rows, cols, index))
}

fn parse_tiledlayout_args(args: &[Value]) -> Result<(usize, usize), RuntimeError> {
    let (rows, cols) = match args {
        [rows, cols] => (
            scalar_usize(rows, "tiledlayout")?,
            scalar_usize(cols, "tiledlayout")?,
        ),
        _ => {
            return Err(RuntimeError::Unsupported(
                "tiledlayout currently supports exactly two positive scalar layout arguments"
                    .to_string(),
            ))
        }
    };
    if rows == 0 || cols == 0 {
        return Err(RuntimeError::ShapeError(format!(
            "tiledlayout requires positive layout counts, found ({rows}, {cols})"
        )));
    }
    Ok((rows, cols))
}

fn next_tile_index(figure: &FigureState) -> usize {
    let max_tiles = figure.layout_rows.max(1) * figure.layout_cols.max(1);
    for index in 1..=max_tiles {
        if !figure.axes.contains_key(&index) {
            return index;
        }
    }
    let current = figure.current_axes.max(1);
    if current >= max_tiles {
        1
    } else {
        current + 1
    }
}

fn parse_print_arguments(
    state: &mut GraphicsState,
    args: &[Value],
) -> Result<ExportSpec, RuntimeError> {
    let (mut handle, raw_args) = match args {
        [handle, rest @ ..] if matches!(handle, Value::Scalar(_)) => {
            (Some(scalar_handle(handle, "print")?), rest)
        }
        _ => (None, args),
    };

    if raw_args.is_empty() {
        return Err(RuntimeError::Unsupported(
            "print currently requires a filename and a file-output device like `-dpng` or `-dsvg`"
                .to_string(),
        ));
    }

    let mut path = None;
    let mut format = None;
    let mut resolution_dpi = None;
    let mut resize_mode = ExportResizeMode::None;
    for value in raw_args {
        let text = text_arg(value, "print")?;
        let normalized = text.trim();
        let lower = normalized.to_ascii_lowercase();
        match lower.as_str() {
            "-clipboard" => {
                return Err(RuntimeError::Unsupported(
                    "print clipboard targets are not supported in the current export subset"
                        .to_string(),
                ))
            }
            "-bestfit" => {
                if resize_mode != ExportResizeMode::None {
                    return Err(RuntimeError::Unsupported(
                        "print currently supports at most one page-resize flag".to_string(),
                    ));
                }
                resize_mode = ExportResizeMode::BestFit;
            }
            "-fillpage" => {
                if resize_mode != ExportResizeMode::None {
                    return Err(RuntimeError::Unsupported(
                        "print currently supports at most one page-resize flag".to_string(),
                    ));
                }
                resize_mode = ExportResizeMode::FillPage;
            }
            "-loose" => {
                return Err(RuntimeError::Unsupported(format!(
                    "print page-layout flag `{normalized}` is not supported in the current export subset"
                )))
            }
            "-image" | "-vector" | "-rgbimage" | "-painters" | "-opengl" | "-zbuffer" => {
                return Err(RuntimeError::Unsupported(format!(
                    "print renderer/content flag `{normalized}` is not supported in the current export subset"
                )))
            }
            _ if lower.starts_with("-f") => {
                let parsed = parse_print_handle_flag(normalized)?;
                if handle.replace(parsed).is_some() {
                    return Err(RuntimeError::Unsupported(
                        "print currently supports only one explicit figure target".to_string(),
                    ));
                }
            }
            _ if lower.starts_with("-p") => {
                return Err(RuntimeError::Unsupported(
                    "print physical printer targets are not supported in the current export subset"
                        .to_string(),
                ))
            }
            _ if lower.starts_with("-d") => {
                let parsed = ExportFormat::parse(normalized, "print")?;
                if format.replace(parsed).is_some() {
                    return Err(RuntimeError::Unsupported(
                        "print currently supports only one `-d...` device flag".to_string(),
                    ));
                }
            }
            _ if lower.starts_with("-r") => {
                if resolution_dpi.is_some() {
                    return Err(RuntimeError::Unsupported(
                        "print currently supports only one `-r...` resolution flag".to_string(),
                    ));
                }
                resolution_dpi = Some(parse_print_resolution_flag(normalized)?);
            }
            _ if lower.starts_with('-') => {
                return Err(RuntimeError::Unsupported(format!(
                    "print flag `{normalized}` is not supported in the current export subset"
                )))
            }
            _ => {
                if path.replace(PathBuf::from(normalized)).is_some() {
                    return Err(RuntimeError::Unsupported(
                        "print currently supports exactly one output filename".to_string(),
                    ));
                }
            }
        }
    }

    let format = format.ok_or_else(|| {
        RuntimeError::Unsupported(
            "print currently requires a file-output device like `-dpng` or `-dsvg`".to_string(),
        )
    })?;
    if resize_mode != ExportResizeMode::None && format != ExportFormat::Pdf {
        return Err(RuntimeError::Unsupported(
            "print currently supports `-bestfit` and `-fillpage` only for PDF output".to_string(),
        ));
    }
    let path = path.ok_or_else(|| {
        RuntimeError::Unsupported("print currently requires an output filename".to_string())
    })?;

    Ok(ExportSpec {
        handle: handle.unwrap_or_else(|| ensure_current_figure(state)),
        path,
        format,
        resolution_dpi,
        append_extension_if_missing: true,
        resize_mode,
    })
}

fn parse_print_handle_flag(flag: &str) -> Result<u32, RuntimeError> {
    let digits = flag.trim().strip_prefix("-f").unwrap_or_default();
    if digits.is_empty() {
        return Err(RuntimeError::Unsupported(
            "print currently expects figure flags like `-f2`".to_string(),
        ));
    }
    let handle = digits.parse::<u32>().map_err(|_| {
        RuntimeError::Unsupported(format!(
            "print currently expects figure flags like `-f2`, found `{flag}`"
        ))
    })?;
    if handle == 0 {
        return Err(RuntimeError::Unsupported(
            "print currently requires positive figure handles in `-fN` flags".to_string(),
        ));
    }
    Ok(handle)
}

fn parse_print_resolution_flag(flag: &str) -> Result<u32, RuntimeError> {
    let digits = flag.trim().strip_prefix("-r").unwrap_or_default();
    if digits.is_empty() {
        return Err(RuntimeError::Unsupported(
            "print currently expects `-rN` resolution flags like `-r300`".to_string(),
        ));
    }
    let dpi = digits.parse::<u32>().map_err(|_| {
        RuntimeError::Unsupported(format!(
            "print currently expects integer resolution flags like `-r300`, found `{flag}`"
        ))
    })?;
    Ok(if dpi == 0 { EXPORT_BASE_DPI } else { dpi })
}

fn resolve_export_format(
    path: &Path,
    explicit_format: Option<&str>,
    builtin_name: &str,
) -> Result<ExportFormat, RuntimeError> {
    if let Some(format) = explicit_format {
        return ExportFormat::parse(format, builtin_name);
    }
    match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if !extension.is_empty() => ExportFormat::parse(extension, builtin_name),
        _ => Ok(ExportFormat::Svg),
    }
}

fn path_has_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| !extension.is_empty())
}

fn resolved_export_path(spec: &ExportSpec) -> PathBuf {
    let mut resolved = spec.path.clone();
    if spec.append_extension_if_missing && !path_has_extension(&resolved) {
        resolved.set_extension(spec.format.extension());
    }
    resolved
}

fn export_figure(
    state: &GraphicsState,
    spec: &ExportSpec,
    output_arity: usize,
    builtin_name: &str,
) -> Result<Vec<Value>, RuntimeError> {
    let figure = state.figures.get(&spec.handle).ok_or_else(|| {
        RuntimeError::MissingVariable(format!("figure handle `{}` does not exist", spec.handle))
    })?;
    let path = resolved_export_path(spec);

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).map_err(|error| {
                RuntimeError::Unsupported(format!(
                    "failed to create graphics output directory `{}`: {error}",
                    parent.display()
                ))
            })?;
        }
    }

    let dpi = spec.resolution_dpi.unwrap_or(EXPORT_BASE_DPI);
    let canvas = resolved_export_canvas_size(figure, spec, dpi)?;
    match spec.format {
        ExportFormat::Svg => {
            fs::write(
                &path,
                render_figure_svg_with_size(
                    figure,
                    canvas.width_px as f64,
                    canvas.height_px as f64,
                ),
            )
            .map_err(|error| {
                RuntimeError::Unsupported(format!(
                    "failed to write graphics output `{}`: {error}",
                    path.display()
                ))
            })?;
        }
        ExportFormat::Png => write_png(figure, &path, canvas)?,
        ExportFormat::Pdf => write_pdf(
            figure,
            &path,
            canvas,
            resolved_pdf_export_layout(figure, spec)?,
        )?,
    }

    one_or_zero_outputs(
        Value::Scalar(spec.handle as f64),
        output_arity,
        builtin_name,
    )
}

fn write_png(
    figure: &FigureState,
    path: &Path,
    canvas: ExportCanvasSize,
) -> Result<(), RuntimeError> {
    let pixmap = render_export_pixmap(figure, canvas)?;
    pixmap.save_png(path).map_err(|error| {
        RuntimeError::Unsupported(format!(
            "failed to write graphics output `{}`: {error}",
            path.display()
        ))
    })
}

fn write_pdf(
    figure: &FigureState,
    path: &Path,
    canvas: ExportCanvasSize,
    layout: PdfExportLayout,
) -> Result<(), RuntimeError> {
    let pixmap = render_export_pixmap(figure, canvas)?;
    let rgb = pixmap_to_pdf_rgb(&pixmap);
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&rgb).map_err(|error| {
        RuntimeError::Unsupported(format!(
            "failed to prepare PDF raster data for `{}`: {error}",
            path.display()
        ))
    })?;
    let compressed = encoder.finish().map_err(|error| {
        RuntimeError::Unsupported(format!(
            "failed to finalize PDF raster data for `{}`: {error}",
            path.display()
        ))
    })?;

    let catalog_id = Ref::new(1);
    let page_tree_id = Ref::new(2);
    let page_id = Ref::new(3);
    let image_id = Ref::new(4);
    let content_id = Ref::new(5);

    let mut pdf = Pdf::new();
    pdf.catalog(catalog_id).pages(page_tree_id);
    pdf.pages(page_tree_id).kids([page_id]).count(1);
    {
        let mut page = pdf.page(page_id);
        page.parent(page_tree_id)
            .media_box(Rect::new(
                0.0,
                0.0,
                layout.page_width_pt,
                layout.page_height_pt,
            ))
            .contents(content_id);
        page.resources()
            .x_objects()
            .pair(Name(b"Im1"), image_id)
            .finish();
        page.finish();
    }
    {
        let mut image = pdf.image_xobject(image_id, &compressed);
        image.filter(Filter::FlateDecode);
        image.width(canvas.width_px as i32);
        image.height(canvas.height_px as i32);
        image.color_space().device_rgb();
        image.bits_per_component(8);
        image.finish();
    }
    let mut content = Content::new();
    content.save_state();
    content.transform([
        layout.content_rect.width_pt,
        0.0,
        0.0,
        layout.content_rect.height_pt,
        layout.content_rect.x_pt,
        layout.content_rect.y_pt,
    ]);
    content.x_object(Name(b"Im1"));
    content.restore_state();
    let stream = content.finish();
    pdf.stream(content_id, &stream).finish();

    fs::write(path, pdf.finish()).map_err(|error| {
        RuntimeError::Unsupported(format!(
            "failed to write graphics output `{}`: {error}",
            path.display()
        ))
    })
}

fn render_export_pixmap(
    figure: &FigureState,
    canvas: ExportCanvasSize,
) -> Result<tiny_skia::Pixmap, RuntimeError> {
    let svg = render_figure_svg_with_size(figure, canvas.width_px as f64, canvas.height_px as f64);
    let mut options = usvg::Options::default();
    options.fontdb_mut().load_system_fonts();
    let tree = usvg::Tree::from_str(&svg, &options).map_err(|error| {
        RuntimeError::Unsupported(format!(
            "failed to parse rendered SVG for raster graphics export: {error}"
        ))
    })?;
    let mut pixmap =
        tiny_skia::Pixmap::new(canvas.width_px, canvas.height_px).ok_or_else(|| {
            RuntimeError::Unsupported(format!(
                "failed to allocate raster graphics export surface with size {}x{}",
                canvas.width_px, canvas.height_px
            ))
        })?;
    resvg::render(
        &tree,
        tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    Ok(pixmap)
}

fn pixmap_to_pdf_rgb(pixmap: &tiny_skia::Pixmap) -> Vec<u8> {
    let mut rgb = Vec::with_capacity((pixmap.width() * pixmap.height() * 3) as usize);
    for pixel in pixmap.data().chunks_exact(4) {
        let alpha = pixel[3] as f64 / 255.0;
        let red = ((pixel[0] as f64) * alpha + 255.0 * (1.0 - alpha)).round() as u8;
        let green = ((pixel[1] as f64) * alpha + 255.0 * (1.0 - alpha)).round() as u8;
        let blue = ((pixel[2] as f64) * alpha + 255.0 * (1.0 - alpha)).round() as u8;
        rgb.extend([red, green, blue]);
    }
    rgb
}

fn resolved_export_canvas_size(
    figure: &FigureState,
    spec: &ExportSpec,
    dpi: u32,
) -> Result<ExportCanvasSize, RuntimeError> {
    let [_, _, width_in, height_in] = match spec.resize_mode {
        ExportResizeMode::None => resolved_export_size_position_in(figure),
        ExportResizeMode::BestFit => best_fit_paper_position_in(figure.paper_size_in),
        ExportResizeMode::FillPage => fill_page_paper_position_in(figure.paper_size_in),
    };
    if !width_in.is_finite() || !height_in.is_finite() || width_in <= 0.0 || height_in <= 0.0 {
        return Err(RuntimeError::Unsupported(
            "graphics export currently requires a positive finite paper position size".to_string(),
        ));
    }
    Ok(ExportCanvasSize {
        width_px: (width_in * dpi as f64).round().max(1.0) as u32,
        height_px: (height_in * dpi as f64).round().max(1.0) as u32,
    })
}

fn resolved_pdf_export_layout(
    figure: &FigureState,
    spec: &ExportSpec,
) -> Result<PdfExportLayout, RuntimeError> {
    let page_width_in = figure.paper_size_in[0];
    let page_height_in = figure.paper_size_in[1];
    if page_width_in <= 0.0 || page_height_in <= 0.0 {
        return Err(RuntimeError::Unsupported(
            "graphics export currently requires a positive finite paper size".to_string(),
        ));
    }
    let [x_in, y_in, width_in, height_in] = match spec.resize_mode {
        ExportResizeMode::None => resolved_export_size_position_in(figure),
        ExportResizeMode::BestFit => best_fit_paper_position_in(figure.paper_size_in),
        ExportResizeMode::FillPage => fill_page_paper_position_in(figure.paper_size_in),
    };
    if width_in <= 0.0 || height_in <= 0.0 {
        return Err(RuntimeError::Unsupported(
            "graphics export currently requires a positive finite paper position size".to_string(),
        ));
    }
    Ok(PdfExportLayout {
        page_width_pt: (page_width_in * 72.0) as f32,
        page_height_pt: (page_height_in * 72.0) as f32,
        content_rect: PdfContentRect {
            x_pt: (x_in * 72.0) as f32,
            y_pt: (y_in * 72.0) as f32,
            width_pt: (width_in * 72.0) as f32,
            height_pt: (height_in * 72.0) as f32,
        },
    })
}

fn displayed_figure_size_in() -> [f64; 2] {
    [
        DEFAULT_RENDER_WIDTH / EXPORT_BASE_DPI as f64,
        DEFAULT_RENDER_HEIGHT / EXPORT_BASE_DPI as f64,
    ]
}

fn standard_paper_size_in(paper_type: PaperType, orientation: PaperOrientation) -> [f64; 2] {
    let base = match paper_type {
        PaperType::UsLetter => [8.5, 11.0],
        PaperType::UsLegal => [8.5, 14.0],
        PaperType::Tabloid => [11.0, 17.0],
        PaperType::A3 => [11.692913385826772, 16.53543307086614],
        PaperType::A4 => [8.267716535433072, 11.692913385826772],
        PaperType::Custom => return displayed_figure_size_in(),
    };
    match orientation {
        PaperOrientation::Portrait => base,
        PaperOrientation::Landscape => [base[1], base[0]],
    }
}

fn default_auto_paper_position_in(page_size_in: [f64; 2]) -> [f64; 4] {
    let [figure_width_in, figure_height_in] = displayed_figure_size_in();
    [
        (page_size_in[0] - figure_width_in) / 2.0,
        (page_size_in[1] - figure_height_in) / 2.0,
        figure_width_in,
        figure_height_in,
    ]
}

fn resolved_export_size_position_in(figure: &FigureState) -> [f64; 4] {
    match figure.paper_position_mode {
        PaperPositionMode::Auto => default_auto_paper_position_in(figure.paper_size_in),
        PaperPositionMode::Manual => figure.paper_position_in,
    }
}

fn best_fit_paper_position_in(page_size_in: [f64; 2]) -> [f64; 4] {
    let [figure_width_in, figure_height_in] = displayed_figure_size_in();
    let available_width = (page_size_in[0] - 2.0 * PDF_MARGIN_INCHES).max(0.0);
    let available_height = (page_size_in[1] - 2.0 * PDF_MARGIN_INCHES).max(0.0);
    let scale = if figure_width_in <= 0.0 || figure_height_in <= 0.0 {
        1.0
    } else {
        (available_width / figure_width_in)
            .min(available_height / figure_height_in)
            .max(0.0)
    };
    let width = figure_width_in * scale;
    let height = figure_height_in * scale;
    [
        PDF_MARGIN_INCHES + (available_width - width) / 2.0,
        PDF_MARGIN_INCHES + (available_height - height) / 2.0,
        width,
        height,
    ]
}

fn fill_page_paper_position_in(page_size_in: [f64; 2]) -> [f64; 4] {
    [
        PDF_MARGIN_INCHES,
        PDF_MARGIN_INCHES,
        (page_size_in[0] - 2.0 * PDF_MARGIN_INCHES).max(0.0),
        (page_size_in[1] - 2.0 * PDF_MARGIN_INCHES).max(0.0),
    ]
}

struct FigureCreationRequest {
    handle: Option<u32>,
    property_pairs: Vec<Value>,
}

pub(crate) struct CloseRequest {
    pub(crate) handles: Vec<u32>,
    pub(crate) status_if_empty: bool,
}

fn parse_figure_creation_request(args: &[Value]) -> Result<FigureCreationRequest, RuntimeError> {
    if args.is_empty() {
        return Ok(FigureCreationRequest {
            handle: None,
            property_pairs: Vec::new(),
        });
    }

    let (handle, property_pairs) = match args.first() {
        Some(Value::CharArray(_)) | Some(Value::String(_)) => {
            if args.len() % 2 != 0 {
                return Err(RuntimeError::Unsupported(
                    "figure currently expects property/value pairs after figure property names"
                        .to_string(),
                ));
            }
            (None, args.to_vec())
        }
        Some(_) => {
            let requested_handle = scalar_handle(&args[0], "figure")?;
            if args.len() == 1 {
                (Some(requested_handle), Vec::new())
            } else {
                if (args.len() - 1) % 2 != 0 {
                    return Err(RuntimeError::Unsupported(
                        "figure currently expects property/value pairs after the optional figure handle"
                            .to_string(),
                    ));
                }
                (Some(requested_handle), args[1..].to_vec())
            }
        }
        None => unreachable!("empty argument slice handled above"),
    };

    Ok(FigureCreationRequest {
        handle,
        property_pairs,
    })
}

pub(crate) fn close_request_handles(
    state: &GraphicsState,
    args: &[Value],
    builtin_name: &str,
) -> Result<CloseRequest, RuntimeError> {
    match args {
        [] => Ok(CloseRequest {
            handles: state.current_figure.into_iter().collect(),
            status_if_empty: false,
        }),
        [requested] if is_text_keyword(requested, "all")? => Ok(CloseRequest {
            handles: state.figures.keys().copied().collect(),
            status_if_empty: true,
        }),
        [requested] => {
            let handles = graphics_handle_inputs(requested, builtin_name)?;
            for handle in &handles.handles {
                match graphics_handle_kind(state, *handle).ok_or_else(|| {
                    RuntimeError::MissingVariable(format!("graphics handle `{handle}` does not exist"))
                })? {
                    GraphicsHandleKind::Figure => {}
                    _ => {
                        return Err(RuntimeError::Unsupported(format!(
                            "{builtin_name} currently supports figure handles or the text argument `all`"
                        )))
                    }
                }
            }
            Ok(CloseRequest {
                handles: handles.handles,
                status_if_empty: false,
            })
        }
        _ => Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently supports no arguments, one figure handle or figure-handle array, or the text argument `all`"
        ))),
    }
}

fn create_new_figure(state: &mut GraphicsState) -> u32 {
    let mut handle = state.next_auto_figure_handle.max(1);
    while graphics_handle_in_use(state, handle) {
        handle += 1;
    }
    state.next_auto_figure_handle = handle + 1;
    state.figures.insert(handle, FigureState::default());
    handle
}

fn select_or_create_figure(state: &mut GraphicsState, handle: u32) -> u32 {
    if graphics_handle_in_use(state, handle) && !state.figures.contains_key(&handle) {
        panic!("attempted to create figure with non-figure graphics handle");
    }
    state
        .figures
        .entry(handle)
        .or_insert_with(FigureState::default);
    if handle >= state.next_auto_figure_handle {
        state.next_auto_figure_handle = handle + 1;
    }
    handle
}

fn ensure_current_figure(state: &mut GraphicsState) -> u32 {
    match state.current_figure {
        Some(handle) if state.figures.contains_key(&handle) => handle,
        _ => {
            let handle = create_new_figure(state);
            state.current_figure = Some(handle);
            handle
        }
    }
}

fn next_axes_handle(state: &mut GraphicsState) -> u32 {
    let mut handle = state.next_axes_handle.max(1001);
    while graphics_handle_in_use(state, handle) {
        handle += 1;
    }
    state.next_axes_handle = handle + 1;
    handle
}

fn current_figure(state: &GraphicsState) -> &FigureState {
    let handle = state
        .current_figure
        .expect("current figure should exist after ensure_current_figure");
    state
        .figures
        .get(&handle)
        .expect("current figure should exist")
}

fn current_axes_snapshot(figure: &FigureState) -> AxesState {
    figure
        .axes
        .get(&figure.current_axes)
        .map(|slot| slot.axes.clone())
        .unwrap_or_default()
}

fn current_axes_handle(state: &mut GraphicsState) -> u32 {
    let figure_handle = ensure_current_figure(state);
    let index = state
        .figures
        .get(&figure_handle)
        .expect("current figure should exist")
        .current_axes
        .max(1);
    ensure_axes_slot(state, figure_handle, index)
}

fn current_axes_mut(state: &mut GraphicsState) -> &mut AxesState {
    let figure_handle = ensure_current_figure(state);
    let index = state
        .figures
        .get(&figure_handle)
        .expect("current figure should exist")
        .current_axes
        .max(1);
    ensure_axes_slot(state, figure_handle, index);
    &mut state
        .figures
        .get_mut(&figure_handle)
        .expect("current figure should exist")
        .axes
        .get_mut(&index)
        .expect("current axes should exist")
        .axes
}

fn create_freeform_axes(state: &mut GraphicsState) -> u32 {
    let figure_handle = ensure_current_figure(state);
    let next_index = state
        .figures
        .get(&figure_handle)
        .expect("figure should exist")
        .axes
        .keys()
        .copied()
        .max()
        .unwrap_or(0)
        + 1;
    let handle = ensure_axes_slot(state, figure_handle, next_index);
    state.current_figure = Some(figure_handle);
    let figure = state
        .figures
        .get_mut(&figure_handle)
        .expect("figure should exist");
    figure.current_axes = next_index;
    figure
        .axes
        .get_mut(&next_index)
        .expect("axes should exist")
        .axes
        .position
        .get_or_insert(default_axes_position());
    handle
}

fn ensure_axes_slot(state: &mut GraphicsState, figure_handle: u32, index: usize) -> u32 {
    let index = index.max(1);
    if let Some(handle) = state
        .figures
        .get(&figure_handle)
        .and_then(|figure| figure.axes.get(&index))
        .map(|slot| slot.handle)
    {
        return handle;
    }

    let handle = next_axes_handle(state);
    state
        .figures
        .get_mut(&figure_handle)
        .expect("figure should exist")
        .axes
        .insert(
            index,
            AxesSlot {
                handle,
                axes: AxesState::default(),
            },
        );
    handle
}

fn default_axes_position() -> [f64; 4] {
    [0.13, 0.11, 0.775, 0.815]
}

fn figure_current_axes_handle(figure: &FigureState) -> Option<u32> {
    figure
        .axes
        .get(&figure.current_axes)
        .map(|slot| slot.handle)
}

fn axes_index_in_figure(figure: &FigureState, handle: u32) -> Option<usize> {
    figure
        .axes
        .iter()
        .find_map(|(index, slot)| (slot.handle == handle).then_some(*index))
}

fn axes_location_by_handle(state: &GraphicsState, handle: u32) -> Option<(u32, usize)> {
    state.figures.iter().find_map(|(figure_handle, figure)| {
        figure
            .axes
            .iter()
            .find_map(|(index, slot)| (slot.handle == handle).then_some((*figure_handle, *index)))
    })
}

fn series_location_by_handle(state: &GraphicsState, handle: u32) -> Option<(u32, usize, usize)> {
    state.figures.iter().find_map(|(figure_handle, figure)| {
        figure.axes.iter().find_map(|(axes_index, slot)| {
            slot.axes
                .series
                .iter()
                .enumerate()
                .find_map(|(series_index, series)| {
                    (series.handle == handle).then_some((*figure_handle, *axes_index, series_index))
                })
        })
    })
}

fn axes_slot_by_handle(state: &GraphicsState, handle: u32) -> Result<&AxesSlot, RuntimeError> {
    let (figure_handle, index) = axes_location_by_handle(state, handle).ok_or_else(|| {
        RuntimeError::MissingVariable(format!("axes handle `{handle}` does not exist"))
    })?;
    Ok(state
        .figures
        .get(&figure_handle)
        .expect("figure should exist")
        .axes
        .get(&index)
        .expect("axes should exist"))
}

fn axes_slot_mut_by_handle(
    state: &mut GraphicsState,
    handle: u32,
) -> Result<&mut AxesSlot, RuntimeError> {
    let (figure_handle, index) = axes_location_by_handle(state, handle).ok_or_else(|| {
        RuntimeError::MissingVariable(format!("axes handle `{handle}` does not exist"))
    })?;
    Ok(state
        .figures
        .get_mut(&figure_handle)
        .expect("figure should exist")
        .axes
        .get_mut(&index)
        .expect("axes should exist"))
}

fn next_series_handle(state: &mut GraphicsState) -> u32 {
    let mut handle = state.next_series_handle.max(2001);
    while graphics_handle_in_use(state, handle) {
        handle += 1;
    }
    state.next_series_handle = handle + 1;
    handle
}

fn next_annotation_handle(state: &mut GraphicsState) -> u32 {
    let mut handle = state.next_annotation_handle.max(3001);
    while graphics_handle_in_use(state, handle) {
        handle += 1;
    }
    state.next_annotation_handle = handle + 1;
    handle
}

fn series_by_handle(state: &GraphicsState, handle: u32) -> Result<&PlotSeries, RuntimeError> {
    let (figure_handle, axes_index, series_index) = series_location_by_handle(state, handle)
        .ok_or_else(|| {
            RuntimeError::MissingVariable(format!(
                "graphics series handle `{handle}` does not exist"
            ))
        })?;
    Ok(&state
        .figures
        .get(&figure_handle)
        .expect("figure should exist")
        .axes
        .get(&axes_index)
        .expect("axes should exist")
        .axes
        .series[series_index])
}

fn series_mut_by_handle(
    state: &mut GraphicsState,
    handle: u32,
) -> Result<&mut PlotSeries, RuntimeError> {
    let (figure_handle, axes_index, series_index) = series_location_by_handle(state, handle)
        .ok_or_else(|| {
            RuntimeError::MissingVariable(format!(
                "graphics series handle `{handle}` does not exist"
            ))
        })?;
    Ok(&mut state
        .figures
        .get_mut(&figure_handle)
        .expect("figure should exist")
        .axes
        .get_mut(&axes_index)
        .expect("axes should exist")
        .axes
        .series[series_index])
}

fn annotation_location_by_handle(state: &GraphicsState, handle: u32) -> Option<(u32, usize)> {
    state.figures.iter().find_map(|(figure_handle, figure)| {
        figure
            .annotations
            .iter()
            .enumerate()
            .find_map(|(index, annotation)| {
                (annotation.handle == handle).then_some((*figure_handle, index))
            })
    })
}

fn annotation_by_handle(
    state: &GraphicsState,
    handle: u32,
) -> Result<&AnnotationObject, RuntimeError> {
    let (figure_handle, annotation_index) = annotation_location_by_handle(state, handle)
        .ok_or_else(|| {
            RuntimeError::MissingVariable(format!(
                "graphics annotation handle `{handle}` does not exist"
            ))
        })?;
    Ok(&state
        .figures
        .get(&figure_handle)
        .expect("figure should exist")
        .annotations[annotation_index])
}

fn annotation_mut_by_handle(
    state: &mut GraphicsState,
    handle: u32,
) -> Result<&mut AnnotationObject, RuntimeError> {
    let (figure_handle, annotation_index) = annotation_location_by_handle(state, handle)
        .ok_or_else(|| {
            RuntimeError::MissingVariable(format!(
                "graphics annotation handle `{handle}` does not exist"
            ))
        })?;
    Ok(&mut state
        .figures
        .get_mut(&figure_handle)
        .expect("figure should exist")
        .annotations[annotation_index])
}

fn graphics_handle_in_use(state: &GraphicsState, handle: u32) -> bool {
    state.figures.contains_key(&handle)
        || axes_location_by_handle(state, handle).is_some()
        || series_location_by_handle(state, handle).is_some()
        || annotation_location_by_handle(state, handle).is_some()
}

fn delete_priority(kind: GraphicsHandleKind) -> u8 {
    match kind {
        GraphicsHandleKind::Series => 0,
        GraphicsHandleKind::Annotation => 0,
        GraphicsHandleKind::Axes => 1,
        GraphicsHandleKind::Figure => 2,
    }
}

fn delete_graphics_handle(state: &mut GraphicsState, handle: u32) -> Result<(), RuntimeError> {
    match graphics_handle_kind(state, handle).ok_or_else(|| {
        RuntimeError::MissingVariable(format!("graphics handle `{handle}` does not exist"))
    })? {
        GraphicsHandleKind::Series => {
            let (figure_handle, axes_index, series_index) =
                series_location_by_handle(state, handle).expect("series handle should exist");
            let figure = state
                .figures
                .get_mut(&figure_handle)
                .expect("figure should exist");
            if figure.current_object == Some(handle) {
                figure.current_object = None;
            }
            let axes = &mut figure
                .axes
                .get_mut(&axes_index)
                .expect("axes should exist")
                .axes;
            axes.series.remove(series_index);
            axes.legend = None;
        }
        GraphicsHandleKind::Annotation => {
            let (figure_handle, annotation_index) = annotation_location_by_handle(state, handle)
                .expect("annotation handle should exist");
            let figure = state
                .figures
                .get_mut(&figure_handle)
                .expect("figure should exist");
            if figure.current_object == Some(handle) {
                figure.current_object = None;
            }
            figure.annotations.remove(annotation_index);
        }
        GraphicsHandleKind::Axes => {
            let (figure_handle, axes_index) =
                axes_location_by_handle(state, handle).expect("axes handle should exist");
            let figure = state
                .figures
                .get_mut(&figure_handle)
                .expect("figure should exist");
            if figure.current_object == Some(handle)
                || figure.current_object.is_some_and(|object| {
                    figure
                        .axes
                        .get(&axes_index)
                        .map(|slot| {
                            slot.axes
                                .series
                                .iter()
                                .any(|series| series.handle == object)
                        })
                        .unwrap_or(false)
                })
            {
                figure.current_object = None;
            }
            figure.axes.remove(&axes_index);
            unlink_axes_handles(figure, &[handle]);
            if figure.current_axes == axes_index {
                figure.current_axes = figure.axes.keys().copied().next().unwrap_or(1);
            }
        }
        GraphicsHandleKind::Figure => {
            state.figures.remove(&handle);
            if state.current_figure == Some(handle) {
                state.current_figure = state.figures.keys().copied().next();
            }
        }
    }

    Ok(())
}

fn reset_graphics_handle(state: &mut GraphicsState, handle: u32) -> Result<(), RuntimeError> {
    match graphics_handle_kind(state, handle).ok_or_else(|| {
        RuntimeError::MissingVariable(format!("graphics handle `{handle}` does not exist"))
    })? {
        GraphicsHandleKind::Figure => Err(RuntimeError::Unsupported(
            "reset currently supports axes and series handles, not figure handles".to_string(),
        )),
        GraphicsHandleKind::Axes => {
            let slot = axes_slot_mut_by_handle(state, handle)?;
            let preserved = slot.axes.clone();
            let mut reset = AxesState::default();
            reset.position = preserved.position;
            reset.series = preserved.series;
            slot.axes = reset;
            Ok(())
        }
        GraphicsHandleKind::Annotation => {
            let annotation = annotation_mut_by_handle(state, handle)?;
            let preserved = annotation.clone();
            let mut reset = default_annotation_object(handle, preserved.kind);
            reset.x = preserved.x;
            reset.y = preserved.y;
            reset.position = preserved.position;
            reset.text = preserved.text;
            *annotation = reset;
            Ok(())
        }
        GraphicsHandleKind::Series => {
            let series = series_mut_by_handle(state, handle)?;
            let preserved = series.clone();
            let mut reset =
                make_series(handle, preserved.kind, default_series_color(preserved.kind));
            reset.x = preserved.x;
            reset.y = preserved.y;
            reset.quiver = preserved.quiver;
            reset.histogram = preserved.histogram;
            reset.histogram2 = preserved.histogram2;
            reset.pie = preserved.pie;
            reset.image = preserved.image;
            reset.contour = preserved.contour;
            reset.contour_fill = preserved.contour_fill;
            reset.surface = preserved.surface;
            reset.three_d = preserved.three_d;
            reset.text = preserved.text;
            reset.rectangle = preserved.rectangle;
            reset.patch = preserved.patch;
            *series = reset;
            Ok(())
        }
    }
}

fn default_series_color(kind: SeriesKind) -> &'static str {
    match kind {
        SeriesKind::Pie
        | SeriesKind::Pie3
        | SeriesKind::ContourFill
        | SeriesKind::Image
        | SeriesKind::Text => "#000000",
        SeriesKind::Mesh => "#3f3f3f",
        SeriesKind::Surface => "#4c4c4c",
        _ => SERIES_COLORS[0],
    }
}

fn copy_graphics_handle(
    state: &mut GraphicsState,
    source_handle: u32,
    target_handle: u32,
    target_kind: GraphicsHandleKind,
) -> Result<u32, RuntimeError> {
    let source_kind = graphics_handle_kind(state, source_handle).ok_or_else(|| {
        RuntimeError::MissingVariable(format!("graphics handle `{source_handle}` does not exist"))
    })?;
    match (source_kind, target_kind) {
        (GraphicsHandleKind::Series, GraphicsHandleKind::Axes) => {
            clone_series_into_axes(state, source_handle, target_handle)
        }
        (GraphicsHandleKind::Axes, GraphicsHandleKind::Figure) => {
            clone_axes_into_figure(state, source_handle, target_handle)
        }
        _ => Err(RuntimeError::Unsupported(
            "copyobj currently supports copying series into axes and axes into figures".to_string(),
        )),
    }
}

fn clone_series_into_axes(
    state: &mut GraphicsState,
    source_handle: u32,
    target_axes_handle: u32,
) -> Result<u32, RuntimeError> {
    let mut series = series_by_handle(state, source_handle)?.clone();
    let new_handle = next_series_handle(state);
    series.handle = new_handle;
    let axes = &mut axes_slot_mut_by_handle(state, target_axes_handle)?.axes;
    axes.series.push(series);
    axes.legend = None;
    set_current_object_for_handle(state, new_handle);
    Ok(new_handle)
}

fn clone_axes_into_figure(
    state: &mut GraphicsState,
    source_axes_handle: u32,
    target_figure_handle: u32,
) -> Result<u32, RuntimeError> {
    let (_, _source_index) =
        axes_location_by_handle(state, source_axes_handle).ok_or_else(|| {
            RuntimeError::MissingVariable(format!(
                "axes handle `{source_axes_handle}` does not exist"
            ))
        })?;
    let mut source_axes = axes_slot_by_handle(state, source_axes_handle)?.axes.clone();
    if source_axes.position.is_none() {
        source_axes.position = Some(resolved_axes_position_array(state, source_axes_handle)?);
    }
    let cloned_series = source_axes
        .series
        .iter()
        .cloned()
        .map(|mut series| {
            series.handle = next_series_handle(state);
            series
        })
        .collect::<Vec<_>>();
    source_axes.series = cloned_series;

    let next_index = state
        .figures
        .get(&target_figure_handle)
        .ok_or_else(|| {
            RuntimeError::MissingVariable(format!(
                "figure handle `{target_figure_handle}` does not exist"
            ))
        })?
        .axes
        .keys()
        .copied()
        .max()
        .unwrap_or(0)
        + 1;
    let new_axes_handle = next_axes_handle(state);
    state
        .figures
        .get_mut(&target_figure_handle)
        .expect("target figure should exist")
        .axes
        .insert(
            next_index,
            AxesSlot {
                handle: new_axes_handle,
                axes: source_axes,
            },
        );
    set_current_object_for_handle(state, new_axes_handle);
    Ok(new_axes_handle)
}

#[derive(Debug, Clone)]
struct GraphicsHandleInputs {
    handles: Vec<u32>,
    rows: usize,
    cols: usize,
    scalar_input: bool,
}

fn graphics_handle_inputs(
    value: &Value,
    builtin_name: &str,
) -> Result<GraphicsHandleInputs, RuntimeError> {
    match value {
        Value::Scalar(_) | Value::Logical(_) => Ok(GraphicsHandleInputs {
            handles: vec![scalar_handle(value, builtin_name)?],
            rows: 1,
            cols: 1,
            scalar_input: true,
        }),
        Value::Matrix(matrix) => {
            if matrix.elements.is_empty() {
                return Ok(GraphicsHandleInputs {
                    handles: Vec::new(),
                    rows: matrix.rows,
                    cols: matrix.cols,
                    scalar_input: false,
                });
            }

            Ok(GraphicsHandleInputs {
                handles: matrix
                    .elements
                    .iter()
                    .map(|entry| scalar_handle(entry, builtin_name))
                    .collect::<Result<Vec<_>, _>>()?,
                rows: matrix.rows,
                cols: matrix.cols,
                scalar_input: false,
            })
        }
        _ => Err(RuntimeError::TypeError(format!(
            "{builtin_name} currently expects a numeric graphics handle array"
        ))),
    }
}

fn graphics_handle_inputs_value(inputs: &GraphicsHandleInputs) -> Result<Value, RuntimeError> {
    if inputs.scalar_input {
        return Ok(Value::Scalar(
            inputs.handles.first().copied().unwrap_or(0) as f64
        ));
    }

    Ok(Value::Matrix(MatrixValue::new(
        inputs.rows,
        inputs.cols,
        inputs
            .handles
            .iter()
            .copied()
            .map(|handle| Value::Scalar(handle as f64))
            .collect(),
    )?))
}

fn parse_linkaxes_mode(value: &Value) -> Result<LinkAxesMode, RuntimeError> {
    match text_arg(value, "linkaxes")?.to_ascii_lowercase().as_str() {
        "x" => Ok(LinkAxesMode::X),
        "y" => Ok(LinkAxesMode::Y),
        "xy" => Ok(LinkAxesMode::XY),
        other => Err(RuntimeError::Unsupported(format!(
            "linkaxes currently supports only the modes `x`, `y`, `xy`, or `off`, found `{other}`"
        ))),
    }
}

fn unlink_axes_handles(figure: &mut FigureState, handles: &[u32]) {
    if handles.is_empty() {
        return;
    }

    let removed = handles
        .iter()
        .copied()
        .collect::<std::collections::BTreeSet<_>>();
    for group in &mut figure.linked_axes {
        group.handles.retain(|handle| !removed.contains(handle));
    }
    figure.linked_axes.retain(|group| group.handles.len() >= 2);
}

fn apply_linkaxes_mode(
    state: &mut GraphicsState,
    _figure_handle: u32,
    handles: &[u32],
    mode: LinkAxesMode,
) -> Result<(), RuntimeError> {
    let mut x_limits: Option<(f64, f64)> = None;
    let mut y_limits: Option<(f64, f64)> = None;
    for handle in handles {
        let axes = &axes_slot_by_handle(state, *handle)?.axes;
        if mode.links_x() {
            let limits = resolved_limits(axes).0;
            x_limits = Some(match x_limits {
                Some((min, max)) => (min.min(limits.0), max.max(limits.1)),
                None => limits,
            });
        }
        if mode.links_y() {
            let limits = resolved_y_limits_for_side(axes, YAxisSide::Left);
            y_limits = Some(match y_limits {
                Some((min, max)) => (min.min(limits.0), max.max(limits.1)),
                None => limits,
            });
        }
    }

    for handle in handles {
        let axes = &mut axes_slot_mut_by_handle(state, *handle)?.axes;
        if let Some(x_limits) = x_limits.filter(|_| mode.links_x()) {
            axes.xlim = Some(x_limits);
        }
        if let Some(y_limits) = y_limits.filter(|_| mode.links_y()) {
            axes.ylim = Some(y_limits);
        }
    }

    Ok(())
}

fn sync_linked_axes_for_handle(
    state: &mut GraphicsState,
    axes_handle: u32,
    sync_x: bool,
    sync_y: bool,
) -> Result<(), RuntimeError> {
    if !sync_x && !sync_y {
        return Ok(());
    }

    let (figure_handle, _) = match axes_location_by_handle(state, axes_handle) {
        Some(location) => location,
        None => return Ok(()),
    };
    let groups = state
        .figures
        .get(&figure_handle)
        .map(|figure| {
            figure
                .linked_axes
                .iter()
                .filter(|group| group.handles.contains(&axes_handle))
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if groups.is_empty() {
        return Ok(());
    }

    let source_axes = axes_slot_by_handle(state, axes_handle)?.axes.clone();
    let source_x_limits = sync_x.then(|| resolved_limits(&source_axes).0);
    let source_y_limits = sync_y.then(|| resolved_y_limits_for_side(&source_axes, YAxisSide::Left));
    for group in groups {
        for handle in group.handles {
            if handle == axes_handle {
                continue;
            }
            let axes = &mut axes_slot_mut_by_handle(state, handle)?.axes;
            if let Some(x_limits) = source_x_limits.filter(|_| group.mode.links_x()) {
                axes.xlim = Some(x_limits);
            }
            if let Some(y_limits) = source_y_limits.filter(|_| group.mode.links_y()) {
                axes.ylim = Some(y_limits);
            }
        }
    }

    Ok(())
}

fn graphics_handle_query_value(
    state: &GraphicsState,
    requested: &Value,
    expected_type: Option<&str>,
    builtin_name: &str,
) -> Result<Value, RuntimeError> {
    let inputs = graphics_handle_inputs(requested, builtin_name)?;
    let flags = inputs
        .handles
        .into_iter()
        .map(|handle| {
            let Some(kind) = graphics_handle_kind(state, handle) else {
                return false;
            };
            expected_type
                .map(|expected| graphics_handle_matches_type(state, handle, kind, expected))
                .unwrap_or(true)
        })
        .collect::<Vec<_>>();
    logical_query_value(flags, inputs.rows, inputs.cols, inputs.scalar_input)
}

fn axes_position_value(state: &GraphicsState, axes_handle: u32) -> Result<Value, RuntimeError> {
    let position = resolved_axes_position_array(state, axes_handle)?;
    Ok(Value::Matrix(MatrixValue::new(
        1,
        4,
        position.into_iter().map(Value::Scalar).collect(),
    )?))
}

fn current_object_value(state: &GraphicsState) -> Result<Value, RuntimeError> {
    let Some(handle) = state.current_figure.and_then(|figure_handle| {
        state
            .figures
            .get(&figure_handle)
            .and_then(|figure| figure.current_object)
    }) else {
        return empty_matrix_value();
    };

    if graphics_handle_in_use(state, handle) {
        Ok(Value::Scalar(handle as f64))
    } else {
        empty_matrix_value()
    }
}

fn set_current_object_for_handle(state: &mut GraphicsState, handle: u32) {
    match graphics_handle_kind(state, handle) {
        Some(GraphicsHandleKind::Figure) => {}
        Some(GraphicsHandleKind::Axes) => {
            if let Some((figure_handle, _)) = axes_location_by_handle(state, handle) {
                if let Some(figure) = state.figures.get_mut(&figure_handle) {
                    figure.current_object = Some(handle);
                }
            }
        }
        Some(GraphicsHandleKind::Annotation) => {
            if let Some((figure_handle, _)) = annotation_location_by_handle(state, handle) {
                if let Some(figure) = state.figures.get_mut(&figure_handle) {
                    figure.current_object = Some(handle);
                }
            }
        }
        Some(GraphicsHandleKind::Series) => {
            if let Some((figure_handle, _, _)) = series_location_by_handle(state, handle) {
                if let Some(figure) = state.figures.get_mut(&figure_handle) {
                    figure.current_object = Some(handle);
                }
            }
        }
        None => {}
    }
}

fn resolved_axes_position_array(
    state: &GraphicsState,
    axes_handle: u32,
) -> Result<[f64; 4], RuntimeError> {
    let (figure_handle, axes_index) =
        axes_location_by_handle(state, axes_handle).ok_or_else(|| {
            RuntimeError::MissingVariable(format!("axes handle `{axes_handle}` does not exist"))
        })?;
    let figure = state
        .figures
        .get(&figure_handle)
        .expect("figure should exist");
    let slot = figure.axes.get(&axes_index).expect("axes should exist");
    let position = if let Some(position) = slot.axes.position {
        position
    } else {
        let cols = figure.layout_cols.max(1);
        let rows = figure.layout_rows.max(1);
        let row = (axes_index.saturating_sub(1)) / cols;
        let col = (axes_index.saturating_sub(1)) % cols;
        [
            col as f64 / cols as f64,
            1.0 - ((row + 1) as f64 / rows as f64),
            1.0 / cols as f64,
            1.0 / rows as f64,
        ]
    };
    Ok(position)
}

fn graphics_property_value_for_handle(
    state: &GraphicsState,
    handle: u32,
    property_value: &Value,
    builtin_name: &str,
) -> Result<Value, RuntimeError> {
    match graphics_handle_kind(state, handle).ok_or_else(|| {
        RuntimeError::MissingVariable(format!("graphics handle `{handle}` does not exist"))
    })? {
        GraphicsHandleKind::Figure => {
            let property = parse_figure_property(property_value, builtin_name)?;
            let figure = state
                .figures
                .get(&handle)
                .expect("figure handle should exist");
            figure_property_value(figure, handle, property)
        }
        GraphicsHandleKind::Axes => {
            let property = parse_axes_property(property_value, builtin_name)?;
            axes_property_value(
                state,
                handle,
                &axes_slot_by_handle(state, handle)?.axes,
                property,
            )
        }
        GraphicsHandleKind::Annotation => {
            let property = parse_annotation_property(property_value, builtin_name)?;
            annotation_property_value(
                state,
                handle,
                annotation_by_handle(state, handle)?,
                property,
            )
        }
        GraphicsHandleKind::Series => {
            let property = parse_series_property(property_value, builtin_name)?;
            series_property_value(state, handle, series_by_handle(state, handle)?, property)
        }
    }
}

fn graphics_property_struct_value(
    state: &GraphicsState,
    handle: u32,
) -> Result<Value, RuntimeError> {
    let field_names = match graphics_handle_kind(state, handle).ok_or_else(|| {
        RuntimeError::MissingVariable(format!("graphics handle `{handle}` does not exist"))
    })? {
        GraphicsHandleKind::Figure => {
            vec![
                "Children",
                "CloseRequestFcn",
                "CurrentAxes",
                "CurrentObject",
                "Name",
                "Number",
                "NumberTitle",
                "PaperOrientation",
                "PaperPosition",
                "PaperPositionMode",
                "PaperSize",
                "PaperType",
                "PaperUnits",
                "Position",
                "ResizeFcn",
                "Type",
                "Visible",
                "WindowStyle",
            ]
        }
        GraphicsHandleKind::Axes => vec![
            "Box",
            "CLim",
            "Children",
            "Colorbar",
            "Colormap",
            "Grid",
            "Hold",
            "Legend",
            "Parent",
            "Position",
            "Title",
            "Type",
            "View",
            "Visible",
            "XLabel",
            "XScale",
            "XLim",
            "XTick",
            "XTickAngle",
            "XTickLabel",
            "YLabel",
            "YScale",
            "YLim",
            "YTick",
            "YTickAngle",
            "YTickLabel",
            "ZLabel",
            "ZLim",
            "ZTick",
            "ZTickAngle",
            "ZTickLabel",
        ],
        GraphicsHandleKind::Annotation => vec![
            "Color",
            "FaceColor",
            "FontSize",
            "LineStyle",
            "LineWidth",
            "Parent",
            "Position",
            "String",
            "Type",
            "Visible",
        ],
        GraphicsHandleKind::Series => vec![
            "AlphaData",
            "AlphaDataMapping",
            "CData",
            "CDataMapping",
            "Color",
            "DisplayName",
            "EdgeColor",
            "FaceColor",
            "LineStyle",
            "LineWidth",
            "Marker",
            "MarkerEdgeColor",
            "MarkerFaceColor",
            "MarkerSize",
            "MaximumNumPoints",
            "SizeData",
            "Parent",
            "Position",
            "String",
            "Type",
            "Visible",
            "XData",
            "YData",
            "ZData",
        ],
    };

    let mut fields = BTreeMap::new();
    for field_name in field_names {
        if let Some(value) =
            graphics_property_value_for_handle_if_supported(state, handle, field_name)
        {
            fields.insert(field_name.to_string(), value);
        }
    }

    Ok(Value::Struct(StructValue { fields }))
}

fn apply_graphics_property_pairs(
    state: &mut GraphicsState,
    handle: u32,
    pairs: &[Value],
) -> Result<(), RuntimeError> {
    let handle_kind = graphics_handle_kind(state, handle).ok_or_else(|| {
        RuntimeError::MissingVariable(format!("graphics handle `{handle}` does not exist"))
    })?;

    for pair in pairs.chunks(2) {
        match handle_kind {
            GraphicsHandleKind::Figure => {
                let property = parse_figure_property(&pair[0], "set")?;
                match property {
                    FigureProperty::Name => {
                        let figure = state.figures.get_mut(&handle).ok_or_else(|| {
                            RuntimeError::MissingVariable(format!(
                                "figure handle `{handle}` does not exist"
                            ))
                        })?;
                        figure.name = text_arg(&pair[1], "set")?;
                    }
                    FigureProperty::NumberTitle => {
                        let figure = state.figures.get_mut(&handle).ok_or_else(|| {
                            RuntimeError::MissingVariable(format!(
                                "figure handle `{handle}` does not exist"
                            ))
                        })?;
                        figure.number_title = on_off_flag(&pair[1], "set")?;
                    }
                    FigureProperty::Visible => {
                        let figure = state.figures.get_mut(&handle).ok_or_else(|| {
                            RuntimeError::MissingVariable(format!(
                                "figure handle `{handle}` does not exist"
                            ))
                        })?;
                        figure.visible = on_off_flag(&pair[1], "set")?;
                    }
                    FigureProperty::Position => {
                        let figure = state.figures.get_mut(&handle).ok_or_else(|| {
                            RuntimeError::MissingVariable(format!(
                                "figure handle `{handle}` does not exist"
                            ))
                        })?;
                        set_figure_window_position(figure, &pair[1])?;
                    }
                    FigureProperty::WindowStyle => {
                        let figure = state.figures.get_mut(&handle).ok_or_else(|| {
                            RuntimeError::MissingVariable(format!(
                                "figure handle `{handle}` does not exist"
                            ))
                        })?;
                        figure.window_style = parse_figure_window_style(&pair[1])?;
                    }
                    FigureProperty::CloseRequestFcn => {
                        let figure = state.figures.get_mut(&handle).ok_or_else(|| {
                            RuntimeError::MissingVariable(format!(
                                "figure handle `{handle}` does not exist"
                            ))
                        })?;
                        figure.close_request_fcn =
                            parse_figure_callback_property(&pair[1], "CloseRequestFcn")?;
                    }
                    FigureProperty::ResizeFcn => {
                        let figure = state.figures.get_mut(&handle).ok_or_else(|| {
                            RuntimeError::MissingVariable(format!(
                                "figure handle `{handle}` does not exist"
                            ))
                        })?;
                        figure.resize_fcn = parse_figure_callback_property(&pair[1], "ResizeFcn")?;
                    }
                    FigureProperty::CurrentAxes => {
                        let requested_axes_handle = scalar_handle(&pair[1], "set")?;
                        let figure = state.figures.get_mut(&handle).ok_or_else(|| {
                            RuntimeError::MissingVariable(format!(
                                "figure handle `{handle}` does not exist"
                            ))
                        })?;
                        let requested_index =
                            axes_index_in_figure(figure, requested_axes_handle).ok_or_else(|| {
                                RuntimeError::MissingVariable(format!(
                                    "axes handle `{requested_axes_handle}` does not belong to figure `{handle}`"
                                ))
                            })?;
                        figure.current_axes = requested_index;
                    }
                    FigureProperty::CurrentObject => {
                        return Err(RuntimeError::Unsupported(
                            "set currently treats the figure `CurrentObject` property as read-only"
                                .to_string(),
                        ));
                    }
                    FigureProperty::Number => {
                        return Err(RuntimeError::Unsupported(
                            "set currently treats the figure `Number` property as read-only"
                                .to_string(),
                        ));
                    }
                    FigureProperty::Type => {
                        return Err(RuntimeError::Unsupported(
                            "set currently treats the figure `Type` property as read-only"
                                .to_string(),
                        ));
                    }
                    FigureProperty::Children => {
                        return Err(RuntimeError::Unsupported(
                            "set currently treats the figure `Children` property as read-only"
                                .to_string(),
                        ));
                    }
                    FigureProperty::PaperUnits => {
                        let figure = state.figures.get_mut(&handle).ok_or_else(|| {
                            RuntimeError::MissingVariable(format!(
                                "figure handle `{handle}` does not exist"
                            ))
                        })?;
                        figure.paper_units = parse_paper_units(&pair[1])?;
                    }
                    FigureProperty::PaperType => {
                        let figure = state.figures.get_mut(&handle).ok_or_else(|| {
                            RuntimeError::MissingVariable(format!(
                                "figure handle `{handle}` does not exist"
                            ))
                        })?;
                        set_figure_paper_type(figure, &pair[1])?;
                    }
                    FigureProperty::PaperSize => {
                        let figure = state.figures.get_mut(&handle).ok_or_else(|| {
                            RuntimeError::MissingVariable(format!(
                                "figure handle `{handle}` does not exist"
                            ))
                        })?;
                        set_figure_paper_size(figure, &pair[1])?;
                    }
                    FigureProperty::PaperPosition => {
                        let figure = state.figures.get_mut(&handle).ok_or_else(|| {
                            RuntimeError::MissingVariable(format!(
                                "figure handle `{handle}` does not exist"
                            ))
                        })?;
                        set_figure_paper_position(figure, &pair[1])?;
                    }
                    FigureProperty::PaperPositionMode => {
                        let figure = state.figures.get_mut(&handle).ok_or_else(|| {
                            RuntimeError::MissingVariable(format!(
                                "figure handle `{handle}` does not exist"
                            ))
                        })?;
                        figure.paper_position_mode = parse_paper_position_mode(&pair[1])?;
                    }
                    FigureProperty::PaperOrientation => {
                        let figure = state.figures.get_mut(&handle).ok_or_else(|| {
                            RuntimeError::MissingVariable(format!(
                                "figure handle `{handle}` does not exist"
                            ))
                        })?;
                        set_figure_paper_orientation(figure, &pair[1])?;
                    }
                }
            }
            GraphicsHandleKind::Axes => {
                let property = parse_axes_property(&pair[0], "set")?;
                if matches!(
                    property,
                    AxesProperty::Type | AxesProperty::Parent | AxesProperty::Children
                ) {
                    return Err(RuntimeError::Unsupported(format!(
                        "set currently treats the axes `{}` property as read-only",
                        match property {
                            AxesProperty::Type => "Type",
                            AxesProperty::Parent => "Parent",
                            AxesProperty::Children => "Children",
                            _ => unreachable!(),
                        }
                    )));
                }
                let sync_x = matches!(property, AxesProperty::XLim);
                let sync_y = matches!(property, AxesProperty::YLim);
                {
                    let axes = &mut axes_slot_mut_by_handle(state, handle)?.axes;
                    set_axes_property(axes, property, &pair[1])?;
                }
                sync_linked_axes_for_handle(state, handle, sync_x, sync_y)?;
            }
            GraphicsHandleKind::Annotation => {
                let property = parse_annotation_property(&pair[0], "set")?;
                if matches!(
                    property,
                    AnnotationProperty::Type | AnnotationProperty::Parent
                ) {
                    return Err(RuntimeError::Unsupported(
                        "set currently treats the annotation `Type` and `Parent` properties as read-only".to_string(),
                    ));
                }
                let annotation = annotation_mut_by_handle(state, handle)?;
                set_annotation_property(annotation, property, &pair[1])?;
            }
            GraphicsHandleKind::Series => {
                let property = parse_series_property(&pair[0], "set")?;
                let series = series_mut_by_handle(state, handle)?;
                set_series_property(series, property, &pair[1])?;
            }
        }
    }

    Ok(())
}

fn apply_graphics_property_struct(
    state: &mut GraphicsState,
    handle: u32,
    props: &StructValue,
) -> Result<(), RuntimeError> {
    let handle_kind = graphics_handle_kind(state, handle).ok_or_else(|| {
        RuntimeError::MissingVariable(format!("graphics handle `{handle}` does not exist"))
    })?;
    let mut entries = props
        .fields
        .iter()
        .map(|(name, value)| {
            (
                graphics_struct_property_priority(handle_kind, name),
                name.clone(),
                value.clone(),
            )
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| entry.0);

    for (_, name, value) in entries {
        if graphics_struct_property_is_read_only(handle_kind, &name)
            || graphics_struct_property_is_shadowed(handle_kind, &name, props)
        {
            continue;
        }
        let pair = vec![Value::CharArray(name), value];
        apply_graphics_property_pairs(state, handle, &pair)?;
    }

    Ok(())
}

fn parse_paper_units(value: &Value) -> Result<PaperUnits, RuntimeError> {
    match text_arg(value, "set")?.to_ascii_lowercase().as_str() {
        "inches" => Ok(PaperUnits::Inches),
        "centimeters" => Ok(PaperUnits::Centimeters),
        "points" => Ok(PaperUnits::Points),
        "normalized" => Ok(PaperUnits::Normalized),
        other => Err(RuntimeError::Unsupported(format!(
            "set currently supports figure `PaperUnits` values `inches`, `centimeters`, `points`, or `normalized`, found `{other}`"
        ))),
    }
}

fn parse_paper_type(value: &Value) -> Result<PaperType, RuntimeError> {
    match text_arg(value, "set")?.to_ascii_lowercase().as_str() {
        "usletter" | "letter" => Ok(PaperType::UsLetter),
        "uslegal" | "legal" => Ok(PaperType::UsLegal),
        "tabloid" => Ok(PaperType::Tabloid),
        "a3" => Ok(PaperType::A3),
        "a4" => Ok(PaperType::A4),
        "custom" => Err(RuntimeError::Unsupported(
            "set currently derives figure `PaperType` `custom` from `PaperSize`; it cannot be set directly".to_string(),
        )),
        other => Err(RuntimeError::Unsupported(format!(
            "set currently supports figure `PaperType` values `usletter`, `uslegal`, `tabloid`, `a3`, or `a4`, found `{other}`"
        ))),
    }
}

fn parse_paper_position_mode(value: &Value) -> Result<PaperPositionMode, RuntimeError> {
    match text_arg(value, "set")?.to_ascii_lowercase().as_str() {
        "auto" => Ok(PaperPositionMode::Auto),
        "manual" => Ok(PaperPositionMode::Manual),
        other => Err(RuntimeError::Unsupported(format!(
            "set currently supports figure `PaperPositionMode` values `auto` or `manual`, found `{other}`"
        ))),
    }
}

fn parse_paper_orientation(value: &Value) -> Result<PaperOrientation, RuntimeError> {
    match text_arg(value, "set")?.to_ascii_lowercase().as_str() {
        "portrait" => Ok(PaperOrientation::Portrait),
        "landscape" => Ok(PaperOrientation::Landscape),
        other => Err(RuntimeError::Unsupported(format!(
            "set currently supports figure `PaperOrientation` values `portrait` or `landscape`, found `{other}`"
        ))),
    }
}

fn parse_figure_window_style(value: &Value) -> Result<FigureWindowStyle, RuntimeError> {
    match text_arg(value, "set")?.to_ascii_lowercase().as_str() {
        "normal" => Ok(FigureWindowStyle::Normal),
        "docked" => Ok(FigureWindowStyle::Docked),
        other => Err(RuntimeError::Unsupported(format!(
            "set currently supports figure `WindowStyle` values `normal` or `docked`, found `{other}`"
        ))),
    }
}

fn set_figure_paper_type(figure: &mut FigureState, value: &Value) -> Result<(), RuntimeError> {
    let paper_type = parse_paper_type(value)?;
    figure.paper_type = paper_type;
    figure.paper_size_in = standard_paper_size_in(paper_type, figure.paper_orientation);
    if figure.paper_position_mode == PaperPositionMode::Auto {
        figure.paper_position_in = default_auto_paper_position_in(figure.paper_size_in);
    }
    Ok(())
}

fn set_figure_paper_size(figure: &mut FigureState, value: &Value) -> Result<(), RuntimeError> {
    if figure.paper_units == PaperUnits::Normalized {
        return Err(RuntimeError::Unsupported(
            "set currently does not support figure `PaperSize` while `PaperUnits` is `normalized`"
                .to_string(),
        ));
    }
    let values = numeric_vector(value, "set")?;
    if values.len() != 2 || !values.iter().all(|entry| entry.is_finite() && *entry > 0.0) {
        return Err(RuntimeError::Unsupported(
            "set currently expects figure `PaperSize` as a positive finite numeric 1x2 vector"
                .to_string(),
        ));
    }
    let width_in = units_to_inches(values[0], figure.paper_units, figure.paper_size_in[0]);
    let height_in = units_to_inches(values[1], figure.paper_units, figure.paper_size_in[1]);
    figure.paper_size_in = [width_in, height_in];
    figure.paper_type = matched_paper_type(figure.paper_size_in, figure.paper_orientation)
        .unwrap_or(PaperType::Custom);
    if figure.paper_position_mode == PaperPositionMode::Auto {
        figure.paper_position_in = default_auto_paper_position_in(figure.paper_size_in);
    }
    Ok(())
}

fn set_figure_paper_position(figure: &mut FigureState, value: &Value) -> Result<(), RuntimeError> {
    let values = numeric_vector(value, "set")?;
    if values.len() != 4 || !values.iter().all(|entry| entry.is_finite()) {
        return Err(RuntimeError::Unsupported(
            "set currently expects figure `PaperPosition` as a finite numeric 1x4 vector"
                .to_string(),
        ));
    }
    figure.paper_position_in = [
        units_to_inches(values[0], figure.paper_units, figure.paper_size_in[0]),
        units_to_inches(values[1], figure.paper_units, figure.paper_size_in[1]),
        units_to_inches(values[2], figure.paper_units, figure.paper_size_in[0]),
        units_to_inches(values[3], figure.paper_units, figure.paper_size_in[1]),
    ];
    Ok(())
}

fn set_figure_paper_orientation(
    figure: &mut FigureState,
    value: &Value,
) -> Result<(), RuntimeError> {
    let orientation = parse_paper_orientation(value)?;
    if orientation != figure.paper_orientation {
        figure.paper_orientation = orientation;
        figure.paper_size_in.swap(0, 1);
        if figure.paper_position_mode == PaperPositionMode::Auto {
            figure.paper_position_in = default_auto_paper_position_in(figure.paper_size_in);
        }
    }
    Ok(())
}

fn set_figure_window_position(figure: &mut FigureState, value: &Value) -> Result<(), RuntimeError> {
    let values = numeric_vector(value, "set")?;
    if values.len() != 4
        || !values.iter().all(|entry| entry.is_finite())
        || values[2] <= 0.0
        || values[3] <= 0.0
    {
        return Err(RuntimeError::Unsupported(
            "set currently expects figure `Position` as a finite numeric 1x4 vector with positive width and height"
                .to_string(),
        ));
    }
    figure.position = [values[0], values[1], values[2], values[3]];
    Ok(())
}

fn parse_figure_callback_property(
    value: &Value,
    property_name: &str,
) -> Result<Option<Value>, RuntimeError> {
    match value {
        Value::Matrix(matrix) if matrix.elements.is_empty() => Ok(None),
        Value::FunctionHandle(_) | Value::CharArray(_) | Value::String(_) => Ok(Some(value.clone())),
        _ => Err(RuntimeError::Unsupported(format!(
            "set currently supports figure `{property_name}` values as function handles, text function names, or []"
        ))),
    }
}

fn callback_property_value(callback: &Option<Value>) -> Result<Value, RuntimeError> {
    match callback {
        Some(value) => Ok(value.clone()),
        None => empty_matrix_value(),
    }
}

fn matched_paper_type(size_in: [f64; 2], orientation: PaperOrientation) -> Option<PaperType> {
    [
        PaperType::UsLetter,
        PaperType::UsLegal,
        PaperType::Tabloid,
        PaperType::A3,
        PaperType::A4,
    ]
    .into_iter()
    .find(|paper_type| {
        let expected = standard_paper_size_in(*paper_type, orientation);
        (expected[0] - size_in[0]).abs() <= 1e-9 && (expected[1] - size_in[1]).abs() <= 1e-9
    })
}

fn graphics_struct_property_priority(kind: GraphicsHandleKind, name: &str) -> u8 {
    let lower = name.to_ascii_lowercase();
    match kind {
        GraphicsHandleKind::Figure => match lower.as_str() {
            "name" => 0,
            "numbertitle" => 10,
            "closerequestfcn" => 20,
            "resizefcn" => 30,
            "windowstyle" => 40,
            "position" => 50,
            "visible" => 60,
            "paperunits" => 70,
            "papertype" => 80,
            "papersize" => 90,
            "paperorientation" => 100,
            "paperpositionmode" => 110,
            "paperposition" => 120,
            "currentaxes" => 130,
            _ => 200,
        },
        GraphicsHandleKind::Axes => match lower.as_str() {
            "position" => 0,
            "xscale" | "yscale" => 5,
            "xlim" | "ylim" | "zlim" | "clim" | "caxis" | "view" => 10,
            "xtick" | "ytick" | "ztick" => 20,
            "xticklabel" | "yticklabel" | "zticklabel" => 30,
            "xtickangle" | "ytickangle" | "ztickangle" => 40,
            "title" | "xlabel" | "ylabel" | "zlabel" => 50,
            _ => 60,
        },
        GraphicsHandleKind::Series => match lower.as_str() {
            "xdata" | "ydata" | "zdata" | "position" => 0,
            "string" => 10,
            "color" | "edgecolor" | "facecolor" => 20,
            "linewidth" | "linestyle" | "marker" | "markersize" | "maximumnumpoints"
            | "sizedata" | "markeredgecolor" | "markerfacecolor" => 30,
            _ => 40,
        },
        GraphicsHandleKind::Annotation => match lower.as_str() {
            "position" => 0,
            "string" => 10,
            "color" | "facecolor" => 20,
            "linewidth" | "linestyle" | "fontsize" => 30,
            _ => 40,
        },
    }
}

fn graphics_struct_property_is_read_only(kind: GraphicsHandleKind, name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    match kind {
        GraphicsHandleKind::Figure => matches!(
            lower.as_str(),
            "currentobject" | "number" | "type" | "children"
        ),
        GraphicsHandleKind::Axes => matches!(lower.as_str(), "type" | "parent" | "children"),
        GraphicsHandleKind::Series => matches!(lower.as_str(), "type" | "parent"),
        GraphicsHandleKind::Annotation => matches!(lower.as_str(), "type" | "parent"),
    }
}

fn graphics_struct_property_is_shadowed(
    kind: GraphicsHandleKind,
    name: &str,
    props: &StructValue,
) -> bool {
    let lower = name.to_ascii_lowercase();
    match kind {
        GraphicsHandleKind::Series => {
            lower == "edgecolor"
                && props
                    .fields
                    .keys()
                    .any(|field| field.eq_ignore_ascii_case("color"))
        }
        GraphicsHandleKind::Annotation => {
            lower == "facecolor"
                && props
                    .fields
                    .keys()
                    .any(|field| field.eq_ignore_ascii_case("color"))
        }
        _ => false,
    }
}

fn graphics_handle_matches_type(
    state: &GraphicsState,
    handle: u32,
    kind: GraphicsHandleKind,
    expected_type: &str,
) -> bool {
    let expected = expected_type.to_ascii_lowercase();
    match kind {
        GraphicsHandleKind::Figure => expected == "figure",
        GraphicsHandleKind::Axes => expected == "axes",
        GraphicsHandleKind::Annotation => annotation_by_handle(state, handle)
            .map(|annotation| annotation.kind.type_name().eq_ignore_ascii_case(&expected))
            .unwrap_or(false),
        GraphicsHandleKind::Series => series_by_handle(state, handle)
            .map(|series| {
                series
                    .kind
                    .property_type_name()
                    .eq_ignore_ascii_case(&expected)
            })
            .unwrap_or(false),
    }
}

#[derive(Debug, Clone)]
struct GraphicsSearchFilter {
    property_name: String,
    expected_value: Value,
}

fn find_graphics_objects_value(
    state: &GraphicsState,
    args: &[Value],
    builtin_name: &str,
) -> Result<Value, RuntimeError> {
    let (roots, filters) = parse_find_graphics_arguments(state, args, builtin_name)?;
    let mut found = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for root in roots {
        collect_graphics_descendants(state, root, &mut found, &mut seen)?;
    }

    found.retain(|handle| graphics_handle_matches_filters(state, *handle, &filters));

    graphics_handle_vector_value(found)
}

fn parse_find_graphics_arguments(
    state: &GraphicsState,
    args: &[Value],
    builtin_name: &str,
) -> Result<(Vec<u32>, Vec<GraphicsSearchFilter>), RuntimeError> {
    let current_roots = state
        .current_figure
        .filter(|handle| state.figures.contains_key(handle))
        .into_iter()
        .collect::<Vec<_>>();

    if args.is_empty() {
        return Ok((current_roots, Vec::new()));
    }

    let (roots, filter_args) = match &args[0] {
        Value::CharArray(_) | Value::String(_) => (current_roots, args),
        _ => {
            let parsed = graphics_handle_inputs(&args[0], builtin_name)?;
            (parsed.handles, &args[1..])
        }
    };

    if filter_args.is_empty() {
        return Ok((roots, Vec::new()));
    }
    if filter_args.len() % 2 != 0 {
        return Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently expects property/value pairs after the optional handle roots"
        )));
    }

    let mut filters = Vec::new();
    for pair in filter_args.chunks(2) {
        filters.push(GraphicsSearchFilter {
            property_name: text_arg(&pair[0], builtin_name)?,
            expected_value: pair[1].clone(),
        });
    }

    Ok((roots, filters))
}

fn graphics_handle_matches_filters(
    state: &GraphicsState,
    handle: u32,
    filters: &[GraphicsSearchFilter],
) -> bool {
    filters.iter().all(|filter| {
        graphics_property_value_for_handle_if_supported(state, handle, &filter.property_name)
            .map(|actual| graphics_search_values_match(&actual, &filter.expected_value))
            .unwrap_or(false)
    })
}

fn graphics_property_value_for_handle_if_supported(
    state: &GraphicsState,
    handle: u32,
    property_name: &str,
) -> Option<Value> {
    let property_name = Value::CharArray(property_name.to_string());
    let value =
        graphics_property_value_for_handle(state, handle, &property_name, "findobj").ok()?;
    if property_name_matches(&property_name, "MaximumNumPoints")
        && matches!(&value, Value::Matrix(matrix) if matrix.elements.is_empty())
    {
        return None;
    }
    Some(value)
}

fn property_name_matches(value: &Value, expected: &str) -> bool {
    matches!(value, Value::CharArray(text) | Value::String(text) if text.eq_ignore_ascii_case(expected))
}

fn graphics_search_values_match(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::CharArray(left), Value::CharArray(right))
        | (Value::CharArray(left), Value::String(right))
        | (Value::String(left), Value::CharArray(right))
        | (Value::String(left), Value::String(right)) => left.eq_ignore_ascii_case(right),
        _ => actual == expected,
    }
}

fn collect_graphics_descendants(
    state: &GraphicsState,
    handle: u32,
    out: &mut Vec<u32>,
    seen: &mut std::collections::BTreeSet<u32>,
) -> Result<(), RuntimeError> {
    if !seen.insert(handle) {
        return Ok(());
    }
    if graphics_handle_kind(state, handle).is_none() {
        return Err(RuntimeError::MissingVariable(format!(
            "graphics handle `{handle}` does not exist"
        )));
    }
    out.push(handle);
    for child in direct_graphics_children(state, handle)? {
        collect_graphics_descendants(state, child, out, seen)?;
    }
    Ok(())
}

fn direct_graphics_children(state: &GraphicsState, handle: u32) -> Result<Vec<u32>, RuntimeError> {
    match graphics_handle_kind(state, handle).ok_or_else(|| {
        RuntimeError::MissingVariable(format!("graphics handle `{handle}` does not exist"))
    })? {
        GraphicsHandleKind::Figure => Ok(figure_children_handles(
            state
                .figures
                .get(&handle)
                .expect("figure handle should exist"),
        )),
        GraphicsHandleKind::Axes => Ok(axes_children_handles(axes_slot_by_handle(state, handle)?)),
        GraphicsHandleKind::Annotation => Ok(Vec::new()),
        GraphicsHandleKind::Series => Ok(Vec::new()),
    }
}

fn ancestor_handle(
    state: &GraphicsState,
    handle: u32,
    kind: &str,
) -> Result<Option<u32>, RuntimeError> {
    match graphics_handle_kind(state, handle).ok_or_else(|| {
        RuntimeError::MissingVariable(format!("graphics handle `{handle}` does not exist"))
    })? {
        GraphicsHandleKind::Figure => match kind {
            "figure" => Ok(Some(handle)),
            "axes" | "line" | "scatter" | "quiver" | "surface" | "mesh" | "image" | "text" | "rectangle" | "patch" | "arrow" | "doublearrow" | "textarrow" | "textbox" | "ellipse" => Ok(None),
            other => Err(RuntimeError::Unsupported(format!(
                "ancestor currently supports only graphics type names like `figure` and `axes`, found `{other}`"
            ))),
        },
        GraphicsHandleKind::Axes => {
            let (figure_handle, _) = axes_location_by_handle(state, handle)
                .expect("axes handle should exist");
            match kind {
                "axes" => Ok(Some(handle)),
                "figure" => Ok(Some(figure_handle)),
                "line" | "scatter" | "quiver" | "surface" | "mesh" | "image" | "text" | "rectangle" | "patch" | "arrow" | "doublearrow" | "textarrow" | "textbox" | "ellipse" => Ok(None),
                other => Err(RuntimeError::Unsupported(format!(
                    "ancestor currently supports only graphics type names like `figure` and `axes`, found `{other}`"
                ))),
            }
        }
        GraphicsHandleKind::Annotation => {
            let (figure_handle, _) = annotation_location_by_handle(state, handle)
                .expect("annotation handle should exist");
            match kind {
                "figure" => Ok(Some(figure_handle)),
                other if annotation_by_handle(state, handle)?.kind.type_name().eq_ignore_ascii_case(other) => Ok(Some(handle)),
                "axes" | "line" | "scatter" | "quiver" | "surface" | "mesh" | "image" | "text" | "rectangle" | "patch" => Ok(None),
                other => Err(RuntimeError::Unsupported(format!(
                    "ancestor currently supports only graphics type names like `figure`, `axes`, and supported annotation types, found `{other}`"
                ))),
            }
        }
        GraphicsHandleKind::Series => {
            let series_kind = graphics_handle_matches_type(
                state,
                handle,
                GraphicsHandleKind::Series,
                kind,
            );
            if series_kind {
                return Ok(Some(handle));
            }
            let (figure_handle, axes_index, _) = series_location_by_handle(state, handle)
                .expect("series handle should exist");
            let axes_handle = state
                .figures
                .get(&figure_handle)
                .expect("figure should exist")
                .axes
                .get(&axes_index)
                .expect("axes should exist")
                .handle;
            match kind {
                "axes" => Ok(Some(axes_handle)),
                "figure" => Ok(Some(figure_handle)),
                "line" | "scatter" | "quiver" | "surface" | "mesh" | "image" | "text" | "rectangle" | "patch" => Ok(None),
                other => Err(RuntimeError::Unsupported(format!(
                    "ancestor currently supports only graphics type names like `figure` and `axes`, found `{other}`"
                ))),
            }
        }
    }
}

fn figure_children_handles(figure: &FigureState) -> Vec<u32> {
    let mut handles = figure
        .axes
        .values()
        .map(|slot| slot.handle)
        .collect::<Vec<_>>();
    handles.extend(
        figure
            .annotations
            .iter()
            .map(|annotation| annotation.handle),
    );
    handles
}

fn axes_children_handles(slot: &AxesSlot) -> Vec<u32> {
    slot.axes
        .series
        .iter()
        .map(|series| series.handle)
        .collect()
}

fn one_or_zero_outputs(
    value: Value,
    output_arity: usize,
    builtin_name: &str,
) -> Result<Vec<Value>, RuntimeError> {
    match output_arity {
        0 => Ok(Vec::new()),
        1 => Ok(vec![value]),
        _ => Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently supports at most one output"
        ))),
    }
}

fn series_handle_array_value(handles: &[u32]) -> Result<Value, RuntimeError> {
    if handles.len() == 1 {
        return Ok(Value::Scalar(handles[0] as f64));
    }

    Ok(Value::Matrix(MatrixValue::new(
        1,
        handles.len(),
        handles
            .iter()
            .copied()
            .map(|handle| Value::Scalar(handle as f64))
            .collect(),
    )?))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FigureProperty {
    Name,
    NumberTitle,
    Visible,
    Position,
    WindowStyle,
    CloseRequestFcn,
    ResizeFcn,
    CurrentAxes,
    CurrentObject,
    Number,
    Type,
    Children,
    PaperUnits,
    PaperType,
    PaperSize,
    PaperPosition,
    PaperPositionMode,
    PaperOrientation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AxesProperty {
    Position,
    Title,
    XLabel,
    YLabel,
    ZLabel,
    XScale,
    YScale,
    XLim,
    YLim,
    ZLim,
    XTick,
    YTick,
    ZTick,
    XTickLabel,
    YTickLabel,
    ZTickLabel,
    XTickAngle,
    YTickAngle,
    ZTickAngle,
    Visible,
    Box,
    Grid,
    Hold,
    View,
    CLim,
    Colormap,
    Colorbar,
    Legend,
    Type,
    Parent,
    Children,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SeriesProperty {
    Type,
    XData,
    YData,
    ZData,
    AlphaData,
    AlphaDataMapping,
    CData,
    CDataMapping,
    DisplayName,
    Visible,
    Parent,
    String,
    Position,
    Color,
    LineWidth,
    LineStyle,
    Marker,
    MarkerSize,
    MaximumNumPoints,
    SizeData,
    MarkerEdgeColor,
    MarkerFaceColor,
    EdgeColor,
    FaceColor,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnnotationProperty {
    Type,
    Parent,
    Position,
    String,
    Color,
    LineWidth,
    LineStyle,
    FaceColor,
    Visible,
    FontSize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GraphicsHandleKind {
    Figure,
    Axes,
    Annotation,
    Series,
}

fn graphics_handle_kind(state: &GraphicsState, handle: u32) -> Option<GraphicsHandleKind> {
    if state.figures.contains_key(&handle) {
        Some(GraphicsHandleKind::Figure)
    } else if axes_location_by_handle(state, handle).is_some() {
        Some(GraphicsHandleKind::Axes)
    } else if annotation_location_by_handle(state, handle).is_some() {
        Some(GraphicsHandleKind::Annotation)
    } else if series_location_by_handle(state, handle).is_some() {
        Some(GraphicsHandleKind::Series)
    } else {
        None
    }
}

fn parse_figure_property(
    value: &Value,
    builtin_name: &str,
) -> Result<FigureProperty, RuntimeError> {
    match text_arg(value, builtin_name)?.to_ascii_lowercase().as_str() {
        "name" => Ok(FigureProperty::Name),
        "numbertitle" => Ok(FigureProperty::NumberTitle),
        "visible" => Ok(FigureProperty::Visible),
        "position" => Ok(FigureProperty::Position),
        "windowstyle" => Ok(FigureProperty::WindowStyle),
        "closerequestfcn" => Ok(FigureProperty::CloseRequestFcn),
        "resizefcn" => Ok(FigureProperty::ResizeFcn),
        "currentaxes" => Ok(FigureProperty::CurrentAxes),
        "currentobject" => Ok(FigureProperty::CurrentObject),
        "number" => Ok(FigureProperty::Number),
        "type" => Ok(FigureProperty::Type),
        "children" => Ok(FigureProperty::Children),
        "paperunits" => Ok(FigureProperty::PaperUnits),
        "papertype" => Ok(FigureProperty::PaperType),
        "papersize" => Ok(FigureProperty::PaperSize),
        "paperposition" => Ok(FigureProperty::PaperPosition),
        "paperpositionmode" => Ok(FigureProperty::PaperPositionMode),
        "paperorientation" => Ok(FigureProperty::PaperOrientation),
        other => Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently does not support the graphics property `{other}`"
        ))),
    }
}

fn parse_axes_property(value: &Value, builtin_name: &str) -> Result<AxesProperty, RuntimeError> {
    match text_arg(value, builtin_name)?.to_ascii_lowercase().as_str() {
        "position" => Ok(AxesProperty::Position),
        "title" => Ok(AxesProperty::Title),
        "xlabel" => Ok(AxesProperty::XLabel),
        "ylabel" => Ok(AxesProperty::YLabel),
        "zlabel" => Ok(AxesProperty::ZLabel),
        "xscale" => Ok(AxesProperty::XScale),
        "yscale" => Ok(AxesProperty::YScale),
        "xlim" => Ok(AxesProperty::XLim),
        "ylim" => Ok(AxesProperty::YLim),
        "zlim" => Ok(AxesProperty::ZLim),
        "xtick" | "xticks" => Ok(AxesProperty::XTick),
        "ytick" | "yticks" => Ok(AxesProperty::YTick),
        "ztick" | "zticks" => Ok(AxesProperty::ZTick),
        "xticklabel" | "xticklabels" => Ok(AxesProperty::XTickLabel),
        "yticklabel" | "yticklabels" => Ok(AxesProperty::YTickLabel),
        "zticklabel" | "zticklabels" => Ok(AxesProperty::ZTickLabel),
        "xtickangle" => Ok(AxesProperty::XTickAngle),
        "ytickangle" => Ok(AxesProperty::YTickAngle),
        "ztickangle" => Ok(AxesProperty::ZTickAngle),
        "visible" => Ok(AxesProperty::Visible),
        "box" => Ok(AxesProperty::Box),
        "grid" => Ok(AxesProperty::Grid),
        "hold" => Ok(AxesProperty::Hold),
        "view" => Ok(AxesProperty::View),
        "clim" | "caxis" => Ok(AxesProperty::CLim),
        "colormap" => Ok(AxesProperty::Colormap),
        "colorbar" => Ok(AxesProperty::Colorbar),
        "legend" => Ok(AxesProperty::Legend),
        "type" => Ok(AxesProperty::Type),
        "parent" => Ok(AxesProperty::Parent),
        "children" => Ok(AxesProperty::Children),
        other => Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently does not support the graphics property `{other}`"
        ))),
    }
}

fn parse_series_property(
    value: &Value,
    builtin_name: &str,
) -> Result<SeriesProperty, RuntimeError> {
    match text_arg(value, builtin_name)?.to_ascii_lowercase().as_str() {
        "type" => Ok(SeriesProperty::Type),
        "xdata" => Ok(SeriesProperty::XData),
        "ydata" => Ok(SeriesProperty::YData),
        "zdata" => Ok(SeriesProperty::ZData),
        "alphadata" => Ok(SeriesProperty::AlphaData),
        "alphadatamapping" => Ok(SeriesProperty::AlphaDataMapping),
        "cdata" => Ok(SeriesProperty::CData),
        "cdatamapping" => Ok(SeriesProperty::CDataMapping),
        "displayname" => Ok(SeriesProperty::DisplayName),
        "visible" => Ok(SeriesProperty::Visible),
        "parent" => Ok(SeriesProperty::Parent),
        "string" => Ok(SeriesProperty::String),
        "position" => Ok(SeriesProperty::Position),
        "color" => Ok(SeriesProperty::Color),
        "linewidth" => Ok(SeriesProperty::LineWidth),
        "linestyle" => Ok(SeriesProperty::LineStyle),
        "marker" => Ok(SeriesProperty::Marker),
        "markersize" => Ok(SeriesProperty::MarkerSize),
        "maximumnumpoints" => Ok(SeriesProperty::MaximumNumPoints),
        "sizedata" => Ok(SeriesProperty::SizeData),
        "markeredgecolor" => Ok(SeriesProperty::MarkerEdgeColor),
        "markerfacecolor" => Ok(SeriesProperty::MarkerFaceColor),
        "edgecolor" => Ok(SeriesProperty::EdgeColor),
        "facecolor" => Ok(SeriesProperty::FaceColor),
        other => Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently does not support the graphics series property `{other}`"
        ))),
    }
}

fn parse_annotation_property(
    value: &Value,
    builtin_name: &str,
) -> Result<AnnotationProperty, RuntimeError> {
    match text_arg(value, builtin_name)?.to_ascii_lowercase().as_str() {
        "type" => Ok(AnnotationProperty::Type),
        "parent" => Ok(AnnotationProperty::Parent),
        "position" => Ok(AnnotationProperty::Position),
        "string" => Ok(AnnotationProperty::String),
        "color" => Ok(AnnotationProperty::Color),
        "linewidth" => Ok(AnnotationProperty::LineWidth),
        "linestyle" => Ok(AnnotationProperty::LineStyle),
        "facecolor" => Ok(AnnotationProperty::FaceColor),
        "visible" => Ok(AnnotationProperty::Visible),
        "fontsize" => Ok(AnnotationProperty::FontSize),
        other => Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently does not support the annotation property `{other}`"
        ))),
    }
}

fn figure_property_value(
    figure: &FigureState,
    handle: u32,
    property: FigureProperty,
) -> Result<Value, RuntimeError> {
    match property {
        FigureProperty::Name => Ok(Value::CharArray(figure.name.clone())),
        FigureProperty::NumberTitle => Ok(Value::CharArray(
            if figure.number_title { "on" } else { "off" }.to_string(),
        )),
        FigureProperty::Visible => Ok(Value::CharArray(
            if figure.visible { "on" } else { "off" }.to_string(),
        )),
        FigureProperty::Position => vector_value(&figure.position),
        FigureProperty::WindowStyle => {
            Ok(Value::CharArray(figure.window_style.as_text().to_string()))
        }
        FigureProperty::CloseRequestFcn => callback_property_value(&figure.close_request_fcn),
        FigureProperty::ResizeFcn => callback_property_value(&figure.resize_fcn),
        FigureProperty::CurrentAxes => match figure_current_axes_handle(figure) {
            Some(axes_handle) => Ok(Value::Scalar(axes_handle as f64)),
            None => empty_matrix_value(),
        },
        FigureProperty::CurrentObject => match figure.current_object {
            Some(handle) => Ok(Value::Scalar(handle as f64)),
            None => empty_matrix_value(),
        },
        FigureProperty::Number => Ok(Value::Scalar(handle as f64)),
        FigureProperty::Type => Ok(Value::CharArray("figure".to_string())),
        FigureProperty::Children => graphics_handle_vector_value(figure_children_handles(figure)),
        FigureProperty::PaperUnits => {
            Ok(Value::CharArray(figure.paper_units.as_text().to_string()))
        }
        FigureProperty::PaperType => Ok(Value::CharArray(figure.paper_type.as_text().to_string())),
        FigureProperty::PaperSize => paper_size_value(figure),
        FigureProperty::PaperPosition => paper_position_value(figure),
        FigureProperty::PaperPositionMode => Ok(Value::CharArray(
            figure.paper_position_mode.as_text().to_string(),
        )),
        FigureProperty::PaperOrientation => Ok(Value::CharArray(
            figure.paper_orientation.as_text().to_string(),
        )),
    }
}

fn paper_size_value(figure: &FigureState) -> Result<Value, RuntimeError> {
    vector_value(&paper_size_for_units(figure, figure.paper_units))
}

fn paper_position_value(figure: &FigureState) -> Result<Value, RuntimeError> {
    vector_value(&paper_position_for_units(figure, figure.paper_units))
}

fn vector_value(values: &[f64]) -> Result<Value, RuntimeError> {
    Ok(Value::Matrix(MatrixValue::new(
        1,
        values.len(),
        values.iter().copied().map(Value::Scalar).collect(),
    )?))
}

fn paper_size_for_units(figure: &FigureState, units: PaperUnits) -> [f64; 2] {
    match units {
        PaperUnits::Normalized => [1.0, 1.0],
        _ => [
            inches_to_units(figure.paper_size_in[0], units, figure.paper_size_in[0]),
            inches_to_units(figure.paper_size_in[1], units, figure.paper_size_in[1]),
        ],
    }
}

fn paper_position_for_units(figure: &FigureState, units: PaperUnits) -> [f64; 4] {
    let position = resolved_export_size_position_in(figure);
    match units {
        PaperUnits::Normalized => [
            if figure.paper_size_in[0] == 0.0 {
                0.0
            } else {
                position[0] / figure.paper_size_in[0]
            },
            if figure.paper_size_in[1] == 0.0 {
                0.0
            } else {
                position[1] / figure.paper_size_in[1]
            },
            if figure.paper_size_in[0] == 0.0 {
                0.0
            } else {
                position[2] / figure.paper_size_in[0]
            },
            if figure.paper_size_in[1] == 0.0 {
                0.0
            } else {
                position[3] / figure.paper_size_in[1]
            },
        ],
        _ => [
            inches_to_units(position[0], units, figure.paper_size_in[0]),
            inches_to_units(position[1], units, figure.paper_size_in[1]),
            inches_to_units(position[2], units, figure.paper_size_in[0]),
            inches_to_units(position[3], units, figure.paper_size_in[1]),
        ],
    }
}

fn inches_to_units(value_in: f64, units: PaperUnits, _reference_in: f64) -> f64 {
    match units {
        PaperUnits::Inches => value_in,
        PaperUnits::Centimeters => value_in * 2.54,
        PaperUnits::Points => value_in * 72.0,
        PaperUnits::Normalized => value_in,
    }
}

fn units_to_inches(value: f64, units: PaperUnits, reference_in: f64) -> f64 {
    match units {
        PaperUnits::Inches => value,
        PaperUnits::Centimeters => value / 2.54,
        PaperUnits::Points => value / 72.0,
        PaperUnits::Normalized => value * reference_in,
    }
}

fn axes_property_value(
    state: &GraphicsState,
    axes_handle: u32,
    property_axes: &AxesState,
    property: AxesProperty,
) -> Result<Value, RuntimeError> {
    match property {
        AxesProperty::Position => axes_position_value(state, axes_handle),
        AxesProperty::Title => Ok(Value::CharArray(property_axes.title.clone())),
        AxesProperty::XLabel => Ok(Value::CharArray(property_axes.xlabel.clone())),
        AxesProperty::YLabel => Ok(Value::CharArray(
            match property_axes.active_y_axis {
                YAxisSide::Left => property_axes.ylabel.clone(),
                YAxisSide::Right => property_axes.ylabel_right.clone(),
            },
        )),
        AxesProperty::ZLabel => Ok(Value::CharArray(property_axes.zlabel.clone())),
        AxesProperty::XScale => Ok(Value::CharArray(
            property_axes.x_scale.as_text().to_string(),
        )),
        AxesProperty::YScale => Ok(Value::CharArray(
            current_y_scale_for_side(property_axes, property_axes.active_y_axis)
                .as_text()
                .to_string(),
        )),
        AxesProperty::XLim => {
            let (lower, upper) = resolved_limits(property_axes).0;
            limit_value(lower, upper)
        }
        AxesProperty::YLim => {
            let (lower, upper) =
                resolved_y_limits_for_side(property_axes, property_axes.active_y_axis);
            limit_value(lower, upper)
        }
        AxesProperty::ZLim => {
            let (lower, upper) = resolved_z_limits(property_axes);
            limit_value(lower, upper)
        }
        AxesProperty::XTick => tick_values_value(&resolved_ticks(property_axes, TickKind::X)),
        AxesProperty::YTick => {
            tick_values_value(&resolved_ticks_active_side(property_axes, TickKind::Y))
        }
        AxesProperty::ZTick => tick_values_value(&resolved_ticks(property_axes, TickKind::Z)),
        AxesProperty::XTickLabel => {
            tick_labels_value(&resolved_tick_labels(property_axes, TickKind::X))
        }
        AxesProperty::YTickLabel => {
            tick_labels_value(&resolved_tick_labels_active_side(property_axes, TickKind::Y))
        }
        AxesProperty::ZTickLabel => {
            tick_labels_value(&resolved_tick_labels(property_axes, TickKind::Z))
        }
        AxesProperty::XTickAngle => Ok(Value::Scalar(property_axes.xtick_angle)),
        AxesProperty::YTickAngle => {
            Ok(Value::Scalar(resolved_tick_angle_active_side(property_axes, TickKind::Y)))
        }
        AxesProperty::ZTickAngle => Ok(Value::Scalar(property_axes.ztick_angle)),
        AxesProperty::Visible => Ok(on_off_value(property_axes.axis_visible)),
        AxesProperty::Box => Ok(on_off_value(property_axes.box_enabled)),
        AxesProperty::Grid => Ok(on_off_value(property_axes.grid_enabled)),
        AxesProperty::Hold => Ok(on_off_value(property_axes.hold_enabled)),
        AxesProperty::View => view_value(property_axes),
        AxesProperty::CLim => {
            let (lower, upper) = effective_caxis(property_axes);
            limit_value(lower, upper)
        }
        AxesProperty::Colormap => Ok(Value::CharArray(
            property_axes.colormap.as_text().to_string(),
        )),
        AxesProperty::Colorbar => Ok(on_off_value(property_axes.colorbar_enabled)),
        AxesProperty::Legend => match &property_axes.legend {
            Some(labels) => tick_labels_value(labels),
            None => empty_matrix_value(),
        },
        AxesProperty::Type => Ok(Value::CharArray("axes".to_string())),
        AxesProperty::Parent => {
            let (figure_handle, _) =
                axes_location_by_handle(state, axes_handle).ok_or_else(|| {
                    RuntimeError::MissingVariable(format!(
                        "axes handle `{axes_handle}` does not exist"
                    ))
                })?;
            Ok(Value::Scalar(figure_handle as f64))
        }
        AxesProperty::Children => {
            let slot = axes_slot_by_handle(state, axes_handle)?;
            graphics_handle_vector_value(axes_children_handles(slot))
        }
    }
}

fn series_property_value(
    state: &GraphicsState,
    series_handle: u32,
    series: &PlotSeries,
    property: SeriesProperty,
) -> Result<Value, RuntimeError> {
    match property {
        SeriesProperty::Type => Ok(Value::CharArray(
            series.kind.property_type_name().to_string(),
        )),
        SeriesProperty::XData => {
            if series.image.is_some() {
                image_axis_property_value(series, SeriesAxis::X)
            } else {
                numeric_series_property_value(series, SeriesAxis::X)
            }
        }
        SeriesProperty::YData => {
            if series.image.is_some() {
                image_axis_property_value(series, SeriesAxis::Y)
            } else {
                numeric_series_property_value(series, SeriesAxis::Y)
            }
        }
        SeriesProperty::ZData => numeric_series_property_value(series, SeriesAxis::Z),
        SeriesProperty::AlphaData => image_alpha_data_value(series),
        SeriesProperty::AlphaDataMapping => image_alpha_data_mapping_value(series),
        SeriesProperty::CData => {
            if series.scatter.is_some() {
                scatter_cdata_value(series)
            } else {
                image_cdata_value(series)
            }
        }
        SeriesProperty::CDataMapping => image_cdata_mapping_value(series),
        SeriesProperty::DisplayName => Ok(Value::CharArray(
            series.display_name.clone().unwrap_or_default(),
        )),
        SeriesProperty::Visible => Ok(on_off_value(series.visible)),
        SeriesProperty::Parent => {
            let (figure_handle, axes_index, _) = series_location_by_handle(state, series_handle)
                .ok_or_else(|| {
                    RuntimeError::MissingVariable(format!(
                        "graphics series handle `{series_handle}` does not exist"
                    ))
                })?;
            let axes_handle = state
                .figures
                .get(&figure_handle)
                .expect("figure should exist")
                .axes
                .get(&axes_index)
                .expect("axes should exist")
                .handle;
            Ok(Value::Scalar(axes_handle as f64))
        }
        SeriesProperty::String => {
            let text = series.text.as_ref().ok_or_else(|| {
                RuntimeError::Unsupported(
                    "the selected graphics series does not support the `String` property"
                        .to_string(),
                )
            })?;
            Ok(Value::CharArray(text.label.clone()))
        }
        SeriesProperty::Position => {
            if let Some(text) = &series.text {
                Ok(Value::Matrix(MatrixValue::new(
                    1,
                    2,
                    vec![Value::Scalar(text.x), Value::Scalar(text.y)],
                )?))
            } else if let Some(rectangle) = &series.rectangle {
                Ok(Value::Matrix(MatrixValue::new(
                    1,
                    4,
                    vec![
                        Value::Scalar(rectangle.x),
                        Value::Scalar(rectangle.y),
                        Value::Scalar(rectangle.width),
                        Value::Scalar(rectangle.height),
                    ],
                )?))
            } else {
                Err(RuntimeError::Unsupported(
                    "the selected graphics series does not support the `Position` property"
                        .to_string(),
                ))
            }
        }
        SeriesProperty::Color => color_property_value(series.color),
        SeriesProperty::LineWidth => Ok(Value::Scalar(series.line_width)),
        SeriesProperty::LineStyle => Ok(Value::CharArray(series.line_style.as_text().to_string())),
        SeriesProperty::Marker => Ok(Value::CharArray(series.marker.as_text().to_string())),
        SeriesProperty::MarkerSize => Ok(Value::Scalar(series.marker_size)),
        SeriesProperty::MaximumNumPoints => match series.maximum_num_points {
            Some(value) => Ok(Value::Scalar(value as f64)),
            None => Ok(Value::Matrix(MatrixValue::new(0, 0, Vec::new())?)),
        },
        SeriesProperty::SizeData => scatter_size_data_value(series),
        SeriesProperty::MarkerEdgeColor => series.marker_edge_color.property_value(),
        SeriesProperty::MarkerFaceColor => series.marker_face_color.property_value(),
        SeriesProperty::EdgeColor => color_property_value(series.color),
        SeriesProperty::FaceColor => {
            let face_color = if let Some(rectangle) = &series.rectangle {
                rectangle.face_color
            } else if let Some(patch) = &series.patch {
                patch.face_color
            } else {
                return Err(RuntimeError::Unsupported(
                    "the selected graphics series does not support the `FaceColor` property"
                        .to_string(),
                ));
            };
            match face_color {
                Some(color) => color_property_value(color),
                None => Ok(Value::CharArray("none".to_string())),
            }
        }
    }
}

fn annotation_property_value(
    state: &GraphicsState,
    annotation_handle: u32,
    annotation: &AnnotationObject,
    property: AnnotationProperty,
) -> Result<Value, RuntimeError> {
    match property {
        AnnotationProperty::Type => Ok(Value::CharArray(annotation.kind.type_name().to_string())),
        AnnotationProperty::Parent => {
            let (figure_handle, _) = annotation_location_by_handle(state, annotation_handle)
                .ok_or_else(|| {
                    RuntimeError::MissingVariable(format!(
                        "graphics annotation handle `{annotation_handle}` does not exist"
                    ))
                })?;
            Ok(Value::Scalar(figure_handle as f64))
        }
        AnnotationProperty::Position => {
            if let Some(position) = annotation.position {
                Ok(Value::Matrix(MatrixValue::new(
                    1,
                    4,
                    position.into_iter().map(Value::Scalar).collect(),
                )?))
            } else {
                Ok(Value::Matrix(MatrixValue::new(
                    1,
                    4,
                    vec![
                        Value::Scalar(*annotation.x.get(0).unwrap_or(&0.3)),
                        Value::Scalar(*annotation.y.get(0).unwrap_or(&0.3)),
                        Value::Scalar(
                            annotation.x.get(1).copied().unwrap_or(0.4)
                                - annotation.x.get(0).copied().unwrap_or(0.3),
                        ),
                        Value::Scalar(
                            annotation.y.get(1).copied().unwrap_or(0.4)
                                - annotation.y.get(0).copied().unwrap_or(0.3),
                        ),
                    ],
                )?))
            }
        }
        AnnotationProperty::String => Ok(Value::CharArray(annotation.text.clone())),
        AnnotationProperty::Color => color_property_value(annotation.color),
        AnnotationProperty::LineWidth => Ok(Value::Scalar(annotation.line_width)),
        AnnotationProperty::LineStyle => Ok(Value::CharArray(
            annotation.line_style.as_text().to_string(),
        )),
        AnnotationProperty::FaceColor => match annotation.face_color {
            Some(color) => color_property_value(color),
            None => Ok(Value::CharArray("none".to_string())),
        },
        AnnotationProperty::Visible => Ok(on_off_value(annotation.visible)),
        AnnotationProperty::FontSize => Ok(Value::Scalar(annotation.font_size)),
    }
}

fn set_annotation_property(
    annotation: &mut AnnotationObject,
    property: AnnotationProperty,
    value: &Value,
) -> Result<(), RuntimeError> {
    match property {
        AnnotationProperty::Type | AnnotationProperty::Parent => {
            return Err(RuntimeError::Unsupported(
                "set currently treats the annotation `Type` and `Parent` properties as read-only"
                    .to_string(),
            ))
        }
        AnnotationProperty::Position => {
            let values = numeric_vector(value, "set")?;
            if values.len() != 4 {
                return Err(RuntimeError::ShapeError(
                    "set currently expects annotation `Position` as a numeric 1x4 vector"
                        .to_string(),
                ));
            }
            annotation.position = Some([values[0], values[1], values[2], values[3]]);
            if matches!(
                annotation.kind,
                AnnotationKind::Line
                    | AnnotationKind::Arrow
                    | AnnotationKind::DoubleArrow
                    | AnnotationKind::TextArrow
            ) {
                annotation.x = vec![values[0], values[0] + values[2]];
                annotation.y = vec![values[1], values[1] + values[3]];
            }
        }
        AnnotationProperty::String => annotation.text = text_arg(value, "set")?,
        AnnotationProperty::Color => annotation.color = parse_graphics_color_input(value, "set")?,
        AnnotationProperty::LineWidth => annotation.line_width = finite_scalar_arg(value, "set")?,
        AnnotationProperty::LineStyle => annotation.line_style = parse_line_style(value, "set")?,
        AnnotationProperty::FaceColor => {
            annotation.face_color = if is_text_keyword(value, "none")? {
                None
            } else {
                Some(parse_graphics_color_input(value, "set")?)
            };
        }
        AnnotationProperty::Visible => annotation.visible = on_off_flag(value, "set")?,
        AnnotationProperty::FontSize => annotation.font_size = finite_scalar_arg(value, "set")?,
    }
    Ok(())
}

fn set_axes_property(
    axes: &mut AxesState,
    property: AxesProperty,
    value: &Value,
) -> Result<(), RuntimeError> {
    match property {
        AxesProperty::Position => {
            let values = numeric_vector(value, "set")?;
            if values.len() != 4 {
                return Err(RuntimeError::ShapeError(
                    "set currently expects axes `Position` as a numeric 1x4 vector".to_string(),
                ));
            }
            axes.position = Some([values[0], values[1], values[2], values[3]]);
        }
        AxesProperty::Title => axes.title = text_arg(value, "set")?,
        AxesProperty::XLabel => axes.xlabel = text_arg(value, "set")?,
        AxesProperty::YLabel => match axes.active_y_axis {
            YAxisSide::Left => axes.ylabel = text_arg(value, "set")?,
            YAxisSide::Right => axes.ylabel_right = text_arg(value, "set")?,
        },
        AxesProperty::ZLabel => axes.zlabel = text_arg(value, "set")?,
        AxesProperty::XScale => axes.x_scale = parse_axis_scale(value, "set")?,
        AxesProperty::YScale => {
            *current_y_scale_mut(axes, axes.active_y_axis) = parse_axis_scale(value, "set")?
        }
        AxesProperty::XLim => axes.xlim = Some(numeric_limit_pair(value, "set")?),
        AxesProperty::YLim => *current_y_limit_mut(axes, axes.active_y_axis) = Some(numeric_limit_pair(value, "set")?),
        AxesProperty::ZLim => axes.zlim = Some(numeric_limit_pair(value, "set")?),
        AxesProperty::XTick => {
            axes.xticks = Some(tick_vector(value, "set")?);
            let tick_count = axes.xticks.as_ref().map(|ticks| ticks.len()).unwrap_or(0);
            sync_tick_label_override(axes, TickKind::X, tick_count);
        }
        AxesProperty::YTick => {
            *current_y_ticks_mut(axes, axes.active_y_axis) = Some(tick_vector(value, "set")?);
            let tick_count = current_y_ticks_mut(axes, axes.active_y_axis)
                .as_ref()
                .map(|ticks| ticks.len())
                .unwrap_or(0);
            sync_tick_label_override(axes, TickKind::Y, tick_count);
        }
        AxesProperty::ZTick => {
            axes.zticks = Some(tick_vector(value, "set")?);
            let tick_count = axes.zticks.as_ref().map(|ticks| ticks.len()).unwrap_or(0);
            sync_tick_label_override(axes, TickKind::Z, tick_count);
        }
        AxesProperty::XTickLabel => set_tick_labels_property(axes, TickKind::X, value)?,
        AxesProperty::YTickLabel => set_tick_labels_property(axes, TickKind::Y, value)?,
        AxesProperty::ZTickLabel => set_tick_labels_property(axes, TickKind::Z, value)?,
        AxesProperty::XTickAngle => axes.xtick_angle = finite_scalar_arg(value, "set")?,
        AxesProperty::YTickAngle => {
            *current_y_tick_angle_mut(axes, axes.active_y_axis) = finite_scalar_arg(value, "set")?
        }
        AxesProperty::ZTickAngle => axes.ztick_angle = finite_scalar_arg(value, "set")?,
        AxesProperty::Visible => axes.axis_visible = on_off_flag(value, "set")?,
        AxesProperty::Box => axes.box_enabled = on_off_flag(value, "set")?,
        AxesProperty::Grid => axes.grid_enabled = on_off_flag(value, "set")?,
        AxesProperty::Hold => axes.hold_enabled = on_off_flag(value, "set")?,
        AxesProperty::View => {
            let values = numeric_vector(value, "set")?;
            if values.len() != 2 {
                return Err(RuntimeError::ShapeError(
                    "set currently expects the axes `View` property to be a 1x2 numeric vector"
                        .to_string(),
                ));
            }
            axes.view_azimuth = values[0];
            axes.view_elevation = values[1];
        }
        AxesProperty::CLim => axes.caxis = Some(numeric_limit_pair(value, "set")?),
        AxesProperty::Colormap => axes.colormap = parse_colormap_kind(value)?,
        AxesProperty::Colorbar => axes.colorbar_enabled = on_off_flag(value, "set")?,
        AxesProperty::Legend => {
            axes.legend = if is_text_keyword(value, "off")? {
                None
            } else {
                Some(text_labels_from_value(value, "set")?)
            };
        }
        AxesProperty::Type | AxesProperty::Parent | AxesProperty::Children => {
            return Err(RuntimeError::Unsupported(
                "set currently treats this axes property as read-only".to_string(),
            ));
        }
    }

    Ok(())
}

fn set_series_property(
    series: &mut PlotSeries,
    property: SeriesProperty,
    value: &Value,
) -> Result<(), RuntimeError> {
    match property {
        SeriesProperty::Type => {
            return Err(RuntimeError::Unsupported(
                "set currently treats the graphics series `Type` property as read-only".to_string(),
            ))
        }
        SeriesProperty::XData => {
            if series.image.is_some() {
                set_image_axis_data(series, SeriesAxis::X, value)?
            } else {
                set_numeric_series_axis(series, SeriesAxis::X, value)?
            }
        }
        SeriesProperty::YData => {
            if series.image.is_some() {
                set_image_axis_data(series, SeriesAxis::Y, value)?
            } else {
                set_numeric_series_axis(series, SeriesAxis::Y, value)?
            }
        }
        SeriesProperty::ZData => set_numeric_series_axis(series, SeriesAxis::Z, value)?,
        SeriesProperty::AlphaData => set_image_alpha_data(series, value)?,
        SeriesProperty::AlphaDataMapping => set_image_alpha_data_mapping(series, value)?,
        SeriesProperty::CData => {
            if series.scatter.is_some() {
                set_scatter_cdata(series, value)?
            } else {
                set_image_cdata(series, value)?
            }
        }
        SeriesProperty::CDataMapping => set_image_cdata_mapping(series, value)?,
        SeriesProperty::DisplayName => {
            series.display_name = Some(text_arg(value, "set")?);
        }
        SeriesProperty::Visible => {
            series.visible = on_off_flag(value, "set")?;
        }
        SeriesProperty::Parent => {
            return Err(RuntimeError::Unsupported(
                "set currently treats the graphics series `Parent` property as read-only"
                    .to_string(),
            ))
        }
        SeriesProperty::String => {
            let text = series.text.as_mut().ok_or_else(|| {
                RuntimeError::Unsupported(
                    "the selected graphics series does not support setting the `String` property"
                        .to_string(),
                )
            })?;
            text.label = text_arg(value, "set")?;
        }
        SeriesProperty::Position => {
            let values = numeric_vector(value, "set")?;
            if let Some(text) = series.text.as_mut() {
                if values.len() != 2 {
                    return Err(RuntimeError::ShapeError(
                        "set currently expects text `Position` to be a numeric 1x2 vector"
                            .to_string(),
                    ));
                }
                text.x = values[0];
                text.y = values[1];
            } else if let Some(rectangle) = series.rectangle.as_mut() {
                if values.len() != 4 {
                    return Err(RuntimeError::ShapeError(
                        "set currently expects rectangle `Position` to be a numeric 1x4 vector"
                            .to_string(),
                    ));
                }
                rectangle.x = values[0];
                rectangle.y = values[1];
                rectangle.width = values[2];
                rectangle.height = values[3];
            } else {
                return Err(RuntimeError::Unsupported(
                    "the selected graphics series does not support setting the `Position` property"
                        .to_string(),
                ));
            }
        }
        SeriesProperty::Color => {
            series.color = parse_graphics_color_input(value, "set")?;
            if let Some(scatter) = series.scatter.as_mut() {
                scatter.colors = ScatterColors::Uniform(series.color);
                scatter.uses_default_color = false;
            }
        }
        SeriesProperty::LineWidth => {
            series.line_width = finite_scalar_arg(value, "set")?;
        }
        SeriesProperty::LineStyle => {
            series.line_style = parse_line_style(value, "set")?;
        }
        SeriesProperty::Marker => {
            series.marker = parse_marker_style(value, "set")?;
        }
        SeriesProperty::MarkerSize => {
            series.marker_size = finite_scalar_arg(value, "set")?;
            if let Some(scatter) = series.scatter.as_mut() {
                scatter.marker_sizes = vec![series.marker_size; scatter.marker_sizes.len()];
            }
        }
        SeriesProperty::MaximumNumPoints => {
            series.maximum_num_points = if matches!(value, Value::Matrix(matrix) if matrix.elements.is_empty())
            {
                None
            } else {
                Some(scalar_usize(value, "set")?)
            };
            trim_series_to_maximum_num_points(series);
        }
        SeriesProperty::SizeData => set_scatter_size_data(series, value)?,
        SeriesProperty::MarkerEdgeColor => {
            series.marker_edge_color = parse_marker_color_input(value, "set")?;
        }
        SeriesProperty::MarkerFaceColor => {
            series.marker_face_color = parse_marker_color_input(value, "set")?;
        }
        SeriesProperty::EdgeColor => {
            series.color = parse_graphics_color_input(value, "set")?;
        }
        SeriesProperty::FaceColor => {
            let next_face_color = if is_text_keyword(value, "none")? {
                None
            } else {
                Some(parse_graphics_color_input(value, "set")?)
            };
            if let Some(rectangle) = series.rectangle.as_mut() {
                rectangle.face_color = next_face_color;
            } else if let Some(patch) = series.patch.as_mut() {
                patch.face_color = next_face_color;
            } else {
                return Err(RuntimeError::Unsupported(
                    "the selected graphics series does not support setting the `FaceColor` property"
                        .to_string(),
                ));
            }
        }
    }

    Ok(())
}

fn set_tick_labels_property(
    axes: &mut AxesState,
    kind: TickKind,
    value: &Value,
) -> Result<(), RuntimeError> {
    let labels = text_labels_from_value(value, "set")?;
    let tick_count = resolved_ticks_active_side(axes, kind).len();
    if !labels.is_empty() && labels.len() != tick_count {
        return Err(RuntimeError::ShapeError(format!(
            "set currently expects {} labels for the selected {}TickLabel property, found {}",
            tick_count,
            kind.axis_name(),
            labels.len()
        )));
    }

    match kind {
        TickKind::X => axes.xtick_labels = Some(labels),
        TickKind::Y => *current_y_tick_labels_mut(axes, axes.active_y_axis) = Some(labels),
        TickKind::Z => axes.ztick_labels = Some(labels),
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SeriesAxis {
    X,
    Y,
    Z,
}

impl SeriesAxis {
    fn property_name(self) -> &'static str {
        match self {
            Self::X => "XData",
            Self::Y => "YData",
            Self::Z => "ZData",
        }
    }
}

fn numeric_series_property_value(
    series: &PlotSeries,
    axis: SeriesAxis,
) -> Result<Value, RuntimeError> {
    let values = series_axis_values(series, axis)?;
    tick_values_value(&values)
}

fn image_cdata_value(series: &PlotSeries) -> Result<Value, RuntimeError> {
    let image = series.image.as_ref().ok_or_else(|| {
        RuntimeError::Unsupported(
            "the selected graphics series does not support the `CData` property".to_string(),
        )
    })?;
    if let Some(rgb_values) = &image.rgb_values {
        let elements = rgb_values
            .iter()
            .flat_map(|pixel| {
                [
                    Value::Scalar(pixel[0]),
                    Value::Scalar(pixel[1]),
                    Value::Scalar(pixel[2]),
                ]
            })
            .collect::<Vec<_>>();
        return Ok(Value::Matrix(MatrixValue::with_dimensions(
            image.rows,
            image.cols * 3,
            vec![image.rows, image.cols, 3],
            elements,
        )?));
    }
    Ok(Value::Matrix(MatrixValue::new(
        image.rows,
        image.cols,
        image.values.iter().copied().map(Value::Scalar).collect(),
    )?))
}

fn image_alpha_data_value(series: &PlotSeries) -> Result<Value, RuntimeError> {
    let image = series.image.as_ref().ok_or_else(|| {
        RuntimeError::Unsupported(
            "the selected graphics series does not support the `AlphaData` property".to_string(),
        )
    })?;
    match &image.alpha_data {
        ImageAlphaData::Scalar(alpha) => Ok(Value::Scalar(*alpha)),
        ImageAlphaData::Matrix(values) => Ok(Value::Matrix(MatrixValue::new(
            image.rows,
            image.cols,
            values.iter().copied().map(Value::Scalar).collect(),
        )?)),
    }
}

fn scatter_cdata_value(series: &PlotSeries) -> Result<Value, RuntimeError> {
    let scatter = series.scatter.as_ref().ok_or_else(|| {
        RuntimeError::Unsupported(
            "the selected graphics series does not support the `CData` property".to_string(),
        )
    })?;
    match &scatter.colors {
        ScatterColors::Uniform(color) => color_property_value(color),
        ScatterColors::Colormapped(values) => {
            if values
                .iter()
                .all(|value| (*value - values[0]).abs() <= f64::EPSILON)
            {
                Ok(Value::Scalar(values[0]))
            } else {
                tick_values_value(values)
            }
        }
        ScatterColors::Rgb(colors) => {
            if colors.iter().skip(1).all(|color| {
                (color[0] - colors[0][0]).abs() <= f64::EPSILON
                    && (color[1] - colors[0][1]).abs() <= f64::EPSILON
                    && (color[2] - colors[0][2]).abs() <= f64::EPSILON
            }) {
                return Ok(Value::Matrix(MatrixValue::new(
                    1,
                    3,
                    colors[0].into_iter().map(Value::Scalar).collect(),
                )?));
            }
            let elements = colors
                .iter()
                .flat_map(|rgb| rgb.iter().copied().map(Value::Scalar))
                .collect::<Vec<_>>();
            Ok(Value::Matrix(MatrixValue::new(colors.len(), 3, elements)?))
        }
    }
}

fn scatter_size_data_value(series: &PlotSeries) -> Result<Value, RuntimeError> {
    let scatter = series.scatter.as_ref().ok_or_else(|| {
        RuntimeError::Unsupported(
            "the selected graphics series does not support the `SizeData` property".to_string(),
        )
    })?;
    let numeric_values = scatter
        .marker_sizes
        .iter()
        .map(|size| size * size)
        .collect::<Vec<_>>();
    if numeric_values.len() == 1
        || numeric_values
            .iter()
            .skip(1)
            .all(|value| (*value - numeric_values[0]).abs() <= f64::EPSILON)
    {
        Ok(Value::Scalar(numeric_values[0]))
    } else {
        Ok(Value::Matrix(MatrixValue::new(
            1,
            numeric_values.len(),
            numeric_values.into_iter().map(Value::Scalar).collect(),
        )?))
    }
}

fn trim_series_to_maximum_num_points(series: &mut PlotSeries) {
    let Some(limit) = series.maximum_num_points else {
        return;
    };
    if limit == 0 {
        series.x.clear();
        series.y.clear();
        if let Some(three_d) = series.three_d.as_mut() {
            three_d.points.clear();
            *three_d = match series.kind {
                SeriesKind::Stem3D => stem3_series_from_points(Vec::new()),
                _ => three_d_series_from_points(Vec::new()),
            };
        }
        if let Some(scatter) = series.scatter.as_mut() {
            scatter.marker_sizes.clear();
            match &mut scatter.colors {
                ScatterColors::Colormapped(values) => values.clear(),
                ScatterColors::Rgb(colors) => colors.clear(),
                ScatterColors::Uniform(_) => {}
            }
        }
        return;
    }

    if let Some(three_d) = series.three_d.as_mut() {
        if three_d.points.len() > limit {
            let drop = three_d.points.len() - limit;
            three_d.points.drain(0..drop);
            *three_d = match series.kind {
                SeriesKind::Stem3D => stem3_series_from_points(three_d.points.clone()),
                _ => three_d_series_from_points(three_d.points.clone()),
            };
        }
    }

    if series.x.len() > limit {
        let drop = series.x.len() - limit;
        series.x.drain(0..drop);
        series.y.drain(0..drop);
    }

    if let Some(scatter) = series.scatter.as_mut() {
        if scatter.marker_sizes.len() > limit {
            let drop = scatter.marker_sizes.len() - limit;
            scatter.marker_sizes.drain(0..drop);
        }
        match &mut scatter.colors {
            ScatterColors::Colormapped(values) if values.len() > limit => {
                let drop = values.len() - limit;
                values.drain(0..drop);
            }
            ScatterColors::Rgb(colors) if colors.len() > limit => {
                let drop = colors.len() - limit;
                colors.drain(0..drop);
            }
            _ => {}
        }
    }
}

fn image_alpha_data_mapping_value(series: &PlotSeries) -> Result<Value, RuntimeError> {
    let image = series.image.as_ref().ok_or_else(|| {
        RuntimeError::Unsupported(
            "the selected graphics series does not support the `AlphaDataMapping` property"
                .to_string(),
        )
    })?;
    let mapping = match image.alpha_mapping {
        AlphaDataMapping::None => "none",
    };
    Ok(Value::CharArray(mapping.to_string()))
}

fn image_cdata_mapping_value(series: &PlotSeries) -> Result<Value, RuntimeError> {
    let image = series.image.as_ref().ok_or_else(|| {
        RuntimeError::Unsupported(
            "the selected graphics series does not support the `CDataMapping` property".to_string(),
        )
    })?;
    let mapping = if image.rgb_values.is_some() {
        "direct"
    } else {
        match image.mapping {
            ImageMapping::Scaled => "scaled",
            ImageMapping::Direct => "direct",
        }
    };
    Ok(Value::CharArray(mapping.to_string()))
}

fn image_axis_property_value(series: &PlotSeries, axis: SeriesAxis) -> Result<Value, RuntimeError> {
    let image = series.image.as_ref().ok_or_else(|| {
        RuntimeError::Unsupported(
            "the selected graphics series does not support this axis data property".to_string(),
        )
    })?;
    let values = match axis {
        SeriesAxis::X => &image.x_data,
        SeriesAxis::Y => &image.y_data,
        SeriesAxis::Z => {
            return Err(RuntimeError::Unsupported(
                "image graphics series do not support the `ZData` property".to_string(),
            ))
        }
    };
    tick_values_value(values)
}

fn series_axis_values(series: &PlotSeries, axis: SeriesAxis) -> Result<Vec<f64>, RuntimeError> {
    match (series.kind, axis) {
        (
            SeriesKind::Line
            | SeriesKind::Scatter
            | SeriesKind::Area
            | SeriesKind::Stairs
            | SeriesKind::Bar
            | SeriesKind::BarHorizontal
            | SeriesKind::Stem,
            SeriesAxis::X,
        ) => Ok(series.x.clone()),
        (
            SeriesKind::Line
            | SeriesKind::Scatter
            | SeriesKind::Area
            | SeriesKind::Stairs
            | SeriesKind::Bar
            | SeriesKind::BarHorizontal
            | SeriesKind::Stem,
            SeriesAxis::Y,
        ) => Ok(series.y.clone()),
        (SeriesKind::Line3D | SeriesKind::Scatter3D | SeriesKind::Stem3D, axis) => {
            let three_d = series.three_d.as_ref().ok_or_else(|| {
                RuntimeError::Unsupported(
                    "graphics series is missing the expected 3-D point data".to_string(),
                )
            })?;
            Ok(three_d
                .points
                .iter()
                .map(|(x, y, z)| match axis {
                    SeriesAxis::X => *x,
                    SeriesAxis::Y => *y,
                    SeriesAxis::Z => *z,
                })
                .collect())
        }
        _ => Err(RuntimeError::Unsupported(format!(
            "the `{}` graphics series type does not currently support the requested data property",
            series.kind.property_type_name()
        ))),
    }
}

fn set_numeric_series_axis(
    series: &mut PlotSeries,
    axis: SeriesAxis,
    value: &Value,
) -> Result<(), RuntimeError> {
    let values = numeric_vector(value, "set")?;
    match (series.kind, axis) {
        (
            SeriesKind::Line
            | SeriesKind::Scatter
            | SeriesKind::Area
            | SeriesKind::Stairs
            | SeriesKind::Bar
            | SeriesKind::BarHorizontal
            | SeriesKind::Stem,
            SeriesAxis::X,
        ) => {
            if values.len() != series.y.len() {
                return Err(RuntimeError::ShapeError(format!(
                    "set currently expects XData to match the existing YData length {}, found {}",
                    series.y.len(),
                    values.len()
                )));
            }
            series.x = values;
        }
        (
            SeriesKind::Line
            | SeriesKind::Scatter
            | SeriesKind::Area
            | SeriesKind::Stairs
            | SeriesKind::Bar
            | SeriesKind::BarHorizontal
            | SeriesKind::Stem,
            SeriesAxis::Y,
        ) => {
            if values.len() != series.x.len() {
                return Err(RuntimeError::ShapeError(format!(
                    "set currently expects YData to match the existing XData length {}, found {}",
                    series.x.len(),
                    values.len()
                )));
            }
            series.y = values;
        }
        (SeriesKind::Line3D | SeriesKind::Scatter3D | SeriesKind::Stem3D, axis) => {
            let mut x_values = series_axis_values(series, SeriesAxis::X)?;
            let mut y_values = series_axis_values(series, SeriesAxis::Y)?;
            let mut z_values = series_axis_values(series, SeriesAxis::Z)?;
            let expected_len = x_values.len();
            if values.len() != expected_len {
                return Err(RuntimeError::ShapeError(format!(
                    "set currently expects {}Data to match the existing point count {}, found {}",
                    match axis {
                        SeriesAxis::X => "X",
                        SeriesAxis::Y => "Y",
                        SeriesAxis::Z => "Z",
                    },
                    expected_len,
                    values.len()
                )));
            }
            match axis {
                SeriesAxis::X => x_values = values,
                SeriesAxis::Y => y_values = values,
                SeriesAxis::Z => z_values = values,
            }
            let points = x_values
                .into_iter()
                .zip(y_values)
                .zip(z_values)
                .map(|((x, y), z)| (x, y, z))
                .collect::<Vec<_>>();
            series.three_d = Some(match series.kind {
                SeriesKind::Stem3D => stem3_series_from_points(points),
                _ => three_d_series_from_points(points),
            });
        }
        _ => {
            return Err(RuntimeError::Unsupported(format!(
                "the `{}` graphics series type does not currently support setting the requested data property",
                series.kind.property_type_name()
            )))
        }
    }

    Ok(())
}

fn set_image_cdata(series: &mut PlotSeries, value: &Value) -> Result<(), RuntimeError> {
    let image = series.image.as_mut().ok_or_else(|| {
        RuntimeError::Unsupported(
            "the selected graphics series does not support setting the `CData` property"
                .to_string(),
        )
    })?;
    let old_rows = image.rows;
    let old_cols = image.cols;
    if let Some((rows, cols, rgb_values)) = rgb_image_matrix(value, "set")? {
        image.rows = rows;
        image.cols = cols;
        image.values.clear();
        image.rgb_values = Some(rgb_values);
        image.display_range = (0.0, 1.0);
        sync_image_alpha_data_extent(image, old_rows, old_cols);
        return Ok(());
    }

    let (rows, cols, values) = numeric_matrix(value, "set")?;
    image.rows = rows;
    image.cols = cols;
    image.rgb_values = None;
    image.values = values;
    image.display_range = match image.mapping {
        ImageMapping::Scaled => finite_min_max(&image.values),
        ImageMapping::Direct => (1.0, 8.0),
    };
    sync_image_alpha_data_extent(image, old_rows, old_cols);
    Ok(())
}

fn set_scatter_cdata(series: &mut PlotSeries, value: &Value) -> Result<(), RuntimeError> {
    let scatter = series.scatter.as_mut().ok_or_else(|| {
        RuntimeError::Unsupported(
            "the selected graphics series does not support setting the `CData` property"
                .to_string(),
        )
    })?;
    scatter.colors = parse_scatter_colors(value, series.x.len(), "set")?;
    scatter.uses_default_color = false;
    if let ScatterColors::Uniform(color) = scatter.colors {
        series.color = color;
    }
    Ok(())
}

fn set_scatter_size_data(series: &mut PlotSeries, value: &Value) -> Result<(), RuntimeError> {
    let point_count = series.x.len();
    let scatter = series.scatter.as_mut().ok_or_else(|| {
        RuntimeError::Unsupported(
            "the selected graphics series does not support setting the `SizeData` property"
                .to_string(),
        )
    })?;
    scatter.marker_sizes = parse_scatter_marker_sizes(value, point_count, "set")?;
    if let Some(size) = scatter.marker_sizes.first().copied() {
        series.marker_size = size;
    }
    Ok(())
}

fn set_image_alpha_data(series: &mut PlotSeries, value: &Value) -> Result<(), RuntimeError> {
    let image = series.image.as_mut().ok_or_else(|| {
        RuntimeError::Unsupported(
            "the selected graphics series does not support setting the `AlphaData` property"
                .to_string(),
        )
    })?;
    image.alpha_data = parse_image_alpha_data(value, image.rows, image.cols)?;
    Ok(())
}

fn set_image_alpha_data_mapping(
    series: &mut PlotSeries,
    value: &Value,
) -> Result<(), RuntimeError> {
    let image = series.image.as_mut().ok_or_else(|| {
        RuntimeError::Unsupported(
            "the selected graphics series does not support setting the `AlphaDataMapping` property"
                .to_string(),
        )
    })?;
    image.alpha_mapping = parse_alpha_data_mapping(value)?;
    Ok(())
}

fn set_image_cdata_mapping(series: &mut PlotSeries, value: &Value) -> Result<(), RuntimeError> {
    let image = series.image.as_mut().ok_or_else(|| {
        RuntimeError::Unsupported(
            "the selected graphics series does not support setting the `CDataMapping` property"
                .to_string(),
        )
    })?;
    if image.rgb_values.is_some() {
        return Err(RuntimeError::Unsupported(
            "truecolor image data does not currently support changing `CDataMapping`".to_string(),
        ));
    }
    image.mapping = parse_image_mapping(value)?;
    image.display_range = match image.mapping {
        ImageMapping::Scaled => finite_min_max(&image.values),
        ImageMapping::Direct => (1.0, 8.0),
    };
    Ok(())
}

fn set_image_axis_data(
    series: &mut PlotSeries,
    axis: SeriesAxis,
    value: &Value,
) -> Result<(), RuntimeError> {
    let image = series.image.as_mut().ok_or_else(|| {
        RuntimeError::Unsupported(
            "the selected graphics series does not support this axis data property".to_string(),
        )
    })?;
    let extent = match axis {
        SeriesAxis::X => image.cols,
        SeriesAxis::Y => image.rows,
        SeriesAxis::Z => {
            return Err(RuntimeError::Unsupported(
                "image graphics series do not support setting `ZData`".to_string(),
            ))
        }
    };
    let parsed = image_coordinate_vector(value, extent, axis.property_name(), "set")?;
    match axis {
        SeriesAxis::X => image.x_data = parsed,
        SeriesAxis::Y => image.y_data = parsed,
        SeriesAxis::Z => unreachable!("handled above"),
    }
    Ok(())
}

fn sync_image_alpha_data_extent(image: &mut ImageSeriesData, old_rows: usize, old_cols: usize) {
    if (image.rows != old_rows || image.cols != old_cols)
        && matches!(image.alpha_data, ImageAlphaData::Matrix(_))
    {
        image.alpha_data = ImageAlphaData::Scalar(1.0);
    }
}

#[derive(Debug, Clone)]
struct XySeriesInput {
    x: Vec<f64>,
    y: Vec<f64>,
}

#[derive(Debug, Clone)]
struct XySeriesGroup {
    series_inputs: Vec<XySeriesInput>,
    style: Option<LineSpecStyle>,
}

#[derive(Debug, Clone)]
struct XyzSeriesGroup {
    three_d: ThreeDSeriesData,
    style: Option<LineSpecStyle>,
}

impl ScatterSeriesData {
    fn default_color(&self) -> Option<&'static str> {
        if self.uses_default_color {
            return None;
        }
        match self.colors {
            ScatterColors::Uniform(color) => Some(color),
            ScatterColors::Colormapped(_) | ScatterColors::Rgb(_) => None,
        }
    }
}

fn numeric_vector(value: &Value, builtin_name: &str) -> Result<Vec<f64>, RuntimeError> {
    match value {
        Value::Scalar(number) => Ok(vec![*number]),
        Value::Logical(flag) => Ok(vec![if *flag { 1.0 } else { 0.0 }]),
        Value::Matrix(matrix) if matrix.rows == 1 || matrix.cols == 1 => matrix
            .iter()
            .map(Value::as_scalar)
            .collect::<Result<Vec<_>, _>>(),
        _ => Err(RuntimeError::TypeError(format!(
            "{builtin_name} currently expects scalar or vector numeric inputs"
        ))),
    }
}

fn parse_xy_series_args<'a>(
    args: &'a [Value],
    builtin_name: &str,
    allow_style: bool,
    allow_matrix_series: bool,
    allow_multiple_groups: bool,
) -> Result<(Vec<XySeriesGroup>, &'a [Value]), RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently supports `{builtin_name}(y)`, `{builtin_name}(x, y)`, and for line plots an optional trailing line-spec string plus current property/value pairs"
        )));
    }

    let mut groups = Vec::new();
    let mut next_index = 0usize;
    while next_index < args.len() {
        if !can_be_numeric_series_input(&args[next_index]) {
            break;
        }

        let single_data_form = args.get(next_index + 1).map_or(true, |next| {
            !can_be_numeric_series_input(next) || is_series_property_name(next)
        });
        let base_arg_count = if single_data_form { 1 } else { 2 };

        let series_inputs = if base_arg_count == 1 {
            if allow_matrix_series {
                expand_single_xy_series_input(&args[next_index], builtin_name)?
            } else {
                let y = numeric_vector(&args[next_index], builtin_name)?;
                let x = (1..=y.len()).map(|value| value as f64).collect::<Vec<_>>();
                vec![XySeriesInput { x, y }]
            }
        } else {
            expand_xy_series_inputs(
                &args[next_index],
                &args[next_index + 1],
                builtin_name,
                allow_matrix_series,
            )?
        };
        next_index += base_arg_count;

        let style = if allow_style
            && args.get(next_index).is_some_and(|candidate| {
                matches!(candidate, Value::CharArray(_) | Value::String(_))
                    && !is_series_property_name(candidate)
            }) {
            let style = parse_matlab_line_spec(&args[next_index], builtin_name)?;
            next_index += 1;
            Some(style)
        } else {
            None
        };

        groups.push(XySeriesGroup {
            series_inputs,
            style,
        });

        if !allow_multiple_groups {
            break;
        }
    }

    let property_pairs = &args[next_index..];
    if property_pairs.len() % 2 != 0 {
        return Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently expects trailing graphics properties as property/value pairs"
        )));
    }

    Ok((groups, property_pairs))
}

fn can_be_numeric_series_input(value: &Value) -> bool {
    matches!(
        value,
        Value::Scalar(_) | Value::Logical(_) | Value::Matrix(_)
    )
}

fn expand_single_xy_series_input(
    value: &Value,
    builtin_name: &str,
) -> Result<Vec<XySeriesInput>, RuntimeError> {
    let (rows, cols, values) = numeric_matrix(value, builtin_name)?;
    if rows == 0 || cols == 0 {
        return Ok(vec![XySeriesInput {
            x: Vec::new(),
            y: Vec::new(),
        }]);
    }
    if rows == 1 || cols == 1 {
        let x = (1..=values.len())
            .map(|value| value as f64)
            .collect::<Vec<_>>();
        return Ok(vec![XySeriesInput { x, y: values }]);
    }

    Ok((0..cols)
        .map(|col| XySeriesInput {
            x: (1..=rows).map(|value| value as f64).collect(),
            y: matrix_column_values(&values, rows, cols, col),
        })
        .collect())
}

fn expand_xy_series_inputs(
    x_value: &Value,
    y_value: &Value,
    builtin_name: &str,
    allow_matrix_series: bool,
) -> Result<Vec<XySeriesInput>, RuntimeError> {
    if !allow_matrix_series {
        return Ok(vec![XySeriesInput {
            x: numeric_vector(x_value, builtin_name)?,
            y: numeric_vector(y_value, builtin_name)?,
        }]);
    }

    let (x_rows, x_cols, x_values) = numeric_matrix(x_value, builtin_name)?;
    let (y_rows, y_cols, y_values) = numeric_matrix(y_value, builtin_name)?;
    let x_is_vector = x_rows == 1 || x_cols == 1;
    let y_is_vector = y_rows == 1 || y_cols == 1;

    if x_is_vector && y_is_vector {
        return Ok(vec![XySeriesInput {
            x: x_values,
            y: y_values,
        }]);
    }

    if x_rows == y_rows && x_cols == y_cols {
        if x_rows == 0 || x_cols == 0 {
            return Ok(vec![XySeriesInput {
                x: Vec::new(),
                y: Vec::new(),
            }]);
        }

        return Ok((0..x_cols)
            .map(|col| XySeriesInput {
                x: matrix_column_values(&x_values, x_rows, x_cols, col),
                y: matrix_column_values(&y_values, y_rows, y_cols, col),
            })
            .collect());
    }

    if x_is_vector && !y_is_vector {
        if x_values.len() == y_rows {
            return Ok((0..y_cols)
                .map(|col| XySeriesInput {
                    x: x_values.clone(),
                    y: matrix_column_values(&y_values, y_rows, y_cols, col),
                })
                .collect());
        }
        if x_values.len() == y_cols {
            return Ok((0..y_rows)
                .map(|row| XySeriesInput {
                    x: x_values.clone(),
                    y: matrix_row_values(&y_values, y_cols, row),
                })
                .collect());
        }
    }

    if !x_is_vector && y_is_vector {
        if y_values.len() == x_rows {
            return Ok((0..x_cols)
                .map(|col| XySeriesInput {
                    x: matrix_column_values(&x_values, x_rows, x_cols, col),
                    y: y_values.clone(),
                })
                .collect());
        }
        if y_values.len() == x_cols {
            return Ok((0..x_rows)
                .map(|row| XySeriesInput {
                    x: matrix_row_values(&x_values, x_cols, row),
                    y: y_values.clone(),
                })
                .collect());
        }
    }

    Err(RuntimeError::ShapeError(format!(
        "{builtin_name} currently requires vector/matrix inputs whose lengths line up or matrices with matching sizes"
    )))
}

fn matrix_column_values(values: &[f64], rows: usize, cols: usize, col: usize) -> Vec<f64> {
    (0..rows).map(|row| values[row * cols + col]).collect()
}

fn matrix_row_values(values: &[f64], cols: usize, row: usize) -> Vec<f64> {
    let start = row * cols;
    values[start..start + cols].to_vec()
}

fn is_series_property_name(value: &Value) -> bool {
    matches!(value, Value::CharArray(_) | Value::String(_))
        && parse_series_property(value, "plot").is_ok()
}

fn apply_series_property_pairs(
    series: &mut PlotSeries,
    property_pairs: &[Value],
    builtin_name: &str,
) -> Result<(), RuntimeError> {
    for pair in property_pairs.chunks_exact(2) {
        let property = parse_series_property(&pair[0], builtin_name)?;
        set_series_property(series, property, &pair[1])?;
    }
    Ok(())
}

fn parse_quiver_args(args: &[Value]) -> Result<QuiverSeriesData, RuntimeError> {
    let (base_x, base_y, u_values, v_values, scale_multiplier) = match args {
        [u, v] => {
            let (rows, cols, u_values) = numeric_matrix(u, "quiver")?;
            let (v_rows, v_cols, v_values) = numeric_matrix(v, "quiver")?;
            if rows != v_rows || cols != v_cols {
                return Err(RuntimeError::ShapeError(format!(
                    "quiver requires U and V inputs with matching shapes, found {}x{} and {}x{}",
                    rows, cols, v_rows, v_cols
                )));
            }
            let x = expand_surface_vector(
                &(1..=cols).map(|value| value as f64).collect::<Vec<_>>(),
                rows,
                cols,
                SurfaceAxis::X,
            );
            let y = expand_surface_vector(
                &(1..=rows).map(|value| value as f64).collect::<Vec<_>>(),
                rows,
                cols,
                SurfaceAxis::Y,
            );
            (x, y, u_values, v_values, None)
        }
        [u, v, scale] => {
            let (rows, cols, u_values) = numeric_matrix(u, "quiver")?;
            let (v_rows, v_cols, v_values) = numeric_matrix(v, "quiver")?;
            if rows != v_rows || cols != v_cols {
                return Err(RuntimeError::ShapeError(format!(
                    "quiver requires U and V inputs with matching shapes, found {}x{} and {}x{}",
                    rows, cols, v_rows, v_cols
                )));
            }
            let x = expand_surface_vector(
                &(1..=cols).map(|value| value as f64).collect::<Vec<_>>(),
                rows,
                cols,
                SurfaceAxis::X,
            );
            let y = expand_surface_vector(
                &(1..=rows).map(|value| value as f64).collect::<Vec<_>>(),
                rows,
                cols,
                SurfaceAxis::Y,
            );
            (x, y, u_values, v_values, Some(quiver_scale_arg(scale)?))
        }
        [x, y, u, v] => {
            let (rows, cols, u_values) = numeric_matrix(u, "quiver")?;
            let (v_rows, v_cols, v_values) = numeric_matrix(v, "quiver")?;
            if rows != v_rows || cols != v_cols {
                return Err(RuntimeError::ShapeError(format!(
                    "quiver requires U and V inputs with matching shapes, found {}x{} and {}x{}",
                    rows, cols, v_rows, v_cols
                )));
            }
            let (base_x, base_y) = explicit_quiver_coordinates(x, y, rows, cols)?;
            (base_x, base_y, u_values, v_values, None)
        }
        [x, y, u, v, scale] => {
            let (rows, cols, u_values) = numeric_matrix(u, "quiver")?;
            let (v_rows, v_cols, v_values) = numeric_matrix(v, "quiver")?;
            if rows != v_rows || cols != v_cols {
                return Err(RuntimeError::ShapeError(format!(
                    "quiver requires U and V inputs with matching shapes, found {}x{} and {}x{}",
                    rows, cols, v_rows, v_cols
                )));
            }
            let (base_x, base_y) = explicit_quiver_coordinates(x, y, rows, cols)?;
            (
                base_x,
                base_y,
                u_values,
                v_values,
                Some(quiver_scale_arg(scale)?),
            )
        }
        _ => {
            return Err(RuntimeError::Unsupported(
                "quiver currently supports `quiver(U, V)`, `quiver(U, V, scale)`, `quiver(X, Y, U, V)`, or `quiver(X, Y, U, V, scale)` with numeric vector or matrix inputs".to_string(),
            ))
        }
    };

    if base_x.len() != u_values.len()
        || base_y.len() != u_values.len()
        || v_values.len() != u_values.len()
    {
        return Err(RuntimeError::ShapeError(
            "quiver currently requires coordinate and vector inputs with matching element counts"
                .to_string(),
        ));
    }

    let effective_scale =
        quiver_effective_scale(&base_x, &base_y, &u_values, &v_values, scale_multiplier);
    let bases = base_x.into_iter().zip(base_y).collect::<Vec<_>>();
    let tips = bases
        .iter()
        .zip(u_values.iter().zip(&v_values))
        .map(|((x, y), (u, v))| (x + u * effective_scale, y + v * effective_scale))
        .collect::<Vec<_>>();
    Ok(QuiverSeriesData { bases, tips })
}

fn parse_quiver3_args(args: &[Value]) -> Result<ThreeDSeriesData, RuntimeError> {
    let (base_x, base_y, base_z, u_values, v_values, w_values, scale_multiplier) = match args {
        [u, v, w] => {
            let (rows, cols, u_values) = numeric_matrix(u, "quiver3")?;
            let (v_rows, v_cols, v_values) = numeric_matrix(v, "quiver3")?;
            let (w_rows, w_cols, w_values) = numeric_matrix(w, "quiver3")?;
            ensure_quiver3_field_shapes(rows, cols, v_rows, v_cols, w_rows, w_cols)?;
            (
                expand_surface_vector(
                    &(1..=cols).map(|value| value as f64).collect::<Vec<_>>(),
                    rows,
                    cols,
                    SurfaceAxis::X,
                ),
                expand_surface_vector(
                    &(1..=rows).map(|value| value as f64).collect::<Vec<_>>(),
                    rows,
                    cols,
                    SurfaceAxis::Y,
                ),
                vec![0.0; rows * cols],
                u_values,
                v_values,
                w_values,
                None,
            )
        }
        [u, v, w, scale] => {
            let (rows, cols, u_values) = numeric_matrix(u, "quiver3")?;
            let (v_rows, v_cols, v_values) = numeric_matrix(v, "quiver3")?;
            let (w_rows, w_cols, w_values) = numeric_matrix(w, "quiver3")?;
            ensure_quiver3_field_shapes(rows, cols, v_rows, v_cols, w_rows, w_cols)?;
            (
                expand_surface_vector(
                    &(1..=cols).map(|value| value as f64).collect::<Vec<_>>(),
                    rows,
                    cols,
                    SurfaceAxis::X,
                ),
                expand_surface_vector(
                    &(1..=rows).map(|value| value as f64).collect::<Vec<_>>(),
                    rows,
                    cols,
                    SurfaceAxis::Y,
                ),
                vec![0.0; rows * cols],
                u_values,
                v_values,
                w_values,
                Some(quiver_scale_arg(scale)?),
            )
        }
        [x, y, z, u, v, w] => {
            let (rows, cols, u_values) = numeric_matrix(u, "quiver3")?;
            let (v_rows, v_cols, v_values) = numeric_matrix(v, "quiver3")?;
            let (w_rows, w_cols, w_values) = numeric_matrix(w, "quiver3")?;
            ensure_quiver3_field_shapes(rows, cols, v_rows, v_cols, w_rows, w_cols)?;
            let (base_x, base_y, base_z) = explicit_quiver3_coordinates(x, y, z, rows, cols)?;
            (base_x, base_y, base_z, u_values, v_values, w_values, None)
        }
        [x, y, z, u, v, w, scale] => {
            let (rows, cols, u_values) = numeric_matrix(u, "quiver3")?;
            let (v_rows, v_cols, v_values) = numeric_matrix(v, "quiver3")?;
            let (w_rows, w_cols, w_values) = numeric_matrix(w, "quiver3")?;
            ensure_quiver3_field_shapes(rows, cols, v_rows, v_cols, w_rows, w_cols)?;
            let (base_x, base_y, base_z) = explicit_quiver3_coordinates(x, y, z, rows, cols)?;
            (
                base_x,
                base_y,
                base_z,
                u_values,
                v_values,
                w_values,
                Some(quiver_scale_arg(scale)?),
            )
        }
        _ => {
            return Err(RuntimeError::Unsupported(
                "quiver3 currently supports `quiver3(U, V, W)`, `quiver3(U, V, W, scale)`, `quiver3(X, Y, Z, U, V, W)`, or `quiver3(X, Y, Z, U, V, W, scale)` with numeric vector or matrix inputs".to_string(),
            ))
        }
    };

    if base_x.len() != u_values.len()
        || base_y.len() != u_values.len()
        || base_z.len() != u_values.len()
        || v_values.len() != u_values.len()
        || w_values.len() != u_values.len()
    {
        return Err(RuntimeError::ShapeError(
            "quiver3 currently requires coordinate and vector inputs with matching element counts"
                .to_string(),
        ));
    }

    let effective_scale = quiver3_effective_scale(
        &base_x,
        &base_y,
        &base_z,
        &u_values,
        &v_values,
        &w_values,
        scale_multiplier,
    );
    let mut points = Vec::with_capacity(base_x.len() * 2);
    for ((((base_x, base_y), base_z), u), (v, w)) in base_x
        .into_iter()
        .zip(base_y)
        .zip(base_z)
        .zip(u_values)
        .zip(v_values.into_iter().zip(w_values))
    {
        points.push((base_x, base_y, base_z));
        points.push((
            base_x + u * effective_scale,
            base_y + v * effective_scale,
            base_z + w * effective_scale,
        ));
    }
    Ok(three_d_series_from_points(points))
}

fn ensure_quiver3_field_shapes(
    rows: usize,
    cols: usize,
    v_rows: usize,
    v_cols: usize,
    w_rows: usize,
    w_cols: usize,
) -> Result<(), RuntimeError> {
    if rows != v_rows || cols != v_cols || rows != w_rows || cols != w_cols {
        return Err(RuntimeError::ShapeError(format!(
            "quiver3 requires U, V, and W inputs with matching shapes, found {}x{}, {}x{}, and {}x{}",
            rows, cols, v_rows, v_cols, w_rows, w_cols
        )));
    }
    Ok(())
}

fn explicit_quiver_coordinates(
    x: &Value,
    y: &Value,
    rows: usize,
    cols: usize,
) -> Result<(Vec<f64>, Vec<f64>), RuntimeError> {
    if rows == 1 || cols == 1 {
        let expected_len = rows * cols;
        let x_values = numeric_vector(x, "quiver")?;
        let y_values = numeric_vector(y, "quiver")?;
        if x_values.len() == expected_len && y_values.len() == expected_len {
            return Ok((x_values, y_values));
        }
    }

    Ok((
        quiver_coordinate_grid(x, rows, cols, SurfaceAxis::X)?,
        quiver_coordinate_grid(y, rows, cols, SurfaceAxis::Y)?,
    ))
}

fn explicit_quiver3_coordinates(
    x: &Value,
    y: &Value,
    z: &Value,
    rows: usize,
    cols: usize,
) -> Result<(Vec<f64>, Vec<f64>, Vec<f64>), RuntimeError> {
    if rows == 1 || cols == 1 {
        let expected_len = rows * cols;
        let x_values = numeric_vector(x, "quiver3")?;
        let y_values = numeric_vector(y, "quiver3")?;
        let z_values = numeric_vector(z, "quiver3")?;
        if x_values.len() == expected_len
            && y_values.len() == expected_len
            && z_values.len() == expected_len
        {
            return Ok((x_values, y_values, z_values));
        }
    }

    Ok((
        quiver3_xy_coordinate_grid(x, rows, cols, SurfaceAxis::X)?,
        quiver3_xy_coordinate_grid(y, rows, cols, SurfaceAxis::Y)?,
        quiver3_z_coordinate_grid(z, rows, cols)?,
    ))
}

fn quiver_coordinate_grid(
    value: &Value,
    rows: usize,
    cols: usize,
    axis: SurfaceAxis,
) -> Result<Vec<f64>, RuntimeError> {
    match value {
        Value::Scalar(number) => Ok(vec![*number; rows * cols]),
        Value::Logical(flag) => Ok(vec![if *flag { 1.0 } else { 0.0 }; rows * cols]),
        Value::Matrix(matrix) if matrix.rows == rows && matrix.cols == cols => matrix
            .iter()
            .map(Value::as_scalar)
            .collect::<Result<Vec<_>, _>>(),
        _ => {
            let vector = numeric_vector(value, "quiver")?;
            match axis {
                SurfaceAxis::X if vector.len() == cols => {
                    Ok(expand_surface_vector(&vector, rows, cols, axis))
                }
                SurfaceAxis::Y if vector.len() == rows => {
                    Ok(expand_surface_vector(&vector, rows, cols, axis))
                }
                SurfaceAxis::X => Err(RuntimeError::ShapeError(format!(
                    "quiver requires X coordinates to be a vector with {} elements or a {}x{} matrix",
                    cols, rows, cols
                ))),
                SurfaceAxis::Y => Err(RuntimeError::ShapeError(format!(
                    "quiver requires Y coordinates to be a vector with {} elements or a {}x{} matrix",
                    rows, rows, cols
                ))),
            }
        }
    }
}

fn quiver3_xy_coordinate_grid(
    value: &Value,
    rows: usize,
    cols: usize,
    axis: SurfaceAxis,
) -> Result<Vec<f64>, RuntimeError> {
    match value {
        Value::Scalar(number) => Ok(vec![*number; rows * cols]),
        Value::Logical(flag) => Ok(vec![if *flag { 1.0 } else { 0.0 }; rows * cols]),
        Value::Matrix(matrix) if matrix.rows == rows && matrix.cols == cols => matrix
            .iter()
            .map(Value::as_scalar)
            .collect::<Result<Vec<_>, _>>(),
        _ => {
            let vector = numeric_vector(value, "quiver3")?;
            match axis {
                SurfaceAxis::X if vector.len() == cols => {
                    Ok(expand_surface_vector(&vector, rows, cols, axis))
                }
                SurfaceAxis::Y if vector.len() == rows => {
                    Ok(expand_surface_vector(&vector, rows, cols, axis))
                }
                SurfaceAxis::X => Err(RuntimeError::ShapeError(format!(
                    "quiver3 requires X coordinates to be a vector with {} elements or a {}x{} matrix",
                    cols, rows, cols
                ))),
                SurfaceAxis::Y => Err(RuntimeError::ShapeError(format!(
                    "quiver3 requires Y coordinates to be a vector with {} elements or a {}x{} matrix",
                    rows, rows, cols
                ))),
            }
        }
    }
}

fn quiver3_z_coordinate_grid(
    value: &Value,
    rows: usize,
    cols: usize,
) -> Result<Vec<f64>, RuntimeError> {
    match value {
        Value::Scalar(number) => Ok(vec![*number; rows * cols]),
        Value::Logical(flag) => Ok(vec![if *flag { 1.0 } else { 0.0 }; rows * cols]),
        Value::Matrix(matrix) if matrix.rows == rows && matrix.cols == cols => matrix
            .iter()
            .map(Value::as_scalar)
            .collect::<Result<Vec<_>, _>>(),
        _ => {
            let vector = numeric_vector(value, "quiver3")?;
            if vector.len() == rows * cols {
                Ok(vector)
            } else {
                Err(RuntimeError::ShapeError(format!(
                    "quiver3 requires Z coordinates to be a scalar, a vector with {} elements, or a {}x{} matrix",
                    rows * cols,
                    rows,
                    cols
                )))
            }
        }
    }
}

fn quiver_scale_arg(value: &Value) -> Result<f64, RuntimeError> {
    let scale = value.as_scalar()?;
    if !scale.is_finite() || scale < 0.0 {
        return Err(RuntimeError::TypeError(
            "quiver and quiver3 currently expect a finite nonnegative numeric scale factor"
                .to_string(),
        ));
    }
    Ok(scale)
}

fn quiver_effective_scale(
    base_x: &[f64],
    base_y: &[f64],
    u_values: &[f64],
    v_values: &[f64],
    scale_multiplier: Option<f64>,
) -> f64 {
    let max_vector = u_values
        .iter()
        .zip(v_values)
        .map(|(u, v)| (u * u + v * v).sqrt())
        .fold(0.0, f64::max);
    if max_vector <= f64::EPSILON {
        return 1.0;
    }

    let spacing = quiver_reference_spacing(base_x, base_y);
    let autoscale = if spacing.is_finite() && spacing > f64::EPSILON {
        0.9 * spacing / max_vector
    } else {
        1.0
    };

    match scale_multiplier {
        Some(scale) if scale.abs() <= f64::EPSILON => 1.0,
        Some(scale) => autoscale * scale,
        None => autoscale,
    }
}

fn quiver3_effective_scale(
    base_x: &[f64],
    base_y: &[f64],
    base_z: &[f64],
    u_values: &[f64],
    v_values: &[f64],
    w_values: &[f64],
    scale_multiplier: Option<f64>,
) -> f64 {
    let max_vector = u_values
        .iter()
        .zip(v_values)
        .zip(w_values)
        .map(|((u, v), w)| (u * u + v * v + w * w).sqrt())
        .fold(0.0, f64::max);
    if max_vector <= f64::EPSILON {
        return 1.0;
    }

    let spacing = quiver3_reference_spacing(base_x, base_y, base_z);
    let autoscale = if spacing.is_finite() && spacing > f64::EPSILON {
        0.9 * spacing / max_vector
    } else {
        1.0
    };

    match scale_multiplier {
        Some(scale) if scale.abs() <= f64::EPSILON => 1.0,
        Some(scale) => autoscale * scale,
        None => autoscale,
    }
}

fn quiver_reference_spacing(base_x: &[f64], base_y: &[f64]) -> f64 {
    let x_spacing = positive_axis_spacing(base_x);
    let y_spacing = positive_axis_spacing(base_y);
    match (x_spacing.is_finite(), y_spacing.is_finite()) {
        (true, true) => x_spacing.min(y_spacing),
        (true, false) => x_spacing,
        (false, true) => y_spacing,
        (false, false) => 1.0,
    }
}

fn quiver3_reference_spacing(base_x: &[f64], base_y: &[f64], base_z: &[f64]) -> f64 {
    let x_spacing = positive_axis_spacing(base_x);
    let y_spacing = positive_axis_spacing(base_y);
    let z_spacing = positive_axis_spacing(base_z);
    [x_spacing, y_spacing, z_spacing]
        .into_iter()
        .filter(|spacing| spacing.is_finite())
        .reduce(f64::min)
        .unwrap_or(1.0)
}

fn positive_axis_spacing(values: &[f64]) -> f64 {
    let mut unique = values
        .iter()
        .copied()
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();
    unique.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    unique.dedup_by(|left, right| (*left - *right).abs() <= f64::EPSILON);
    unique
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .filter(|delta| *delta > f64::EPSILON)
        .fold(f64::INFINITY, f64::min)
}

fn parse_histogram_args(args: &[Value]) -> Result<HistogramSeriesData, RuntimeError> {
    let (data_value, requested_bins) = match args {
        [data] => (data, None),
        [data, requested] => (data, Some(requested)),
        _ => {
            return Err(RuntimeError::Unsupported(
                "histogram currently supports `histogram(data)`, `histogram(data, nbins)`, or `histogram(data, edges)`".to_string(),
            ))
        }
    };

    let (_, _, raw_values) = numeric_matrix(data_value, "histogram")?;
    let values = raw_values
        .into_iter()
        .filter(|value| value.is_finite())
        .collect::<Vec<_>>();

    let edges = match requested_bins {
        None => uniform_histogram_edges(&values, 10),
        Some(requested) => {
            let requested_values = numeric_vector(requested, "histogram")?;
            if requested_values.len() == 1 {
                uniform_histogram_edges(&values, scalar_usize(requested, "histogram")?)
            } else {
                histogram_edges(requested)?
            }
        }
    };

    Ok(HistogramSeriesData {
        counts: histogram_counts(&values, &edges),
        edges,
    })
}

fn parse_histogram2_args(args: &[Value]) -> Result<Histogram2SeriesData, RuntimeError> {
    let outputs = invoke_stdlib_builtin_outputs("histcounts2", args, 3)?;
    let [counts_value, x_edges_value, y_edges_value] = outputs.as_slice() else {
        return Err(RuntimeError::Unsupported(
            "histogram2 currently expects three outputs from the shared histcounts2 helper"
                .to_string(),
        ));
    };

    let (rows, cols, counts) = numeric_matrix(counts_value, "histogram2")?;
    let x_edges = numeric_vector(x_edges_value, "histogram2")?;
    let y_edges = numeric_vector(y_edges_value, "histogram2")?;
    if x_edges.len() != rows + 1 || y_edges.len() != cols + 1 {
        return Err(RuntimeError::ShapeError(format!(
            "histogram2 expected edge vectors of lengths {} and {} to match the counts matrix, found {} and {}",
            rows + 1,
            cols + 1,
            x_edges.len(),
            y_edges.len()
        )));
    }

    let upper = counts.iter().copied().fold(0.0, f64::max);
    Ok(Histogram2SeriesData {
        x_edges,
        y_edges,
        counts,
        count_range: if upper <= f64::EPSILON {
            (0.0, 1.0)
        } else {
            (0.0, upper)
        },
    })
}

fn parse_pie_args(args: &[Value], builtin_name: &str) -> Result<PieSeriesData, RuntimeError> {
    let (values_arg, second, third) = match args {
        [values] => (values, None, None),
        [values, second] => (values, Some(second), None),
        [values, second, third] => (values, Some(second), Some(third)),
        _ => {
            return Err(RuntimeError::Unsupported(
                format!(
                    "{builtin_name} currently supports `{builtin_name}(x)`, `{builtin_name}(x, explode)`, `{builtin_name}(x, labels)`, or `{builtin_name}(x, explode, labels)`"
                )
            ))
        }
    };

    let values = numeric_vector(values_arg, builtin_name)?;
    if values.is_empty() {
        return Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently requires at least one data value"
        )));
    }
    if values
        .iter()
        .any(|value| !value.is_finite() || *value < 0.0)
    {
        return Err(RuntimeError::TypeError(format!(
            "{builtin_name} currently expects finite nonnegative numeric values"
        )));
    }
    let sum = values.iter().sum::<f64>();
    if sum <= f64::EPSILON {
        return Err(RuntimeError::ShapeError(format!(
            "{builtin_name} currently requires at least one positive data value"
        )));
    }

    let mut explode = vec![false; values.len()];
    let mut labels = values
        .iter()
        .map(|value| format!("{}%", format_percentage_label(*value, sum)))
        .collect::<Vec<_>>();

    match (second, third) {
        (None, None) => {}
        (Some(second), None) if pie_arg_is_text_labels(second) => {
            labels = pie_labels(second, values.len(), builtin_name)?;
        }
        (Some(second), None) => {
            explode = pie_explode(second, values.len(), builtin_name)?;
        }
        (Some(second), Some(third)) => {
            explode = pie_explode(second, values.len(), builtin_name)?;
            labels = pie_labels(third, values.len(), builtin_name)?;
        }
        (None, Some(_)) => unreachable!(),
    }

    let total = if sum <= 1.0 { 1.0 } else { sum };
    let mut start = -std::f64::consts::FRAC_PI_2;
    let mut slices = Vec::with_capacity(values.len());
    for (index, value) in values.into_iter().enumerate() {
        let span = std::f64::consts::TAU * (value / total);
        let end = start + span;
        slices.push(PieSlice {
            start_angle: start,
            end_angle: end,
            exploded: explode[index],
            label: labels[index].clone(),
            color: SERIES_COLORS[index % SERIES_COLORS.len()],
        });
        start = end;
    }

    Ok(PieSeriesData { slices })
}

fn parse_xyz_series_args<'a>(
    args: &'a [Value],
    builtin_name: &str,
    allow_style: bool,
    allow_property_pairs: bool,
    allow_multiple_groups: bool,
) -> Result<(Vec<XyzSeriesGroup>, &'a [Value]), RuntimeError> {
    if args.is_empty() {
        return Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently supports exactly three numeric vector arguments{}",
            if allow_style {
                " with optional trailing line-spec strings"
            } else {
                ""
            }
        )));
    }

    let mut groups = Vec::new();
    let mut next_index = 0usize;
    while next_index < args.len() {
        if next_index + 2 >= args.len()
            || !can_be_numeric_series_input(&args[next_index])
            || !can_be_numeric_series_input(&args[next_index + 1])
            || !can_be_numeric_series_input(&args[next_index + 2])
        {
            break;
        }

        let x = numeric_vector(&args[next_index], builtin_name)?;
        let y = numeric_vector(&args[next_index + 1], builtin_name)?;
        let z = numeric_vector(&args[next_index + 2], builtin_name)?;
        if x.len() != y.len() || x.len() != z.len() {
            return Err(RuntimeError::ShapeError(format!(
                "{builtin_name} requires x, y, and z vectors with matching lengths, found {}, {}, and {}",
                x.len(),
                y.len(),
                z.len()
            )));
        }
        if x.is_empty() {
            return Err(RuntimeError::Unsupported(format!(
                "{builtin_name} currently requires at least one data point"
            )));
        }

        next_index += 3;
        let style = if allow_style
            && args.get(next_index).is_some_and(|candidate| {
                matches!(candidate, Value::CharArray(_) | Value::String(_))
                    && !is_series_property_name(candidate)
            }) {
            let style = parse_matlab_line_spec(&args[next_index], builtin_name)?;
            next_index += 1;
            Some(style)
        } else {
            None
        };

        groups.push(XyzSeriesGroup {
            three_d: three_d_series_from_points(
                x.into_iter()
                    .zip(y)
                    .zip(z)
                    .map(|((x, y), z)| (x, y, z))
                    .collect(),
            ),
            style,
        });

        if !allow_multiple_groups {
            break;
        }
    }

    let property_pairs = &args[next_index..];
    if !allow_property_pairs && !property_pairs.is_empty() {
        return Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently supports exactly three numeric vector arguments{}",
            if allow_style {
                " with an optional trailing line-spec string"
            } else {
                ""
            }
        )));
    }
    if property_pairs.len() % 2 != 0 {
        return Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently expects trailing graphics properties as property/value pairs"
        )));
    }

    Ok((groups, property_pairs))
}

fn parse_scatter_args<'a>(
    args: &'a [Value],
    builtin_name: &str,
) -> Result<(Vec<(XySeriesInput, ScatterSeriesData)>, &'a [Value]), RuntimeError> {
    let [x_arg, y_arg, rest @ ..] = args else {
        return Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently supports `{builtin_name}(x, y)` with optional size/color inputs, optional `filled`, and property/value pairs"
        )));
    };

    let (series_inputs, matrix_shape) = expand_scatter_xy_inputs(x_arg, y_arg, builtin_name)?;
    if series_inputs.iter().any(|input| input.x.is_empty()) {
        return Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently requires at least one data point"
        )));
    }

    let scatters = parse_scatter_visual_args(
        rest,
        series_inputs[0].x.len(),
        series_inputs.len(),
        matrix_shape,
        builtin_name,
    )?;
    let (scatter_groups, property_pairs) = scatters;
    Ok((
        series_inputs.into_iter().zip(scatter_groups).collect(),
        property_pairs,
    ))
}

fn expand_scatter_xy_inputs(
    x_value: &Value,
    y_value: &Value,
    builtin_name: &str,
) -> Result<(Vec<XySeriesInput>, Option<(usize, usize)>), RuntimeError> {
    let (x_rows, x_cols, x_values) = numeric_matrix(x_value, builtin_name)?;
    let (y_rows, y_cols, y_values) = numeric_matrix(y_value, builtin_name)?;
    let x_is_vector = x_rows == 1 || x_cols == 1;
    let y_is_vector = y_rows == 1 || y_cols == 1;

    if x_is_vector && y_is_vector {
        if x_values.len() != y_values.len() {
            return Err(RuntimeError::ShapeError(format!(
                "{builtin_name} requires x and y vectors with matching lengths, found {} and {}",
                x_values.len(),
                y_values.len()
            )));
        }
        return Ok((
            vec![XySeriesInput {
                x: x_values,
                y: y_values,
            }],
            None,
        ));
    }

    if !x_is_vector && !y_is_vector {
        if x_rows != y_rows || x_cols != y_cols {
            return Err(RuntimeError::ShapeError(format!(
                "{builtin_name} requires x and y matrices with matching sizes, found {}x{} and {}x{}",
                x_rows, x_cols, y_rows, y_cols
            )));
        }
        let groups = (0..x_cols)
            .map(|col| XySeriesInput {
                x: matrix_column_values(&x_values, x_rows, x_cols, col),
                y: matrix_column_values(&y_values, y_rows, y_cols, col),
            })
            .collect::<Vec<_>>();
        return Ok((groups, Some((x_rows, x_cols))));
    }

    if x_is_vector && !y_is_vector {
        if x_values.len() != y_rows {
            return Err(RuntimeError::ShapeError(format!(
                "{builtin_name} requires x to have length {} to match the rows of y, found {}",
                y_rows,
                x_values.len()
            )));
        }
        let groups = (0..y_cols)
            .map(|col| XySeriesInput {
                x: x_values.clone(),
                y: matrix_column_values(&y_values, y_rows, y_cols, col),
            })
            .collect::<Vec<_>>();
        return Ok((groups, Some((y_rows, y_cols))));
    }

    if y_values.len() != x_rows {
        return Err(RuntimeError::ShapeError(format!(
            "{builtin_name} requires y to have length {} to match the rows of x, found {}",
            x_rows,
            y_values.len()
        )));
    }
    let groups = (0..x_cols)
        .map(|col| XySeriesInput {
            x: matrix_column_values(&x_values, x_rows, x_cols, col),
            y: y_values.clone(),
        })
        .collect::<Vec<_>>();
    Ok((groups, Some((x_rows, x_cols))))
}

fn parse_scatter3_args<'a>(
    args: &'a [Value],
    builtin_name: &str,
) -> Result<(Vec<(ThreeDSeriesData, ScatterSeriesData)>, &'a [Value]), RuntimeError> {
    let [x_arg, y_arg, z_arg, rest @ ..] = args else {
        return Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently supports `{builtin_name}(x, y, z)` with optional size/color inputs, optional `filled`, and property/value pairs"
        )));
    };

    let (series_inputs, matrix_shape) =
        expand_scatter_xyz_inputs(x_arg, y_arg, z_arg, builtin_name)?;
    if series_inputs.iter().any(|points| points.points.is_empty()) {
        return Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently requires at least one data point"
        )));
    }

    let (scatter_groups, property_pairs) = parse_scatter_visual_args(
        rest,
        series_inputs[0].points.len(),
        series_inputs.len(),
        matrix_shape,
        builtin_name,
    )?;
    Ok((
        series_inputs.into_iter().zip(scatter_groups).collect(),
        property_pairs,
    ))
}

fn expand_scatter_xyz_inputs(
    x_value: &Value,
    y_value: &Value,
    z_value: &Value,
    builtin_name: &str,
) -> Result<(Vec<ThreeDSeriesData>, Option<(usize, usize)>), RuntimeError> {
    let (x_rows, x_cols, x_values) = numeric_matrix(x_value, builtin_name)?;
    let (y_rows, y_cols, y_values) = numeric_matrix(y_value, builtin_name)?;
    let (z_rows, z_cols, z_values) = numeric_matrix(z_value, builtin_name)?;
    let x_is_vector = x_rows == 1 || x_cols == 1;
    let y_is_vector = y_rows == 1 || y_cols == 1;
    let z_is_vector = z_rows == 1 || z_cols == 1;

    if x_is_vector && y_is_vector && z_is_vector {
        if x_values.len() != y_values.len() || x_values.len() != z_values.len() {
            return Err(RuntimeError::ShapeError(format!(
                "{builtin_name} requires x, y, and z vectors with matching lengths, found {}, {}, and {}",
                x_values.len(),
                y_values.len(),
                z_values.len()
            )));
        }
        return Ok((
            vec![three_d_series_from_points(
                x_values
                    .into_iter()
                    .zip(y_values)
                    .zip(z_values)
                    .map(|((x, y), z)| (x, y, z))
                    .collect(),
            )],
            None,
        ));
    }

    let rows = [x_rows, y_rows, z_rows]
        .into_iter()
        .zip([x_is_vector, y_is_vector, z_is_vector])
        .filter_map(|(rows, is_vector)| (!is_vector).then_some(rows))
        .next()
        .unwrap_or(0);
    let cols = [x_cols, y_cols, z_cols]
        .into_iter()
        .zip([x_is_vector, y_is_vector, z_is_vector])
        .filter_map(|(cols, is_vector)| (!is_vector).then_some(cols))
        .next()
        .unwrap_or(1);

    for (label, current_rows, current_cols, is_vector, len) in [
        ("x", x_rows, x_cols, x_is_vector, x_values.len()),
        ("y", y_rows, y_cols, y_is_vector, y_values.len()),
        ("z", z_rows, z_cols, z_is_vector, z_values.len()),
    ] {
        if is_vector {
            if len != rows {
                return Err(RuntimeError::ShapeError(format!(
                    "{builtin_name} requires {label} to have length {rows} to match matrix inputs, found {len}"
                )));
            }
        } else if current_rows != rows || current_cols != cols {
            return Err(RuntimeError::ShapeError(format!(
                "{builtin_name} requires matching matrix sizes across x, y, and z, found {}x{}, {}x{}, and {}x{}",
                x_rows, x_cols, y_rows, y_cols, z_rows, z_cols
            )));
        }
    }

    let groups = (0..cols)
        .map(|col| {
            let x = if x_is_vector {
                x_values.clone()
            } else {
                matrix_column_values(&x_values, rows, cols, col)
            };
            let y = if y_is_vector {
                y_values.clone()
            } else {
                matrix_column_values(&y_values, rows, cols, col)
            };
            let z = if z_is_vector {
                z_values.clone()
            } else {
                matrix_column_values(&z_values, rows, cols, col)
            };
            three_d_series_from_points(
                x.into_iter()
                    .zip(y)
                    .zip(z)
                    .map(|((x, y), z)| (x, y, z))
                    .collect(),
            )
        })
        .collect::<Vec<_>>();
    Ok((groups, Some((rows, cols))))
}

fn parse_scatter_visual_args<'a>(
    args: &'a [Value],
    point_count: usize,
    group_count: usize,
    matrix_shape: Option<(usize, usize)>,
    builtin_name: &str,
) -> Result<(Vec<ScatterSeriesData>, &'a [Value]), RuntimeError> {
    let mut next_index = 0usize;
    let mut marker_sizes = vec![vec![6.0; point_count]; group_count];
    let mut colors = None::<Vec<ScatterColors>>;
    let mut filled = false;
    let mut marker = None;

    if let Some(candidate) = args.get(next_index) {
        if !is_series_property_name(candidate) && !is_text_keyword(candidate, "filled")? {
            if can_be_numeric_series_input(candidate) {
                marker_sizes = parse_scatter_marker_sizes_grouped(
                    candidate,
                    point_count,
                    group_count,
                    matrix_shape,
                    builtin_name,
                )?;
                next_index += 1;
                if let Some(color_candidate) = args.get(next_index) {
                    if !is_series_property_name(color_candidate)
                        && !is_text_keyword(color_candidate, "filled")?
                    {
                        colors = Some(parse_scatter_colors_grouped(
                            color_candidate,
                            point_count,
                            group_count,
                            matrix_shape,
                            builtin_name,
                        )?);
                        next_index += 1;
                    }
                }
            } else {
                colors = Some(parse_scatter_colors_grouped(
                    candidate,
                    point_count,
                    group_count,
                    matrix_shape,
                    builtin_name,
                )?);
                next_index += 1;
            }
        }
    }

    while let Some(value) = args.get(next_index) {
        if is_series_property_name(value) {
            break;
        }
        if is_text_keyword(value, "filled")? {
            filled = true;
            next_index += 1;
            continue;
        }
        if matches!(value, Value::CharArray(_) | Value::String(_)) {
            marker = Some(parse_marker_style(value, builtin_name)?);
            next_index += 1;
            continue;
        }
        break;
    }

    let property_pairs = &args[next_index..];
    if property_pairs.len() % 2 != 0 {
        return Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently expects trailing graphics properties as property/value pairs"
        )));
    }

    let scatters = (0..group_count)
        .map(|index| ScatterSeriesData {
            marker_sizes: marker_sizes[index].clone(),
            colors: colors
                .as_ref()
                .map(|all| all[index].clone())
                .unwrap_or(ScatterColors::Uniform("#1f77b4")),
            filled,
            uses_default_color: colors.is_none(),
            marker,
        })
        .collect::<Vec<_>>();
    Ok((scatters, property_pairs))
}

fn parse_scatter_marker_sizes(
    value: &Value,
    point_count: usize,
    builtin_name: &str,
) -> Result<Vec<f64>, RuntimeError> {
    let values = match value {
        Value::Matrix(matrix) if matrix.elements.is_empty() => vec![6.0; point_count],
        _ => numeric_vector(value, builtin_name)?,
    };
    if values.len() == 1 {
        return Ok(vec![scatter_marker_size_value(values[0]); point_count]);
    }
    if values.len() != point_count {
        return Err(RuntimeError::ShapeError(format!(
            "{builtin_name} marker sizes must be scalar or match the point count {}, found {} values",
            point_count,
            values.len()
        )));
    }
    Ok(values.into_iter().map(scatter_marker_size_value).collect())
}

fn parse_scatter_marker_sizes_grouped(
    value: &Value,
    point_count: usize,
    group_count: usize,
    matrix_shape: Option<(usize, usize)>,
    builtin_name: &str,
) -> Result<Vec<Vec<f64>>, RuntimeError> {
    if group_count == 1 {
        return Ok(vec![parse_scatter_marker_sizes(
            value,
            point_count,
            builtin_name,
        )?]);
    }

    if let Some((rows, cols)) = matrix_shape {
        let (value_rows, value_cols, values) = numeric_matrix(value, builtin_name)?;
        if value_rows == rows && value_cols == cols {
            return Ok((0..cols)
                .map(|col| {
                    matrix_column_values(&values, rows, cols, col)
                        .into_iter()
                        .map(scatter_marker_size_value)
                        .collect::<Vec<_>>()
                })
                .collect());
        }
    }

    let values = match value {
        Value::Matrix(matrix) if matrix.elements.is_empty() => vec![6.0; point_count],
        _ => numeric_vector(value, builtin_name)?,
    };
    if values.len() == 1 {
        return Ok(vec![
            vec![scatter_marker_size_value(values[0]); point_count];
            group_count
        ]);
    }
    if values.len() == point_count {
        let shared = values
            .into_iter()
            .map(scatter_marker_size_value)
            .collect::<Vec<_>>();
        return Ok(vec![shared; group_count]);
    }

    Err(RuntimeError::ShapeError(format!(
        "{builtin_name} marker sizes must be scalar, match the point count {point_count}, or match the matrix input shape for {group_count} series"
    )))
}

fn scatter_marker_size_value(value: f64) -> f64 {
    value.max(0.0).sqrt().max(1.0)
}

fn parse_scatter_colors(
    value: &Value,
    point_count: usize,
    builtin_name: &str,
) -> Result<ScatterColors, RuntimeError> {
    match value {
        Value::CharArray(_) | Value::String(_) => Ok(ScatterColors::Uniform(
            parse_graphics_color_input(value, builtin_name)?,
        )),
        Value::Matrix(matrix) if matrix.elements.is_empty() => {
            Ok(ScatterColors::Uniform("#1f77b4"))
        }
        Value::Matrix(matrix) if matrix.cols == 3 && matrix.rows > 1 => {
            if matrix.rows != point_count {
                return Err(RuntimeError::ShapeError(format!(
                    "{builtin_name} RGB color matrices must have one row per point, found {} rows for {} points",
                    matrix.rows,
                    point_count
                )));
            }
            let mut colors = Vec::with_capacity(matrix.rows);
            for row in 0..matrix.rows {
                colors.push([
                    matrix.get(row, 0).as_scalar()?,
                    matrix.get(row, 1).as_scalar()?,
                    matrix.get(row, 2).as_scalar()?,
                ]);
            }
            normalize_scatter_rgb_colors(&mut colors, builtin_name)?;
            Ok(ScatterColors::Rgb(colors))
        }
        Value::Matrix(matrix) if matrix.rows == 1 && matrix.cols == 3 => {
            let mut colors = vec![[
                matrix.get(0, 0).as_scalar()?,
                matrix.get(0, 1).as_scalar()?,
                matrix.get(0, 2).as_scalar()?,
            ]];
            normalize_scatter_rgb_colors(&mut colors, builtin_name)?;
            Ok(ScatterColors::Rgb(vec![colors[0]; point_count]))
        }
        _ => {
            let values = numeric_vector(value, builtin_name)?;
            if values.len() == 1 {
                Ok(ScatterColors::Colormapped(vec![values[0]; point_count]))
            } else if values.len() == point_count {
                Ok(ScatterColors::Colormapped(values))
            } else {
                Err(RuntimeError::ShapeError(format!(
                    "{builtin_name} color data must be a color spec, an N-by-3 RGB matrix, or a numeric vector matching the point count {}, found {} value(s)",
                    point_count,
                    values.len()
                )))
            }
        }
    }
}

fn parse_scatter_colors_grouped(
    value: &Value,
    point_count: usize,
    group_count: usize,
    matrix_shape: Option<(usize, usize)>,
    builtin_name: &str,
) -> Result<Vec<ScatterColors>, RuntimeError> {
    if group_count == 1 {
        return Ok(vec![parse_scatter_colors(
            value,
            point_count,
            builtin_name,
        )?]);
    }

    match value {
        Value::CharArray(_) | Value::String(_) => {
            let color = parse_graphics_color_input(value, builtin_name)?;
            Ok(vec![ScatterColors::Uniform(color); group_count])
        }
        Value::Matrix(matrix) if matrix.rows == 1 && matrix.cols == 3 => {
            let single = parse_scatter_colors(value, point_count, builtin_name)?;
            Ok(vec![single; group_count])
        }
        Value::Matrix(matrix) if matrix.cols == 3 && matrix.rows == point_count => {
            let single = parse_scatter_colors(value, point_count, builtin_name)?;
            Ok(vec![single; group_count])
        }
        _ => {
            if let Some((rows, cols)) = matrix_shape {
                let (value_rows, value_cols, values) = numeric_matrix(value, builtin_name)?;
                if value_rows == rows && value_cols == cols {
                    return Ok((0..cols)
                        .map(|col| {
                            ScatterColors::Colormapped(matrix_column_values(
                                &values, rows, cols, col,
                            ))
                        })
                        .collect());
                }
            }

            let values = numeric_vector(value, builtin_name)?;
            if values.len() == 1 {
                Ok(vec![
                    ScatterColors::Colormapped(vec![values[0]; point_count]);
                    group_count
                ])
            } else if values.len() == point_count {
                Ok(vec![ScatterColors::Colormapped(values); group_count])
            } else {
                Err(RuntimeError::ShapeError(format!(
                    "{builtin_name} color data must be scalar, match the point count {point_count}, or match the matrix input shape for {group_count} series"
                )))
            }
        }
    }
}

fn normalize_scatter_rgb_colors(
    colors: &mut [[f64; 3]],
    builtin_name: &str,
) -> Result<(), RuntimeError> {
    let mut max_channel = f64::NEG_INFINITY;
    let mut min_channel = f64::INFINITY;
    for color in colors.iter() {
        for channel in color {
            max_channel = max_channel.max(*channel);
            min_channel = min_channel.min(*channel);
        }
    }

    if min_channel < 0.0 {
        return Err(RuntimeError::TypeError(format!(
            "{builtin_name} RGB color values must be nonnegative"
        )));
    }

    if max_channel <= 1.0 {
        return Ok(());
    }

    if max_channel <= 255.0 {
        for color in colors.iter_mut() {
            for channel in color {
                *channel /= 255.0;
            }
        }
        return Ok(());
    }

    Err(RuntimeError::TypeError(format!(
        "{builtin_name} RGB color values currently support only [0, 1] or [0, 255] ranges"
    )))
}

fn three_d_series_from_points(points: Vec<(f64, f64, f64)>) -> ThreeDSeriesData {
    if points.is_empty() {
        return ThreeDSeriesData {
            points,
            x_range: (0.0, 1.0),
            y_range: (0.0, 1.0),
            z_range: (0.0, 1.0),
        };
    }

    let mut x_values = Vec::with_capacity(points.len());
    let mut y_values = Vec::with_capacity(points.len());
    let mut z_values = Vec::with_capacity(points.len());
    for (x, y, z) in &points {
        x_values.push(*x);
        y_values.push(*y);
        z_values.push(*z);
    }

    ThreeDSeriesData {
        points,
        x_range: finite_min_max(&x_values),
        y_range: finite_min_max(&y_values),
        z_range: finite_min_max(&z_values),
    }
}

fn stem3_series_from_points(points: Vec<(f64, f64, f64)>) -> ThreeDSeriesData {
    let mut series = three_d_series_from_points(points);
    series.z_range.0 = series.z_range.0.min(0.0);
    series.z_range.1 = series.z_range.1.max(0.0);
    series
}

fn parse_image_matrix_args(
    args: &[Value],
    builtin_name: &str,
    mode: ImageMode,
) -> Result<ImageSeriesData, RuntimeError> {
    let (x_data, y_data, matrix_value) = match args {
        [matrix_value] => (None, None, matrix_value),
        [x_data, y_data, matrix_value] => (Some(x_data), Some(y_data), matrix_value),
        _ => {
            return Err(RuntimeError::Unsupported(format!(
                "{builtin_name} currently supports either one image matrix argument or `XData`, `YData`, and image matrix"
            )))
        }
    };

    if let Some((rows, cols, rgb_values)) = rgb_image_matrix(matrix_value, builtin_name)? {
        let (x_data, y_data) = image_coordinate_data(rows, cols, x_data, y_data, builtin_name)?;
        return Ok(ImageSeriesData {
            rows,
            cols,
            values: Vec::new(),
            rgb_values: Some(rgb_values),
            alpha_data: ImageAlphaData::Scalar(1.0),
            alpha_mapping: AlphaDataMapping::None,
            x_data,
            y_data,
            display_range: (0.0, 1.0),
            mapping: ImageMapping::Scaled,
        });
    }

    let (rows, cols, values) = numeric_matrix(matrix_value, builtin_name)?;
    let (x_data, y_data) = image_coordinate_data(rows, cols, x_data, y_data, builtin_name)?;
    let (display_range, mapping) = match mode {
        ImageMode::Scaled => (finite_min_max(&values), ImageMapping::Scaled),
        ImageMode::UnitRange => ((0.0, 1.0), ImageMapping::Scaled),
        ImageMode::Direct => ((1.0, 8.0), ImageMapping::Direct),
    };

    Ok(ImageSeriesData {
        rows,
        cols,
        values,
        rgb_values: None,
        alpha_data: ImageAlphaData::Scalar(1.0),
        alpha_mapping: AlphaDataMapping::None,
        x_data,
        y_data,
        display_range,
        mapping,
    })
}

fn image_coordinate_data(
    rows: usize,
    cols: usize,
    x_data: Option<&Value>,
    y_data: Option<&Value>,
    builtin_name: &str,
) -> Result<(Vec<f64>, Vec<f64>), RuntimeError> {
    let x = match x_data {
        Some(value) => image_coordinate_vector(value, cols, "XData", builtin_name)?,
        None => default_image_coordinate_data(cols),
    };
    let y = match y_data {
        Some(value) => image_coordinate_vector(value, rows, "YData", builtin_name)?,
        None => default_image_coordinate_data(rows),
    };
    Ok((x, y))
}

fn image_coordinate_vector(
    value: &Value,
    extent: usize,
    axis_name: &str,
    builtin_name: &str,
) -> Result<Vec<f64>, RuntimeError> {
    let values = numeric_vector(value, builtin_name)?;
    match values.len() {
        1 => Ok(vec![values[0]]),
        2 => {
            if extent <= 1 {
                Ok(vec![values[0]])
            } else {
                let step = (values[1] - values[0]) / (extent - 1) as f64;
                Ok((0..extent)
                    .map(|index| values[0] + step * index as f64)
                    .collect())
            }
        }
        count if count == extent => Ok(values),
        count => Err(RuntimeError::ShapeError(format!(
            "{builtin_name} currently expects {axis_name} to have length 1, 2, or exactly the image extent {extent}, found {count}"
        ))),
    }
}

fn default_image_coordinate_data(extent: usize) -> Vec<f64> {
    if extent == 0 {
        Vec::new()
    } else {
        (1..=extent).map(|value| value as f64).collect()
    }
}

fn parse_text_args(args: &[Value]) -> Result<TextSeriesData, RuntimeError> {
    let [x, y, label] = args else {
        return Err(RuntimeError::Unsupported(
            "text currently supports exactly three arguments: x, y, and one text label".to_string(),
        ));
    };

    Ok(TextSeriesData {
        x: finite_scalar_arg(x, "text")?,
        y: finite_scalar_arg(y, "text")?,
        label: text_arg(label, "text")?,
    })
}

struct LineSpec {
    x: Vec<f64>,
    y: Vec<f64>,
    color: Option<&'static str>,
    marker_edge_color: MarkerColorMode,
    marker_face_color: MarkerColorMode,
    display_name: Option<String>,
    visible: bool,
    line_width: f64,
    line_style: LineStyle,
    marker: MarkerStyle,
    marker_size: f64,
}

struct RectangleSpec {
    position: [f64; 4],
    edge_color: Option<&'static str>,
    face_color: Option<&'static str>,
    line_width: f64,
    line_style: LineStyle,
    visible: bool,
}

struct PatchSpec {
    x: Vec<f64>,
    y: Vec<f64>,
    edge_color: &'static str,
    face_color: Option<&'static str>,
    line_width: f64,
    line_style: LineStyle,
    visible: bool,
    display_name: Option<String>,
}

struct Fill3Spec {
    x: Vec<f64>,
    y: Vec<f64>,
    zipped_points: Vec<(f64, f64, f64)>,
    edge_color: &'static str,
    face_color: Option<&'static str>,
    line_width: f64,
    line_style: LineStyle,
    visible: bool,
    display_name: Option<String>,
}

struct Bar3hSpec {
    z_positions: Vec<f64>,
    rows: usize,
    cols: usize,
    values: Vec<f64>,
}

#[derive(Debug, Clone, Copy)]
struct LineSpecStyle {
    color: Option<&'static str>,
    line_style: Option<LineStyle>,
    marker: Option<MarkerStyle>,
}

struct ReferenceLineSpec {
    values: Vec<f64>,
    labels: Vec<String>,
    style: Option<LineSpecStyle>,
    property_pairs: Vec<Value>,
}

struct ErrorBarSpec {
    x: Vec<f64>,
    y: Vec<f64>,
    vertical_lower: Option<Vec<f64>>,
    vertical_upper: Option<Vec<f64>>,
    horizontal_lower: Option<Vec<f64>>,
    horizontal_upper: Option<Vec<f64>>,
    style: Option<LineSpecStyle>,
    property_pairs: Vec<Value>,
}

struct LegendSpec {
    labels: Option<Vec<String>>,
    location: Option<LegendLocation>,
    orientation: Option<LegendOrientation>,
}

struct AnnotationSpec {
    kind: AnnotationKind,
    x: Vec<f64>,
    y: Vec<f64>,
    position: Option<[f64; 4]>,
    text: String,
    color: &'static str,
    line_width: f64,
    line_style: LineStyle,
    visible: bool,
    face_color: Option<&'static str>,
    font_size: f64,
}

fn parse_reference_line_spec(
    args: &[Value],
    builtin_name: &str,
) -> Result<ReferenceLineSpec, RuntimeError> {
    let Some((positions, rest)) = args.split_first() else {
        return Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently expects at least one scalar or vector position argument"
        )));
    };

    let values = numeric_vector(positions, builtin_name)?;
    if values.is_empty() {
        return Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently requires at least one position value"
        )));
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err(RuntimeError::TypeError(format!(
            "{builtin_name} currently expects finite position values"
        )));
    }

    let mut style = None;
    let mut next_index = 0usize;
    if let Some(candidate) = rest.first() {
        if reference_line_supports_linespec(candidate, rest.get(1), builtin_name)? {
            style = Some(parse_matlab_line_spec(candidate, builtin_name)?);
            next_index = 1;
        }
    }

    let mut labels = Vec::new();
    let remaining = &rest[next_index..];
    let mut label_offset = 0usize;
    if let Some(candidate) = remaining.first() {
        if reference_line_supports_labels(candidate, remaining.get(1), builtin_name)? {
            labels = normalize_reference_line_labels(
                text_labels_from_value(candidate, builtin_name)?,
                values.len(),
                builtin_name,
            )?;
            label_offset = 1;
        }
    }

    let property_pairs = &remaining[label_offset..];
    if property_pairs.len() % 2 != 0 {
        return Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently expects trailing properties as property/value pairs"
        )));
    }

    let (property_pairs, property_labels) =
        split_reference_line_property_pairs(property_pairs, builtin_name, values.len())?;
    if !property_labels.is_empty() {
        labels = property_labels;
    }

    if labels.is_empty() {
        labels = vec![String::new(); values.len()];
    }

    Ok(ReferenceLineSpec {
        values,
        labels,
        style,
        property_pairs,
    })
}

fn parse_errorbar_spec(args: &[Value]) -> Result<ErrorBarSpec, RuntimeError> {
    if args.len() < 2 {
        return Err(RuntimeError::Unsupported(
            "errorbar currently supports `(y, err)`, `(x, y, err)`, `(x, y, lower, upper)`, or `(x, y, lower, upper, xlower, xupper)`"
                .to_string(),
        ));
    }

    let mut numeric_count = 0usize;
    while numeric_count < args.len()
        && numeric_count < 6
        && can_be_numeric_series_input(&args[numeric_count])
    {
        numeric_count += 1;
    }

    let (x, y, mut vertical_lower, mut vertical_upper, mut horizontal_lower, mut horizontal_upper, rest) = match numeric_count {
        2 => {
            let y = numeric_vector(&args[0], "errorbar")?;
            let err = numeric_vector(&args[1], "errorbar")?;
            let x = (1..=y.len()).map(|value| value as f64).collect::<Vec<_>>();
            (
                x,
                y,
                Some(err.clone()),
                Some(err),
                None,
                None,
                &args[2..],
            )
        }
        3 => {
            let x = numeric_vector(&args[0], "errorbar")?;
            let y = numeric_vector(&args[1], "errorbar")?;
            let err = numeric_vector(&args[2], "errorbar")?;
            (
                x,
                y,
                Some(err.clone()),
                Some(err),
                None,
                None,
                &args[3..],
            )
        }
        4 => {
            let x = numeric_vector(&args[0], "errorbar")?;
            let y = numeric_vector(&args[1], "errorbar")?;
            let lower = numeric_vector(&args[2], "errorbar")?;
            let upper = numeric_vector(&args[3], "errorbar")?;
            (x, y, Some(lower), Some(upper), None, None, &args[4..])
        }
        6 => {
            let x = numeric_vector(&args[0], "errorbar")?;
            let y = numeric_vector(&args[1], "errorbar")?;
            let lower = numeric_vector(&args[2], "errorbar")?;
            let upper = numeric_vector(&args[3], "errorbar")?;
            let x_lower = numeric_vector(&args[4], "errorbar")?;
            let x_upper = numeric_vector(&args[5], "errorbar")?;
            (
                x,
                y,
                Some(lower),
                Some(upper),
                Some(x_lower),
                Some(x_upper),
                &args[6..],
            )
        }
        _ => {
            return Err(RuntimeError::Unsupported(
                "errorbar currently supports `(y, err)`, `(x, y, err)`, `(x, y, lower, upper)`, or `(x, y, lower, upper, xlower, xupper)`"
                    .to_string(),
            ))
        }
    };

    let point_count = y.len();
    if point_count == 0 {
        return Err(RuntimeError::Unsupported(
            "errorbar currently requires at least one data point".to_string(),
        ));
    }
    if x.len() != point_count {
        return Err(RuntimeError::ShapeError(format!(
            "errorbar requires x and y vectors with matching lengths, found {} and {}",
            x.len(),
            y.len()
        )));
    }

    for values in [
        vertical_lower.as_ref(),
        vertical_upper.as_ref(),
        horizontal_lower.as_ref(),
        horizontal_upper.as_ref(),
    ]
    .into_iter()
    .flatten()
    {
        if values.len() != point_count {
            return Err(RuntimeError::ShapeError(format!(
                "errorbar requires all error vectors to match the plotted point count {}, found {}",
                point_count,
                values.len()
            )));
        }
        if values
            .iter()
            .any(|value| !value.is_finite() || *value < 0.0)
        {
            return Err(RuntimeError::TypeError(
                "errorbar currently expects finite nonnegative error magnitudes".to_string(),
            ));
        }
    }

    if let Some(candidate) = rest.first() {
        if is_text_keyword(candidate, "horizontal")? {
            if horizontal_lower.is_some() || horizontal_upper.is_some() {
                return Err(RuntimeError::Unsupported(
                    "errorbar horizontal orientation keyword is not supported together with explicit horizontal error vectors".to_string(),
                ));
            }
            horizontal_lower = vertical_lower.clone();
            horizontal_upper = vertical_upper.clone();
            vertical_lower = None;
            vertical_upper = None;
        } else if is_text_keyword(candidate, "both")? {
            if horizontal_lower.is_some() || horizontal_upper.is_some() {
                return Err(RuntimeError::Unsupported(
                    "errorbar `both` orientation is not supported together with explicit horizontal error vectors".to_string(),
                ));
            }
            horizontal_lower = vertical_lower.clone();
            horizontal_upper = vertical_upper.clone();
        } else if is_text_keyword(candidate, "vertical")? {
        }
    }

    if vertical_lower.is_none()
        && vertical_upper.is_none()
        && horizontal_lower.is_none()
        && horizontal_upper.is_none()
    {
        return Err(RuntimeError::TypeError(
            "errorbar currently requires at least one vertical or horizontal error component"
                .to_string(),
        ));
    }

    let mut style = None;
    let mut next_index = 0usize;
    let mut rest = rest;
    if let Some(candidate) = rest.first() {
        if is_text_keyword(candidate, "horizontal")?
            || is_text_keyword(candidate, "vertical")?
            || is_text_keyword(candidate, "both")?
        {
            rest = &rest[1..];
        }
    }
    if let Some(candidate) = rest.first() {
        if !is_series_property_name(candidate)
            && parse_matlab_line_spec(candidate, "errorbar").is_ok()
        {
            style = Some(parse_matlab_line_spec(candidate, "errorbar")?);
            next_index = 1;
        }
    }
    let property_pairs = rest[next_index..].to_vec();
    if property_pairs.len() % 2 != 0 {
        return Err(RuntimeError::Unsupported(
            "errorbar currently expects trailing properties as property/value pairs".to_string(),
        ));
    }

    Ok(ErrorBarSpec {
        x,
        y,
        vertical_lower,
        vertical_upper,
        horizontal_lower,
        horizontal_upper,
        style,
        property_pairs,
    })
}

fn parse_legend_spec(args: &[Value], axes: &AxesState) -> Result<LegendSpec, RuntimeError> {
    if args.is_empty() {
        return Ok(LegendSpec {
            labels: Some(default_legend_labels(axes)),
            location: None,
            orientation: None,
        });
    }
    if args.len() == 1 && is_text_keyword(&args[0], "off")? {
        return Ok(LegendSpec {
            labels: None,
            location: None,
            orientation: None,
        });
    }
    if args.len() == 1 && is_text_keyword(&args[0], "show")? {
        return Ok(LegendSpec {
            labels: Some(
                axes.legend
                    .clone()
                    .unwrap_or_else(|| default_legend_labels(axes)),
            ),
            location: None,
            orientation: None,
        });
    }

    let mut split_index = args.len();
    while split_index >= 2 {
        let candidate_name = &args[split_index - 2];
        let (Value::CharArray(text) | Value::String(text)) = candidate_name else {
            break;
        };
        let lower = text.to_ascii_lowercase();
        if lower == "location" || lower == "orientation" {
            split_index -= 2;
        } else {
            break;
        }
    }

    let mut location = None;
    let mut orientation = None;
    for pair in args[split_index..].chunks(2) {
        let name = text_arg(&pair[0], "legend")?;
        let value = text_arg(&pair[1], "legend")?;
        match name.to_ascii_lowercase().as_str() {
            "location" => {
                location = Some(LegendLocation::from_text(&value).ok_or_else(|| {
                    RuntimeError::Unsupported(format!(
                        "legend currently supports Location values like `northeast`, `northwest`, `southwest`, `southeast`, `north`, `south`, `east`, `west`, and `best`, found `{value}`"
                    ))
                })?);
            }
            "orientation" => {
                orientation = Some(LegendOrientation::from_text(&value).ok_or_else(|| {
                    RuntimeError::Unsupported(format!(
                        "legend currently supports Orientation values `vertical` or `horizontal`, found `{value}`"
                    ))
                })?);
            }
            other => {
                return Err(RuntimeError::Unsupported(format!(
                    "legend currently supports only the `Location` and `Orientation` options, found `{other}`"
                )))
            }
        }
    }

    let label_args = &args[..split_index];
    let labels = if label_args.is_empty() {
        Some(
            axes.legend
                .clone()
                .unwrap_or_else(|| default_legend_labels(axes)),
        )
    } else if label_args.len() == 1 {
        match &label_args[0] {
            cell @ Value::Cell(_) => Some(text_labels_from_value(cell, "legend")?),
            value => Some(vec![text_arg(value, "legend")?]),
        }
    } else {
        Some(
            label_args
                .iter()
                .map(|arg| text_arg(arg, "legend"))
                .collect::<Result<Vec<_>, _>>()?,
        )
    };

    Ok(LegendSpec {
        labels,
        location,
        orientation,
    })
}

fn parse_annotation_spec(args: &[Value]) -> Result<AnnotationSpec, RuntimeError> {
    let Some((kind_value, rest)) = args.split_first() else {
        return Err(RuntimeError::Unsupported(
            "annotation currently expects at least one type argument".to_string(),
        ));
    };
    let kind_text = text_arg(kind_value, "annotation")?;
    let kind = match kind_text.to_ascii_lowercase().as_str() {
        "line" => AnnotationKind::Line,
        "arrow" => AnnotationKind::Arrow,
        "doublearrow" => AnnotationKind::DoubleArrow,
        "textarrow" => AnnotationKind::TextArrow,
        "textbox" => AnnotationKind::TextBox,
        "rectangle" => AnnotationKind::Rectangle,
        "ellipse" => AnnotationKind::Ellipse,
        other => {
            return Err(RuntimeError::Unsupported(format!(
                "annotation currently supports `line`, `arrow`, `doublearrow`, `textarrow`, `textbox`, `rectangle`, and `ellipse`, found `{other}`"
            )))
        }
    };

    let mut spec = AnnotationSpec {
        kind,
        x: vec![0.3, 0.4],
        y: vec![0.3, 0.4],
        position: Some([0.3, 0.3, 0.1, 0.1]),
        text: String::new(),
        color: "#1f77b4",
        line_width: 1.5,
        line_style: LineStyle::Solid,
        visible: true,
        face_color: None,
        font_size: 12.0,
    };

    let mut next_index = 0usize;
    match kind {
        AnnotationKind::Line
        | AnnotationKind::Arrow
        | AnnotationKind::DoubleArrow
        | AnnotationKind::TextArrow => {
            if rest.len() >= 2
                && can_be_numeric_series_input(&rest[0])
                && can_be_numeric_series_input(&rest[1])
            {
                spec.x = numeric_vector(&rest[0], "annotation")?;
                spec.y = numeric_vector(&rest[1], "annotation")?;
                if spec.x.len() != 2 || spec.y.len() != 2 {
                    return Err(RuntimeError::ShapeError(
                        "annotation line-like objects currently expect x and y as 1x2 normalized vectors"
                            .to_string(),
                    ));
                }
                next_index = 2;
            }
            if kind == AnnotationKind::TextArrow
                && rest.get(next_index).is_some()
                && !is_annotation_property_name(rest[next_index].clone())?
            {
                spec.text = text_arg(&rest[next_index], "annotation")?;
                next_index += 1;
            }
            spec.position = None;
        }
        AnnotationKind::TextBox | AnnotationKind::Rectangle | AnnotationKind::Ellipse => {
            if let Some(position_value) = rest.first() {
                if can_be_numeric_series_input(position_value) {
                    let values = numeric_vector(position_value, "annotation")?;
                    if values.len() != 4 {
                        return Err(RuntimeError::ShapeError(
                            "annotation shape objects currently expect a 1x4 normalized position vector"
                                .to_string(),
                        ));
                    }
                    spec.position = Some([values[0], values[1], values[2], values[3]]);
                    next_index = 1;
                }
            }
            if kind == AnnotationKind::TextBox
                && rest.get(next_index).is_some()
                && !is_annotation_property_name(rest[next_index].clone())?
            {
                spec.text = text_arg(&rest[next_index], "annotation")?;
                next_index += 1;
            }
            spec.x.clear();
            spec.y.clear();
        }
    }

    let property_pairs = &rest[next_index..];
    if property_pairs.len() % 2 != 0 {
        return Err(RuntimeError::Unsupported(
            "annotation currently expects trailing properties as property/value pairs".to_string(),
        ));
    }
    for pair in property_pairs.chunks_exact(2) {
        let property = text_arg(&pair[0], "annotation")?;
        match property.to_ascii_lowercase().as_str() {
            "string" => spec.text = text_arg(&pair[1], "annotation")?,
            "color" => spec.color = parse_graphics_color_input(&pair[1], "annotation")?,
            "linewidth" => spec.line_width = finite_scalar_arg(&pair[1], "annotation")?,
            "linestyle" => spec.line_style = parse_line_style(&pair[1], "annotation")?,
            "visible" => spec.visible = on_off_flag(&pair[1], "annotation")?,
            "facecolor" => {
                spec.face_color = if is_text_keyword(&pair[1], "none")? {
                    None
                } else {
                    Some(parse_graphics_color_input(&pair[1], "annotation")?)
                };
            }
            "fontsize" => spec.font_size = finite_scalar_arg(&pair[1], "annotation")?,
            other => {
                return Err(RuntimeError::Unsupported(format!(
                    "annotation currently supports `String`, `Color`, `LineWidth`, `LineStyle`, `Visible`, `FaceColor`, and `FontSize`, found `{other}`"
                )))
            }
        }
    }

    Ok(spec)
}

fn is_annotation_property_name(value: Value) -> Result<bool, RuntimeError> {
    match value {
        Value::CharArray(text) | Value::String(text) => Ok(matches!(
            text.to_ascii_lowercase().as_str(),
            "string" | "color" | "linewidth" | "linestyle" | "visible" | "facecolor" | "fontsize"
        )),
        _ => Ok(false),
    }
}

fn reference_line_supports_linespec(
    value: &Value,
    next_value: Option<&Value>,
    builtin_name: &str,
) -> Result<bool, RuntimeError> {
    match value {
        Value::CharArray(text) | Value::String(text)
            if !is_reference_line_property_name(text) || next_value.is_none() =>
        {
            Ok(parse_matlab_line_spec(value, builtin_name).is_ok())
        }
        _ => Ok(false),
    }
}

fn reference_line_supports_labels(
    value: &Value,
    next_value: Option<&Value>,
    builtin_name: &str,
) -> Result<bool, RuntimeError> {
    match value {
        Value::CharArray(text) | Value::String(text) => {
            if is_reference_line_property_name(text) && next_value.is_some() {
                return Ok(false);
            }
            Ok(true)
        }
        Value::Cell(_) | Value::Matrix(_) => {
            let labels = text_labels_from_value(value, builtin_name)?;
            Ok(!labels.is_empty())
        }
        _ => Ok(false),
    }
}

fn is_reference_line_property_name(text: &str) -> bool {
    matches!(
        text.to_ascii_lowercase().as_str(),
        "label" | "color" | "displayname" | "visible" | "linewidth" | "linestyle"
    )
}

fn normalize_reference_line_labels(
    labels: Vec<String>,
    expected_len: usize,
    builtin_name: &str,
) -> Result<Vec<String>, RuntimeError> {
    match labels.len() {
        0 => Ok(vec![String::new(); expected_len]),
        1 if expected_len > 1 => Ok(vec![labels[0].clone(); expected_len]),
        count if count == expected_len => Ok(labels),
        count => Err(RuntimeError::ShapeError(format!(
            "{builtin_name} currently expects one label or exactly {expected_len} labels, found {count}"
        ))),
    }
}

fn split_reference_line_property_pairs(
    property_pairs: &[Value],
    builtin_name: &str,
    value_count: usize,
) -> Result<(Vec<Value>, Vec<String>), RuntimeError> {
    let mut filtered = Vec::new();
    let mut labels = Vec::new();
    for pair in property_pairs.chunks_exact(2) {
        if is_text_keyword(&pair[0], "label")? {
            labels = normalize_reference_line_labels(
                text_labels_from_value(&pair[1], builtin_name)?,
                value_count,
                builtin_name,
            )?;
        } else {
            filtered.push(pair[0].clone());
            filtered.push(pair[1].clone());
        }
    }
    Ok((filtered, labels))
}

fn parse_line_spec(args: &[Value]) -> Result<LineSpec, RuntimeError> {
    if args.len() == 2 {
        let x = numeric_vector(&args[0], "line")?;
        let y = numeric_vector(&args[1], "line")?;
        if x.len() != y.len() {
            return Err(RuntimeError::ShapeError(format!(
                "line requires XData and YData with matching lengths, found {} and {}",
                x.len(),
                y.len()
            )));
        }
        if x.is_empty() {
            return Err(RuntimeError::Unsupported(
                "line currently requires at least one data point".to_string(),
            ));
        }
        return Ok(LineSpec {
            x,
            y,
            color: None,
            marker_edge_color: MarkerColorMode::Auto,
            marker_face_color: MarkerColorMode::None,
            display_name: None,
            visible: true,
            line_width: 2.5,
            line_style: LineStyle::Solid,
            marker: MarkerStyle::None,
            marker_size: 5.0,
        });
    }

    if args.len() < 4 || args.len() % 2 != 0 {
        return Err(RuntimeError::Unsupported(
            "line currently supports either `line(x, y)` or property/value pairs including `XData` and `YData`".to_string(),
        ));
    }

    let mut x = None;
    let mut y = None;
    let mut color = None;
    let mut marker_edge_color = MarkerColorMode::Auto;
    let mut marker_face_color = MarkerColorMode::None;
    let mut display_name = None;
    let mut visible = true;
    let mut line_width = 2.5;
    let mut line_style = LineStyle::Solid;
    let mut marker = MarkerStyle::None;
    let mut marker_size = 5.0;

    for pair in args.chunks(2) {
        let property = text_arg(&pair[0], "line")?.to_ascii_lowercase();
        match property.as_str() {
            "xdata" => x = Some(numeric_vector(&pair[1], "line")?),
            "ydata" => y = Some(numeric_vector(&pair[1], "line")?),
            "color" => color = Some(parse_graphics_color_input(&pair[1], "line")?),
            "displayname" => display_name = Some(text_arg(&pair[1], "line")?),
            "visible" => visible = on_off_flag(&pair[1], "line")?,
            "linewidth" => line_width = finite_scalar_arg(&pair[1], "line")?,
            "linestyle" => line_style = parse_line_style(&pair[1], "line")?,
            "marker" => marker = parse_marker_style(&pair[1], "line")?,
            "markersize" => marker_size = finite_scalar_arg(&pair[1], "line")?,
            "markeredgecolor" => marker_edge_color = parse_marker_color_input(&pair[1], "line")?,
            "markerfacecolor" => marker_face_color = parse_marker_color_input(&pair[1], "line")?,
            other => {
                return Err(RuntimeError::Unsupported(format!(
                    "line currently supports only `XData`, `YData`, `Color`, `DisplayName`, `Visible`, `LineWidth`, `LineStyle`, `Marker`, `MarkerSize`, `MarkerEdgeColor`, and `MarkerFaceColor`, found `{other}`"
                )))
            }
        }
    }

    let x = x.ok_or_else(|| {
        RuntimeError::Unsupported(
            "line currently requires an `XData` property in property/value form".to_string(),
        )
    })?;
    let y = y.ok_or_else(|| {
        RuntimeError::Unsupported(
            "line currently requires a `YData` property in property/value form".to_string(),
        )
    })?;
    if x.len() != y.len() {
        return Err(RuntimeError::ShapeError(format!(
            "line requires XData and YData with matching lengths, found {} and {}",
            x.len(),
            y.len()
        )));
    }
    if x.is_empty() {
        return Err(RuntimeError::Unsupported(
            "line currently requires at least one data point".to_string(),
        ));
    }

    Ok(LineSpec {
        x,
        y,
        color,
        marker_edge_color,
        marker_face_color,
        display_name,
        visible,
        line_width,
        line_style,
        marker,
        marker_size,
    })
}

fn parse_rectangle_spec(args: &[Value]) -> Result<RectangleSpec, RuntimeError> {
    if args.len() < 2 || args.len() % 2 != 0 {
        return Err(RuntimeError::Unsupported(
            "rectangle currently supports property/value pairs including `Position`".to_string(),
        ));
    }

    let mut position = None;
    let mut edge_color = None;
    let mut face_color = None;
    let mut line_width = 2.5;
    let mut line_style = LineStyle::Solid;
    let mut visible = true;

    for pair in args.chunks(2) {
        let property = text_arg(&pair[0], "rectangle")?.to_ascii_lowercase();
        match property.as_str() {
            "position" => {
                let values = numeric_vector(&pair[1], "rectangle")?;
                if values.len() != 4 {
                    return Err(RuntimeError::ShapeError(
                        "rectangle currently expects `Position` as a numeric 1x4 vector"
                            .to_string(),
                    ));
                }
                position = Some([values[0], values[1], values[2], values[3]]);
            }
            "edgecolor" => edge_color = Some(parse_graphics_color_input(&pair[1], "rectangle")?),
            "facecolor" => {
                face_color = if is_text_keyword(&pair[1], "none")? {
                    None
                } else {
                    Some(parse_graphics_color_input(&pair[1], "rectangle")?)
                };
            }
            "linewidth" => line_width = finite_scalar_arg(&pair[1], "rectangle")?,
            "linestyle" => line_style = parse_line_style(&pair[1], "rectangle")?,
            "visible" => visible = on_off_flag(&pair[1], "rectangle")?,
            other => {
                return Err(RuntimeError::Unsupported(format!(
                    "rectangle currently supports only `Position`, `EdgeColor`, `FaceColor`, `LineWidth`, `LineStyle`, and `Visible`, found `{other}`"
                )))
            }
        }
    }

    Ok(RectangleSpec {
        position: position.ok_or_else(|| {
            RuntimeError::Unsupported(
                "rectangle currently requires a `Position` property".to_string(),
            )
        })?,
        edge_color,
        face_color,
        line_width,
        line_style,
        visible,
    })
}

fn parse_patch_spec(
    args: &[Value],
    builtin_name: &str,
    allow_property_value: bool,
) -> Result<PatchSpec, RuntimeError> {
    if !allow_property_value
        || (args.len() == 3 && !matches!(args[0], Value::CharArray(_) | Value::String(_)))
    {
        let [x_arg, y_arg, color_arg] = args else {
            return Err(RuntimeError::Unsupported(format!(
                "{builtin_name} currently supports `{builtin_name}(x, y, color)`{}",
                if allow_property_value {
                    " or property/value pairs including `XData` and `YData`"
                } else {
                    ""
                }
            )));
        };
        let x = numeric_vector(x_arg, builtin_name)?;
        let y = numeric_vector(y_arg, builtin_name)?;
        if x.len() != y.len() {
            return Err(RuntimeError::ShapeError(format!(
                "{builtin_name} requires XData and YData with matching lengths, found {} and {}",
                x.len(),
                y.len()
            )));
        }
        if x.len() < 3 {
            return Err(RuntimeError::Unsupported(format!(
                "{builtin_name} currently requires at least three vertices"
            )));
        }
        let color = parse_graphics_color_input(color_arg, builtin_name)?;
        return Ok(PatchSpec {
            x,
            y,
            edge_color: color,
            face_color: Some(color),
            line_width: 1.5,
            line_style: LineStyle::Solid,
            visible: true,
            display_name: None,
        });
    }

    if args.len() < 4 || args.len() % 2 != 0 {
        return Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently supports property/value pairs including `XData` and `YData`"
        )));
    }

    let mut x = None;
    let mut y = None;
    let mut edge_color = "#1f77b4";
    let mut face_color = None;
    let mut line_width = 1.5;
    let mut line_style = LineStyle::Solid;
    let mut visible = true;
    let mut display_name = None;

    for pair in args.chunks(2) {
        let property = text_arg(&pair[0], builtin_name)?.to_ascii_lowercase();
        match property.as_str() {
            "xdata" => x = Some(numeric_vector(&pair[1], builtin_name)?),
            "ydata" => y = Some(numeric_vector(&pair[1], builtin_name)?),
            "edgecolor" => edge_color = parse_graphics_color_input(&pair[1], builtin_name)?,
            "facecolor" => {
                face_color = if is_text_keyword(&pair[1], "none")? {
                    None
                } else {
                    Some(parse_graphics_color_input(&pair[1], builtin_name)?)
                };
            }
            "linewidth" => line_width = finite_scalar_arg(&pair[1], builtin_name)?,
            "linestyle" => line_style = parse_line_style(&pair[1], builtin_name)?,
            "visible" => visible = on_off_flag(&pair[1], builtin_name)?,
            "displayname" => display_name = Some(text_arg(&pair[1], builtin_name)?),
            other => {
                return Err(RuntimeError::Unsupported(format!(
                    "{builtin_name} currently supports only `XData`, `YData`, `EdgeColor`, `FaceColor`, `LineWidth`, `LineStyle`, `Visible`, and `DisplayName`, found `{other}`"
                )))
            }
        }
    }

    let x = x.ok_or_else(|| {
        RuntimeError::Unsupported(format!(
            "{builtin_name} currently requires an `XData` property"
        ))
    })?;
    let y = y.ok_or_else(|| {
        RuntimeError::Unsupported(format!(
            "{builtin_name} currently requires a `YData` property"
        ))
    })?;
    if x.len() != y.len() {
        return Err(RuntimeError::ShapeError(format!(
            "{builtin_name} requires XData and YData with matching lengths, found {} and {}",
            x.len(),
            y.len()
        )));
    }
    if x.len() < 3 {
        return Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently requires at least three vertices"
        )));
    }

    Ok(PatchSpec {
        x,
        y,
        edge_color,
        face_color,
        line_width,
        line_style,
        visible,
        display_name,
    })
}

fn parse_fill3_spec(args: &[Value]) -> Result<Fill3Spec, RuntimeError> {
    let [x_arg, y_arg, z_arg, rest @ ..] = args else {
        return Err(RuntimeError::Unsupported(
            "fill3 currently supports `fill3(x, y, z)` or `fill3(x, y, z, color)`".to_string(),
        ));
    };

    if rest.len() > 1 {
        return Err(RuntimeError::Unsupported(
            "fill3 currently supports at most one trailing color argument".to_string(),
        ));
    }

    let x = numeric_vector(x_arg, "fill3")?;
    let y = numeric_vector(y_arg, "fill3")?;
    let z = numeric_vector(z_arg, "fill3")?;
    if x.len() != y.len() || x.len() != z.len() {
        return Err(RuntimeError::ShapeError(format!(
            "fill3 requires matching X, Y, and Z vertex counts, found {}, {}, and {}",
            x.len(),
            y.len(),
            z.len()
        )));
    }
    if x.len() < 3 {
        return Err(RuntimeError::Unsupported(
            "fill3 currently requires at least three vertices".to_string(),
        ));
    }

    let (edge_color, face_color) = if let Some(color_arg) = rest.first() {
        let color = parse_graphics_color_input(color_arg, "fill3")?;
        (color, Some(color))
    } else {
        ("#1f77b4", Some("#1f77b4"))
    };

    Ok(Fill3Spec {
        zipped_points: x
            .iter()
            .zip(&y)
            .zip(&z)
            .map(|((x, y), z)| (*x, *y, *z))
            .collect(),
        x,
        y,
        edge_color,
        face_color,
        line_width: 1.5,
        line_style: LineStyle::Solid,
        visible: true,
        display_name: None,
    })
}

fn parse_contour_args(args: &[Value]) -> Result<ContourSeriesData, RuntimeError> {
    let (x, y, rows, cols, values, levels) = match args {
        [z] => {
            let (rows, cols, values) = numeric_matrix(z, "contour")?;
            let (lower, upper) = finite_min_max(&values);
            (
                (1..=cols).map(|value| value as f64).collect::<Vec<_>>(),
                (1..=rows).map(|value| value as f64).collect::<Vec<_>>(),
                rows,
                cols,
                values,
                generated_contour_levels(lower, upper, 10),
            )
        }
        [z, requested_levels] => {
            let (rows, cols, values) = numeric_matrix(z, "contour")?;
            let (lower, upper) = finite_min_max(&values);
            (
                (1..=cols).map(|value| value as f64).collect::<Vec<_>>(),
                (1..=rows).map(|value| value as f64).collect::<Vec<_>>(),
                rows,
                cols,
                values,
                parse_contour_levels(requested_levels, lower, upper)?,
            )
        }
        [x, y, z] => {
            let (rows, cols, values) = numeric_matrix(z, "contour")?;
            let x = numeric_vector(x, "contour")?;
            let y = numeric_vector(y, "contour")?;
            let (lower, upper) = finite_min_max(&values);
            (
                x,
                y,
                rows,
                cols,
                values,
                generated_contour_levels(lower, upper, 10),
            )
        }
        [x, y, z, requested_levels] => {
            let (rows, cols, values) = numeric_matrix(z, "contour")?;
            let x = numeric_vector(x, "contour")?;
            let y = numeric_vector(y, "contour")?;
            let (lower, upper) = finite_min_max(&values);
            (
                x,
                y,
                rows,
                cols,
                values,
                parse_contour_levels(requested_levels, lower, upper)?,
            )
        }
        _ => {
            return Err(RuntimeError::Unsupported(
                "contour currently supports `contour(Z)`, `contour(Z, levels)`, `contour(X, Y, Z)`, or `contour(X, Y, Z, levels)` with vector X/Y coordinates and a numeric Z matrix"
                    .to_string(),
            ))
        }
    };

    if rows < 2 || cols < 2 {
        return Err(RuntimeError::ShapeError(
            "contour currently requires a numeric matrix with at least two rows and two columns"
                .to_string(),
        ));
    }
    if x.len() != cols || y.len() != rows {
        return Err(RuntimeError::ShapeError(format!(
            "contour requires X and Y coordinate vectors that match the Z matrix shape, found X length {} and Y length {} for a {}x{} matrix",
            x.len(),
            y.len(),
            rows,
            cols
        )));
    }
    if levels.is_empty() {
        return Err(RuntimeError::Unsupported(
            "contour currently requires at least one finite contour level".to_string(),
        ));
    }

    Ok(build_contour_series(&x, &y, rows, cols, &values, &levels))
}

fn parse_contour_levels(value: &Value, lower: f64, upper: f64) -> Result<Vec<f64>, RuntimeError> {
    match value {
        Value::Scalar(count)
            if count.is_finite() && *count >= 1.0 && count.fract().abs() <= f64::EPSILON =>
        {
            Ok(generated_contour_levels(lower, upper, *count as usize))
        }
        Value::Logical(flag) => Ok(generated_contour_levels(
            lower,
            upper,
            if *flag { 1 } else { 0 },
        )),
        _ => {
            let mut levels = numeric_vector(value, "contour")?
                .into_iter()
                .filter(|level| level.is_finite())
                .collect::<Vec<_>>();
            levels.sort_by(|left, right| {
                left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal)
            });
            levels.dedup_by(|left, right| (*left - *right).abs() <= 1e-9);
            Ok(levels)
        }
    }
}

fn generated_contour_levels(lower: f64, upper: f64, count: usize) -> Vec<f64> {
    if count == 0 {
        return Vec::new();
    }
    if !lower.is_finite() || !upper.is_finite() {
        return vec![0.0];
    }
    if (upper - lower).abs() <= f64::EPSILON {
        return vec![lower];
    }

    let step = (upper - lower) / (count as f64 + 1.0);
    (1..=count)
        .map(|index| lower + step * index as f64)
        .collect()
}

fn build_contour_series(
    x: &[f64],
    y: &[f64],
    rows: usize,
    cols: usize,
    values: &[f64],
    levels: &[f64],
) -> ContourSeriesData {
    let mut segments = Vec::new();
    for row in 0..rows - 1 {
        for col in 0..cols - 1 {
            let bottom_left = ContourPoint {
                x: x[col],
                y: y[row],
                z: values[row * cols + col],
            };
            let bottom_right = ContourPoint {
                x: x[col + 1],
                y: y[row],
                z: values[row * cols + col + 1],
            };
            let top_right = ContourPoint {
                x: x[col + 1],
                y: y[row + 1],
                z: values[(row + 1) * cols + col + 1],
            };
            let top_left = ContourPoint {
                x: x[col],
                y: y[row + 1],
                z: values[(row + 1) * cols + col],
            };
            for level in levels {
                if let Some(segment) =
                    contour_triangle_segment(bottom_left, bottom_right, top_right, *level)
                {
                    segments.push(segment);
                }
                if let Some(segment) =
                    contour_triangle_segment(bottom_left, top_right, top_left, *level)
                {
                    segments.push(segment);
                }
            }
        }
    }

    let x_domain = finite_min_max(x);
    let y_domain = finite_min_max(y);
    ContourSeriesData {
        segments,
        x_domain,
        y_domain,
        level_range: finite_min_max(levels),
    }
}

fn parse_contourf_args(args: &[Value]) -> Result<ContourFillSeriesData, RuntimeError> {
    let (x, y, rows, cols, values, levels) = match args {
        [z] => {
            let (rows, cols, values) = numeric_matrix(z, "contourf")?;
            let (lower, upper) = finite_min_max(&values);
            (
                (1..=cols).map(|value| value as f64).collect::<Vec<_>>(),
                (1..=rows).map(|value| value as f64).collect::<Vec<_>>(),
                rows,
                cols,
                values,
                generated_contour_levels(lower, upper, 10),
            )
        }
        [z, requested_levels] => {
            let (rows, cols, values) = numeric_matrix(z, "contourf")?;
            let (lower, upper) = finite_min_max(&values);
            (
                (1..=cols).map(|value| value as f64).collect::<Vec<_>>(),
                (1..=rows).map(|value| value as f64).collect::<Vec<_>>(),
                rows,
                cols,
                values,
                parse_contour_levels(requested_levels, lower, upper)?,
            )
        }
        [x, y, z] => {
            let (rows, cols, values) = numeric_matrix(z, "contourf")?;
            let x = numeric_vector(x, "contourf")?;
            let y = numeric_vector(y, "contourf")?;
            let (lower, upper) = finite_min_max(&values);
            (
                x,
                y,
                rows,
                cols,
                values,
                generated_contour_levels(lower, upper, 10),
            )
        }
        [x, y, z, requested_levels] => {
            let (rows, cols, values) = numeric_matrix(z, "contourf")?;
            let x = numeric_vector(x, "contourf")?;
            let y = numeric_vector(y, "contourf")?;
            let (lower, upper) = finite_min_max(&values);
            (
                x,
                y,
                rows,
                cols,
                values,
                parse_contour_levels(requested_levels, lower, upper)?,
            )
        }
        _ => {
            return Err(RuntimeError::Unsupported(
                "contourf currently supports `contourf(Z)`, `contourf(Z, levels)`, `contourf(X, Y, Z)`, or `contourf(X, Y, Z, levels)` with vector X/Y coordinates and a numeric Z matrix"
                    .to_string(),
            ))
        }
    };

    if rows < 2 || cols < 2 {
        return Err(RuntimeError::ShapeError(
            "contourf currently requires a numeric matrix with at least two rows and two columns"
                .to_string(),
        ));
    }
    if x.len() != cols || y.len() != rows {
        return Err(RuntimeError::ShapeError(format!(
            "contourf requires X and Y coordinate vectors that match the Z matrix shape, found X length {} and Y length {} for a {}x{} matrix",
            x.len(),
            y.len(),
            rows,
            cols
        )));
    }
    if levels.is_empty() {
        return Err(RuntimeError::Unsupported(
            "contourf currently requires at least one finite contour level".to_string(),
        ));
    }

    Ok(build_contour_fill_series(
        &x, &y, rows, cols, &values, &levels,
    ))
}

fn build_contour_fill_series(
    x: &[f64],
    y: &[f64],
    rows: usize,
    cols: usize,
    values: &[f64],
    levels: &[f64],
) -> ContourFillSeriesData {
    let mut patches = Vec::new();
    let sorted_levels = {
        let mut levels = levels.to_vec();
        levels.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
        levels
    };

    for row in 0..rows - 1 {
        for col in 0..cols - 1 {
            let bottom_left = values[row * cols + col];
            let bottom_right = values[row * cols + col + 1];
            let top_left = values[(row + 1) * cols + col];
            let top_right = values[(row + 1) * cols + col + 1];
            let average = (bottom_left + bottom_right + top_left + top_right) / 4.0;
            patches.push(ContourFillPatch {
                points: [
                    (x[col], y[row]),
                    (x[col + 1], y[row]),
                    (x[col + 1], y[row + 1]),
                    (x[col], y[row + 1]),
                ],
                color_value: contour_fill_band_value(average, &sorted_levels),
            });
        }
    }

    ContourFillSeriesData {
        patches,
        x_domain: finite_min_max(x),
        y_domain: finite_min_max(y),
        level_range: (
            *sorted_levels.first().unwrap_or(&0.0),
            *sorted_levels.last().unwrap_or(&1.0),
        ),
    }
}

fn contour_fill_band_value(value: f64, levels: &[f64]) -> f64 {
    if levels.is_empty() {
        return value;
    }
    if levels.len() == 1 {
        return levels[0];
    }
    if value <= levels[0] {
        return levels[0];
    }

    for pair in levels.windows(2) {
        if value <= pair[1] {
            return (pair[0] + pair[1]) / 2.0;
        }
    }
    *levels.last().unwrap_or(&value)
}

fn parse_surface_args(args: &[Value]) -> Result<SurfaceSeriesData, RuntimeError> {
    let (rows, cols, z_values, x_grid, y_grid) = match args {
        [z] => {
            let (rows, cols, z_values) = numeric_matrix(z, "surf")?;
            let x_grid = expand_surface_vector(
                &(1..=cols).map(|value| value as f64).collect::<Vec<_>>(),
                rows,
                cols,
                SurfaceAxis::X,
            );
            let y_grid = expand_surface_vector(
                &(1..=rows).map(|value| value as f64).collect::<Vec<_>>(),
                rows,
                cols,
                SurfaceAxis::Y,
            );
            (rows, cols, z_values, x_grid, y_grid)
        }
        [x, y, z] => {
            let (rows, cols, z_values) = numeric_matrix(z, "surf")?;
            let x_grid = surface_coordinate_grid(x, rows, cols, SurfaceAxis::X)?;
            let y_grid = surface_coordinate_grid(y, rows, cols, SurfaceAxis::Y)?;
            (rows, cols, z_values, x_grid, y_grid)
        }
        _ => {
            return Err(RuntimeError::Unsupported(
                "surf currently supports `surf(Z)` or `surf(X, Y, Z)` with numeric matrix Z values and vector-or-matrix X/Y coordinates"
                    .to_string(),
            ))
        }
    };

    if rows < 2 || cols < 2 {
        return Err(RuntimeError::ShapeError(
            "surf currently requires a numeric matrix with at least two rows and two columns"
                .to_string(),
        ));
    }

    Ok(build_surface_series(
        rows, cols, &x_grid, &y_grid, &z_values,
    ))
}

fn parse_surface_combo_args(
    args: &[Value],
    builtin_name: &str,
) -> Result<(SurfaceSeriesData, ContourSeriesData), RuntimeError> {
    let surface_args = match args {
        [_z] => &args[..1],
        [_z, _levels] => &args[..1],
        [_x, _y, _z] => &args[..3],
        [_x, _y, _z, _levels] => &args[..3],
        _ => {
            return Err(RuntimeError::Unsupported(format!(
                "{builtin_name} currently supports `{builtin_name}(Z)`, `{builtin_name}(Z, levels)`, `{builtin_name}(X, Y, Z)`, or `{builtin_name}(X, Y, Z, levels)` with numeric matrix Z values and vector X/Y coordinates"
            )))
        }
    };
    Ok((parse_surface_args(surface_args)?, parse_contour_args(args)?))
}

fn surface_coordinate_grid(
    value: &Value,
    rows: usize,
    cols: usize,
    axis: SurfaceAxis,
) -> Result<Vec<f64>, RuntimeError> {
    match value {
        Value::Matrix(matrix) if matrix.rows == rows && matrix.cols == cols => matrix
            .iter()
            .map(Value::as_scalar)
            .collect::<Result<Vec<_>, _>>(),
        _ => {
            let vector = numeric_vector(value, "surf")?;
            match axis {
                SurfaceAxis::X if vector.len() == cols => {
                    Ok(expand_surface_vector(&vector, rows, cols, axis))
                }
                SurfaceAxis::Y if vector.len() == rows => {
                    Ok(expand_surface_vector(&vector, rows, cols, axis))
                }
                SurfaceAxis::X => Err(RuntimeError::ShapeError(format!(
                    "surf requires X coordinates to be a vector with {} elements or a {}x{} matrix",
                    cols, rows, cols
                ))),
                SurfaceAxis::Y => Err(RuntimeError::ShapeError(format!(
                    "surf requires Y coordinates to be a vector with {} elements or a {}x{} matrix",
                    rows, rows, cols
                ))),
            }
        }
    }
}

fn expand_surface_vector(vector: &[f64], rows: usize, cols: usize, axis: SurfaceAxis) -> Vec<f64> {
    let mut out = Vec::with_capacity(rows * cols);
    for row in 0..rows {
        for col in 0..cols {
            out.push(match axis {
                SurfaceAxis::X => vector[col],
                SurfaceAxis::Y => vector[row],
            });
        }
    }
    out
}

fn build_surface_series(
    rows: usize,
    cols: usize,
    x_grid: &[f64],
    y_grid: &[f64],
    z_values: &[f64],
) -> SurfaceSeriesData {
    let z_range = finite_min_max(z_values);

    let mut patches = Vec::new();
    for row in 0..rows - 1 {
        for col in 0..cols - 1 {
            let bottom_left = row * cols + col;
            let bottom_right = row * cols + col + 1;
            let top_left = (row + 1) * cols + col;
            let top_right = (row + 1) * cols + col + 1;
            let points = [
                (
                    x_grid[bottom_left],
                    y_grid[bottom_left],
                    z_values[bottom_left],
                ),
                (
                    x_grid[bottom_right],
                    y_grid[bottom_right],
                    z_values[bottom_right],
                ),
                (x_grid[top_right], y_grid[top_right], z_values[top_right]),
                (x_grid[top_left], y_grid[top_left], z_values[top_left]),
            ];
            let color_value = (z_values[bottom_left]
                + z_values[bottom_right]
                + z_values[top_right]
                + z_values[top_left])
                / 4.0;
            patches.push(SurfacePatch {
                points,
                color_value,
            });
        }
    }

    SurfaceSeriesData {
        patches,
        grid: Some(SurfaceGridData {
            rows,
            cols,
            x_values: x_grid.to_vec(),
            y_values: y_grid.to_vec(),
            z_values: z_values.to_vec(),
        }),
        x_range: finite_min_max(x_grid),
        y_range: finite_min_max(y_grid),
        z_range,
    }
}

fn meshz_curtain_surface(surface: &SurfaceSeriesData) -> SurfaceSeriesData {
    if surface.patches.is_empty() {
        return surface.clone();
    }

    let base_z = surface.z_range.0.min(0.0);
    let mut patches = surface.patches.clone();
    let left_x = surface.x_range.0;
    let right_x = surface.x_range.1;
    let bottom_y = surface.y_range.0;
    let top_y = surface.y_range.1;

    for patch in &surface.patches {
        for edge in [
            (patch.points[0], patch.points[1]),
            (patch.points[1], patch.points[2]),
            (patch.points[2], patch.points[3]),
            (patch.points[3], patch.points[0]),
        ] {
            let on_left = (edge.0 .0 - left_x).abs() <= 1e-9 && (edge.1 .0 - left_x).abs() <= 1e-9;
            let on_right =
                (edge.0 .0 - right_x).abs() <= 1e-9 && (edge.1 .0 - right_x).abs() <= 1e-9;
            let on_bottom =
                (edge.0 .1 - bottom_y).abs() <= 1e-9 && (edge.1 .1 - bottom_y).abs() <= 1e-9;
            let on_top = (edge.0 .1 - top_y).abs() <= 1e-9 && (edge.1 .1 - top_y).abs() <= 1e-9;
            if on_left || on_right || on_bottom || on_top {
                patches.push(SurfacePatch {
                    points: [
                        (edge.0 .0, edge.0 .1, base_z),
                        (edge.1 .0, edge.1 .1, base_z),
                        edge.1,
                        edge.0,
                    ],
                    color_value: (edge.0 .2 + edge.1 .2) / 2.0,
                });
            }
        }
    }

    SurfaceSeriesData {
        patches,
        grid: None,
        x_range: surface.x_range,
        y_range: surface.y_range,
        z_range: (base_z, surface.z_range.1.max(base_z)),
    }
}

fn ribbon_surface(args: &[Value]) -> Result<SurfaceSeriesData, RuntimeError> {
    let (rows, cols, values, width) =
        match args {
            [y] => {
                let (rows, cols, values) = numeric_matrix(y, "ribbon")?;
                (rows, cols, values, 0.75)
            }
            [y, width] => {
                let (rows, cols, values) = numeric_matrix(y, "ribbon")?;
                (rows, cols, values, finite_scalar_arg(width, "ribbon")?)
            }
            _ => return Err(RuntimeError::Unsupported(
                "ribbon currently supports `ribbon(y)` or `ribbon(y, width)` with a numeric matrix"
                    .to_string(),
            )),
        };

    if rows < 2 || cols < 1 {
        return Err(RuntimeError::ShapeError(
            "ribbon currently requires a numeric matrix with at least two rows".to_string(),
        ));
    }

    let mut patches = Vec::new();
    let mut x_values = Vec::new();
    let mut y_values = Vec::new();
    let mut z_values = Vec::new();
    let half_width = width.max(0.0) / 2.0;

    for col in 0..cols {
        for row in 0..rows {
            let center_x = col as f64 + 1.0;
            let y_value = row as f64 + 1.0;
            let z_value = values[row * cols + col];
            x_values.extend([center_x - half_width, center_x + half_width]);
            y_values.push(y_value);
            z_values.push(z_value);
        }
        for row in 0..rows - 1 {
            let lower = values[row * cols + col];
            let upper = values[(row + 1) * cols + col];
            let y0 = row as f64 + 1.0;
            let y1 = row as f64 + 2.0;
            let x0 = col as f64 + 1.0 - half_width;
            let x1 = col as f64 + 1.0 + half_width;
            patches.push(SurfacePatch {
                points: [
                    (x0, y0, lower),
                    (x1, y0, lower),
                    (x1, y1, upper),
                    (x0, y1, upper),
                ],
                color_value: (lower + upper) / 2.0,
            });
        }
    }

    if x_values.is_empty() {
        x_values.extend([0.5, 1.5]);
        y_values.extend([1.0, 2.0]);
        z_values.extend([0.0, 1.0]);
    }

    Ok(SurfaceSeriesData {
        patches,
        grid: None,
        x_range: finite_min_max(&x_values),
        y_range: finite_min_max(&y_values),
        z_range: finite_min_max(&z_values),
    })
}

fn bar3_surface(rows: usize, cols: usize, values: &[f64]) -> SurfaceSeriesData {
    let mut patches = Vec::new();
    let mut x_values = Vec::new();
    let mut y_values = Vec::new();
    let mut z_values = Vec::new();
    let half = 0.4;

    for row in 0..rows.max(1) {
        for col in 0..cols.max(1) {
            let index = row * cols.max(1) + col;
            let value = values.get(index).copied().unwrap_or(0.0);
            let x_center = col as f64 + 1.0;
            let y_center = row as f64 + 1.0;
            let x0 = x_center - half;
            let x1 = x_center + half;
            let y0 = y_center - half;
            let y1 = y_center + half;
            let z0 = 0.0;
            let z1 = value;

            x_values.extend([x0, x1]);
            y_values.extend([y0, y1]);
            z_values.extend([z0, z1]);

            if value.abs() <= f64::EPSILON {
                continue;
            }

            patches.push(SurfacePatch {
                points: [(x0, y0, z1), (x1, y0, z1), (x1, y1, z1), (x0, y1, z1)],
                color_value: value,
            });
            patches.push(SurfacePatch {
                points: [(x0, y0, z0), (x1, y0, z0), (x1, y0, z1), (x0, y0, z1)],
                color_value: value,
            });
            patches.push(SurfacePatch {
                points: [(x1, y0, z0), (x1, y1, z0), (x1, y1, z1), (x1, y0, z1)],
                color_value: value,
            });
            patches.push(SurfacePatch {
                points: [(x1, y1, z0), (x0, y1, z0), (x0, y1, z1), (x1, y1, z1)],
                color_value: value,
            });
            patches.push(SurfacePatch {
                points: [(x0, y1, z0), (x0, y0, z0), (x0, y0, z1), (x0, y1, z1)],
                color_value: value,
            });
        }
    }

    if x_values.is_empty() {
        x_values.extend([0.5, 1.5]);
        y_values.extend([0.5, 1.5]);
        z_values.extend([0.0, 1.0]);
    }

    SurfaceSeriesData {
        patches,
        grid: None,
        x_range: finite_min_max(&x_values),
        y_range: finite_min_max(&y_values),
        z_range: finite_min_max(&z_values),
    }
}

fn parse_bar3h_args(args: &[Value]) -> Result<Bar3hSpec, RuntimeError> {
    let (z_positions, rows, cols, values) = match args {
        [y] => {
            let (rows, cols, matrix_values) = numeric_matrix(y, "bar3h")?;
            if rows == 1 || cols == 1 {
                let values = numeric_vector(y, "bar3h")?;
                let rows = values.len();
                let z_positions = (0..rows).map(|index| index as f64 + 1.0).collect::<Vec<_>>();
                (z_positions, rows, 1, values)
            } else {
                let z_positions = (0..rows).map(|index| index as f64 + 1.0).collect::<Vec<_>>();
                (z_positions, rows, cols, matrix_values)
            }
        }
        [z, y] => {
            let z_positions = numeric_vector(z, "bar3h")?;
            let (rows, cols, matrix_values) = numeric_matrix(y, "bar3h")?;
            if rows == 1 || cols == 1 {
                let values = numeric_vector(y, "bar3h")?;
                if z_positions.len() != values.len() {
                    return Err(RuntimeError::ShapeError(format!(
                        "bar3h requires Z positions and vector values with matching lengths, found {} and {}",
                        z_positions.len(),
                        values.len()
                    )));
                }
                (z_positions, values.len(), 1, values)
            } else {
                if z_positions.len() != rows {
                    return Err(RuntimeError::ShapeError(format!(
                        "bar3h requires Z positions with one entry per matrix row, found {} positions for {} rows",
                        z_positions.len(),
                        rows
                    )));
                }
                (z_positions, rows, cols, matrix_values)
            }
        }
        _ => {
            return Err(RuntimeError::Unsupported(
                "bar3h currently supports `bar3h(y)` or `bar3h(z, y)` with numeric vector or matrix data"
                    .to_string(),
            ))
        }
    };

    Ok(Bar3hSpec {
        z_positions,
        rows,
        cols,
        values,
    })
}

fn bar3h_surface(
    z_positions: &[f64],
    rows: usize,
    cols: usize,
    values: &[f64],
) -> SurfaceSeriesData {
    let mut patches = Vec::new();
    let mut x_values = Vec::new();
    let mut y_values = Vec::new();
    let mut z_values = Vec::new();
    let half_x = 0.4;
    let half_z = bar3h_half_depth(z_positions);

    for row in 0..rows.max(1) {
        for col in 0..cols.max(1) {
            let index = row * cols.max(1) + col;
            let value = values.get(index).copied().unwrap_or(0.0);
            let x_center = col as f64 + 1.0;
            let z_center = z_positions.get(row).copied().unwrap_or(row as f64 + 1.0);
            let x0 = x_center - half_x;
            let x1 = x_center + half_x;
            let y0: f64 = 0.0;
            let y1 = value;
            let z0 = z_center - half_z;
            let z1 = z_center + half_z;

            x_values.extend([x0, x1]);
            y_values.extend([y0.min(y1), y0.max(y1)]);
            z_values.extend([z0, z1]);

            if value.abs() <= f64::EPSILON {
                continue;
            }

            patches.push(SurfacePatch {
                points: [(x0, y1, z0), (x1, y1, z0), (x1, y1, z1), (x0, y1, z1)],
                color_value: value,
            });
            patches.push(SurfacePatch {
                points: [(x0, y0, z0), (x1, y0, z0), (x1, y1, z0), (x0, y1, z0)],
                color_value: value,
            });
            patches.push(SurfacePatch {
                points: [(x1, y0, z0), (x1, y0, z1), (x1, y1, z1), (x1, y1, z0)],
                color_value: value,
            });
            patches.push(SurfacePatch {
                points: [(x1, y0, z1), (x0, y0, z1), (x0, y1, z1), (x1, y1, z1)],
                color_value: value,
            });
            patches.push(SurfacePatch {
                points: [(x0, y0, z1), (x0, y0, z0), (x0, y1, z0), (x0, y1, z1)],
                color_value: value,
            });
        }
    }

    if x_values.is_empty() {
        x_values.extend([0.5, 1.5]);
        y_values.extend([0.0, 1.0]);
        z_values.extend([
            z_positions.first().copied().unwrap_or(1.0) - half_z,
            z_positions.first().copied().unwrap_or(1.0) + half_z,
        ]);
    }

    SurfaceSeriesData {
        patches,
        grid: None,
        x_range: finite_min_max(&x_values),
        y_range: finite_min_max(&y_values),
        z_range: finite_min_max(&z_values),
    }
}

fn bar3h_half_depth(z_positions: &[f64]) -> f64 {
    if z_positions.len() < 2 {
        return 0.4;
    }
    let mut sorted = z_positions.to_vec();
    sorted.sort_by(|left, right| left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal));
    let spacing = sorted
        .windows(2)
        .filter_map(|window| {
            let delta = (window[1] - window[0]).abs();
            (delta > f64::EPSILON).then_some(delta)
        })
        .fold(f64::INFINITY, f64::min);
    if spacing.is_finite() {
        (spacing * 0.4).max(0.1)
    } else {
        0.4
    }
}

fn numeric_limit_pair(value: &Value, builtin_name: &str) -> Result<(f64, f64), RuntimeError> {
    let values = numeric_vector(value, builtin_name)?;
    if values.len() != 2 {
        return Err(RuntimeError::ShapeError(format!(
            "{builtin_name} currently expects a numeric vector with exactly two elements"
        )));
    }
    Ok((values[0], values[1]))
}

fn numeric_matrix(
    value: &Value,
    builtin_name: &str,
) -> Result<(usize, usize, Vec<f64>), RuntimeError> {
    match value {
        Value::Scalar(number) => Ok((1, 1, vec![*number])),
        Value::Logical(flag) => Ok((1, 1, vec![if *flag { 1.0 } else { 0.0 }])),
        Value::Matrix(matrix) => Ok((
            matrix.rows,
            matrix.cols,
            matrix
                .iter()
                .map(Value::as_scalar)
                .collect::<Result<Vec<_>, _>>()?,
        )),
        _ => Err(RuntimeError::TypeError(format!(
            "{builtin_name} currently expects a numeric matrix input"
        ))),
    }
}

fn rgb_image_matrix(
    value: &Value,
    builtin_name: &str,
) -> Result<Option<(usize, usize, Vec<[f64; 3]>)>, RuntimeError> {
    let Value::Matrix(matrix) = value else {
        return Ok(None);
    };

    let mut dims = matrix.dims.clone();
    while dims.len() > 2 && dims.last() == Some(&1) {
        dims.pop();
    }
    if dims.len() != 3 || dims[2] != 3 {
        return Ok(None);
    }

    let rows = dims[0];
    let cols = dims[1];
    let mut rgb_values = vec![[0.0; 3]; rows * cols];
    for row in 0..rows {
        for col in 0..cols {
            for channel in 0..3 {
                let linear = row_major_linear_index(&[row, col, channel], &dims);
                rgb_values[row * cols + col][channel] = matrix.elements[linear].as_scalar()?;
            }
        }
    }

    let max_channel = rgb_values
        .iter()
        .flat_map(|pixel| pixel.iter())
        .copied()
        .fold(f64::NEG_INFINITY, f64::max);
    let min_channel = rgb_values
        .iter()
        .flat_map(|pixel| pixel.iter())
        .copied()
        .fold(f64::INFINITY, f64::min);

    if min_channel < 0.0 {
        return Err(RuntimeError::TypeError(format!(
            "{builtin_name} RGB image values must be nonnegative"
        )));
    }

    if max_channel <= 1.0 {
        return Ok(Some((rows, cols, rgb_values)));
    }

    if max_channel <= 255.0 {
        for pixel in &mut rgb_values {
            for channel in pixel {
                *channel /= 255.0;
            }
        }
        return Ok(Some((rows, cols, rgb_values)));
    }

    Err(RuntimeError::TypeError(format!(
        "{builtin_name} RGB image values currently support only [0, 1] or [0, 255] ranges"
    )))
}

fn finite_min_max(values: &[f64]) -> (f64, f64) {
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);

    if !min.is_finite() || !max.is_finite() {
        (0.0, 1.0)
    } else if (max - min).abs() <= f64::EPSILON {
        (min - 0.5, max + 0.5)
    } else {
        (min, max)
    }
}

fn row_major_linear_index(index: &[usize], dims: &[usize]) -> usize {
    let mut linear = 0usize;
    for (axis, &value) in index.iter().enumerate() {
        linear = (linear * dims[axis].max(1)) + value;
    }
    linear
}

fn uniform_histogram_edges(values: &[f64], bin_count: usize) -> Vec<f64> {
    let (mut lower, mut upper) = if values.is_empty() {
        (0.0, 1.0)
    } else {
        finite_min_max(values)
    };
    if (upper - lower).abs() <= f64::EPSILON {
        lower -= 0.5;
        upper += 0.5;
    }

    let width = (upper - lower) / bin_count as f64;
    (0..=bin_count)
        .map(|index| lower + index as f64 * width)
        .collect()
}

fn histogram_edges(value: &Value) -> Result<Vec<f64>, RuntimeError> {
    let edges = numeric_vector(value, "histogram")?;
    if edges.len() < 2 {
        return Err(RuntimeError::ShapeError(
            "histogram currently expects an edges vector with at least two elements".to_string(),
        ));
    }
    if edges.iter().any(|edge| !edge.is_finite()) {
        return Err(RuntimeError::TypeError(
            "histogram currently expects finite numeric bin edges".to_string(),
        ));
    }
    if edges.windows(2).any(|window| window[1] <= window[0]) {
        return Err(RuntimeError::ShapeError(
            "histogram currently expects strictly increasing bin edges".to_string(),
        ));
    }
    Ok(edges)
}

fn histogram_counts(values: &[f64], edges: &[f64]) -> Vec<f64> {
    let mut counts = vec![0.0; edges.len().saturating_sub(1)];
    if counts.is_empty() {
        return counts;
    }

    let lower = edges[0];
    let upper = edges[edges.len() - 1];
    for &value in values {
        if value < lower || value > upper {
            continue;
        }

        let index = if (value - upper).abs() <= f64::EPSILON {
            counts.len() - 1
        } else {
            match edges
                .windows(2)
                .position(|window| value >= window[0] && value < window[1])
            {
                Some(index) => index,
                None => continue,
            }
        };
        counts[index] += 1.0;
    }

    counts
}

fn pie_slice_points(slice: &PieSlice) -> Vec<(f64, f64)> {
    let rim = pie_slice_rim_points(slice);
    let (center_x, center_y) = pie_center(slice);
    let mut points = Vec::with_capacity(rim.len() + 1);
    points.push((center_x, center_y));
    points.extend(rim);
    points
}

fn pie_slice_rim_points(slice: &PieSlice) -> Vec<(f64, f64)> {
    let (center_x, center_y) = pie_center(slice);
    let span = (slice.end_angle - slice.start_angle).abs();
    let segments = ((span / (std::f64::consts::PI / 18.0)).ceil() as usize).max(2);
    let mut points = Vec::with_capacity(segments + 1);
    for step in 0..=segments {
        let fraction = step as f64 / segments as f64;
        let angle = slice.start_angle + fraction * (slice.end_angle - slice.start_angle);
        points.push((center_x + angle.cos(), center_y + angle.sin()));
    }
    points
}

fn pie_center(slice: &PieSlice) -> (f64, f64) {
    if !slice.exploded {
        return (0.0, 0.0);
    }
    let mid = (slice.start_angle + slice.end_angle) / 2.0;
    (0.12 * mid.cos(), 0.12 * mid.sin())
}

fn pie_label_point(slice: &PieSlice) -> (f64, f64) {
    let mid = (slice.start_angle + slice.end_angle) / 2.0;
    let (center_x, center_y) = pie_center(slice);
    (center_x + 1.18 * mid.cos(), center_y + 1.18 * mid.sin())
}

const PIE3_HEIGHT: f64 = 0.28;

fn pie3_side_color(color: &'static str) -> String {
    css_color_rgb(color)
        .map(|rgb| rgb_string([rgb[0] * 0.72, rgb[1] * 0.72, rgb[2] * 0.72]))
        .unwrap_or_else(|| color.to_string())
}

fn pie_arg_is_text_labels(value: &Value) -> bool {
    match value {
        Value::Cell(cell) => cell.iter().all(pie_arg_is_text_labels),
        Value::Matrix(matrix) => matrix.iter().all(pie_arg_is_text_labels),
        _ => text_arg(value, "pie").is_ok(),
    }
}

fn pie_labels(
    value: &Value,
    expected_len: usize,
    builtin_name: &str,
) -> Result<Vec<String>, RuntimeError> {
    let labels = text_labels_from_value(value, builtin_name)?;
    if labels.len() != expected_len {
        return Err(RuntimeError::ShapeError(format!(
            "{builtin_name} currently expects exactly {} labels to match the data length",
            expected_len,
        )));
    }
    Ok(labels)
}

fn pie_explode(
    value: &Value,
    expected_len: usize,
    builtin_name: &str,
) -> Result<Vec<bool>, RuntimeError> {
    let values = numeric_vector(value, builtin_name)?;
    if values.len() != expected_len {
        return Err(RuntimeError::ShapeError(format!(
            "{builtin_name} currently expects exactly {} explode values to match the data length",
            expected_len,
        )));
    }
    Ok(values
        .into_iter()
        .map(|value| value.abs() > f64::EPSILON)
        .collect())
}

fn format_percentage_label(value: f64, sum: f64) -> String {
    let percent = if sum <= 1.0 {
        value * 100.0
    } else {
        (value / sum) * 100.0
    };
    format_number(percent.round())
}

fn scalar_handle(value: &Value, builtin_name: &str) -> Result<u32, RuntimeError> {
    let number = value.as_scalar()?;
    if !number.is_finite() || number < 1.0 || number.fract() != 0.0 {
        return Err(RuntimeError::TypeError(format!(
            "{builtin_name} currently expects a positive integer graphics handle"
        )));
    }
    Ok(number as u32)
}

fn scalar_usize(value: &Value, builtin_name: &str) -> Result<usize, RuntimeError> {
    let number = value.as_scalar()?;
    if !number.is_finite() || number < 1.0 || number.fract() != 0.0 {
        return Err(RuntimeError::TypeError(format!(
            "{builtin_name} currently expects positive integer scalar arguments"
        )));
    }
    Ok(number as usize)
}

fn finite_scalar_arg(value: &Value, builtin_name: &str) -> Result<f64, RuntimeError> {
    let number = value.as_scalar()?;
    if !number.is_finite() {
        return Err(RuntimeError::TypeError(format!(
            "{builtin_name} currently expects a finite numeric scalar angle"
        )));
    }
    Ok(number)
}

fn limit_value(lower: f64, upper: f64) -> Result<Value, RuntimeError> {
    Ok(Value::Matrix(MatrixValue::new(
        1,
        2,
        vec![Value::Scalar(lower), Value::Scalar(upper)],
    )?))
}

fn empty_matrix_value() -> Result<Value, RuntimeError> {
    Ok(Value::Matrix(MatrixValue::new(0, 0, Vec::new())?))
}

fn graphics_handle_vector_value(handles: Vec<u32>) -> Result<Value, RuntimeError> {
    if handles.is_empty() {
        return empty_matrix_value();
    }

    Ok(Value::Matrix(MatrixValue::new(
        1,
        handles.len(),
        handles
            .into_iter()
            .map(|handle| Value::Scalar(handle as f64))
            .collect(),
    )?))
}

fn color_property_value(color: &'static str) -> Result<Value, RuntimeError> {
    let rgb = css_color_rgb(color).ok_or_else(|| {
        RuntimeError::Unsupported(format!(
            "graphics color `{color}` is not yet representable as a MATLAB color value"
        ))
    })?;
    Ok(Value::Matrix(MatrixValue::new(
        1,
        3,
        rgb.into_iter().map(Value::Scalar).collect(),
    )?))
}

fn parse_graphics_color_input(
    value: &Value,
    builtin_name: &str,
) -> Result<&'static str, RuntimeError> {
    match value {
        Value::CharArray(text) | Value::String(text) => match text.to_ascii_lowercase().as_str() {
            "r" | "red" => Ok("#ff0000"),
            "g" | "green" => Ok("#00aa00"),
            "b" | "blue" => Ok("#0000ff"),
            "c" | "cyan" => Ok("#00cccc"),
            "m" | "magenta" => Ok("#cc00cc"),
            "y" | "yellow" => Ok("#cccc00"),
            "k" | "black" => Ok("#000000"),
            "w" | "white" => Ok("#ffffff"),
            other => Err(RuntimeError::Unsupported(format!(
                "{builtin_name} currently supports only basic MATLAB color codes/names like `r`, `g`, `b`, or `black`, found `{other}`"
            ))),
        },
        _ => {
            let values = numeric_vector(value, builtin_name)?;
            if values.len() != 3 {
                return Err(RuntimeError::ShapeError(format!(
                    "{builtin_name} currently expects color values as a 1x3 numeric vector or a basic color name"
                )));
            }
            let rgb = [values[0], values[1], values[2]];
            rgb_to_static_color(rgb).ok_or_else(|| {
                RuntimeError::Unsupported(format!(
                    "{builtin_name} currently supports only a small current subset of named RGB colors"
                ))
            })
        }
    }
}

fn parse_marker_color_input(
    value: &Value,
    builtin_name: &str,
) -> Result<MarkerColorMode, RuntimeError> {
    if is_text_keyword(value, "auto")? {
        return Ok(MarkerColorMode::Auto);
    }
    if is_text_keyword(value, "flat")? {
        return Ok(MarkerColorMode::Flat);
    }
    if is_text_keyword(value, "none")? {
        return Ok(MarkerColorMode::None);
    }
    Ok(MarkerColorMode::Fixed(parse_graphics_color_input(
        value,
        builtin_name,
    )?))
}

fn parse_line_style(value: &Value, builtin_name: &str) -> Result<LineStyle, RuntimeError> {
    match text_arg(value, builtin_name)?.to_ascii_lowercase().as_str() {
        "-" => Ok(LineStyle::Solid),
        "--" => Ok(LineStyle::Dashed),
        ":" => Ok(LineStyle::Dotted),
        "-." | "dashdot" => Ok(LineStyle::DashDot),
        "none" => Ok(LineStyle::None),
        other => Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently supports only line styles `-`, `--`, `:`, `-.`, and `none`, found `{other}`"
        ))),
    }
}

fn parse_marker_style(value: &Value, builtin_name: &str) -> Result<MarkerStyle, RuntimeError> {
    match text_arg(value, builtin_name)?.to_ascii_lowercase().as_str() {
        "none" => Ok(MarkerStyle::None),
        "." | "point" => Ok(MarkerStyle::Point),
        "o" | "circle" => Ok(MarkerStyle::Circle),
        "x" => Ok(MarkerStyle::XMark),
        "+" | "plus" => Ok(MarkerStyle::Plus),
        "*" | "star" => Ok(MarkerStyle::Star),
        "s" | "square" => Ok(MarkerStyle::Square),
        "d" | "diamond" => Ok(MarkerStyle::Diamond),
        "v" | "triangledown" => Ok(MarkerStyle::TriangleDown),
        "^" | "triangleup" => Ok(MarkerStyle::TriangleUp),
        "<" | "triangleleft" => Ok(MarkerStyle::TriangleLeft),
        ">" | "triangleright" => Ok(MarkerStyle::TriangleRight),
        "p" | "pentagram" => Ok(MarkerStyle::Pentagram),
        "h" | "hexagram" => Ok(MarkerStyle::Hexagram),
        other => Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently supports MATLAB-style markers like `.`, `o`, `x`, `+`, `*`, `s`, `d`, `^`, `v`, `<`, `>`, `p`, `h`, and `none`, found `{other}`"
        ))),
    }
}

fn parse_matlab_line_spec(
    value: &Value,
    builtin_name: &str,
) -> Result<LineSpecStyle, RuntimeError> {
    let text = text_arg(value, builtin_name)?;
    let chars = text.chars().collect::<Vec<_>>();
    let mut index = 0usize;
    let mut color = None;
    let mut line_style = None;
    let mut marker = None;

    while index < chars.len() {
        let ch = chars[index];
        if index + 1 < chars.len() && ch == '-' {
            let matched_style = match chars[index + 1] {
                '-' => Some(LineStyle::Dashed),
                '.' => Some(LineStyle::DashDot),
                _ => None,
            };
            if let Some(style) = matched_style {
                if line_style.replace(style).is_some() {
                    return Err(RuntimeError::Unsupported(format!(
                        "{builtin_name} currently supports at most one line style in a style string"
                    )));
                }
                index += 2;
                continue;
            }
        }

        match ch {
            '-' => {
                if line_style.replace(LineStyle::Solid).is_some() {
                    return Err(RuntimeError::Unsupported(format!(
                        "{builtin_name} currently supports at most one line style in a style string"
                    )));
                }
            }
            ':' => {
                if line_style.replace(LineStyle::Dotted).is_some() {
                    return Err(RuntimeError::Unsupported(format!(
                        "{builtin_name} currently supports at most one line style in a style string"
                    )));
                }
            }
            '.' => {
                if marker.replace(MarkerStyle::Point).is_some() {
                    return Err(RuntimeError::Unsupported(format!(
                        "{builtin_name} currently supports at most one marker in a style string"
                    )));
                }
            }
            'o' => {
                if marker.replace(MarkerStyle::Circle).is_some() {
                    return Err(RuntimeError::Unsupported(format!(
                        "{builtin_name} currently supports at most one marker in a style string"
                    )));
                }
            }
            'x' => {
                if marker.replace(MarkerStyle::XMark).is_some() {
                    return Err(RuntimeError::Unsupported(format!(
                        "{builtin_name} currently supports at most one marker in a style string"
                    )));
                }
            }
            '+' => {
                if marker.replace(MarkerStyle::Plus).is_some() {
                    return Err(RuntimeError::Unsupported(format!(
                        "{builtin_name} currently supports at most one marker in a style string"
                    )));
                }
            }
            '*' => {
                if marker.replace(MarkerStyle::Star).is_some() {
                    return Err(RuntimeError::Unsupported(format!(
                        "{builtin_name} currently supports at most one marker in a style string"
                    )));
                }
            }
            's' => {
                if marker.replace(MarkerStyle::Square).is_some() {
                    return Err(RuntimeError::Unsupported(format!(
                        "{builtin_name} currently supports at most one marker in a style string"
                    )));
                }
            }
            'd' => {
                if marker.replace(MarkerStyle::Diamond).is_some() {
                    return Err(RuntimeError::Unsupported(format!(
                        "{builtin_name} currently supports at most one marker in a style string"
                    )));
                }
            }
            'v' => {
                if marker.replace(MarkerStyle::TriangleDown).is_some() {
                    return Err(RuntimeError::Unsupported(format!(
                        "{builtin_name} currently supports at most one marker in a style string"
                    )));
                }
            }
            '^' => {
                if marker.replace(MarkerStyle::TriangleUp).is_some() {
                    return Err(RuntimeError::Unsupported(format!(
                        "{builtin_name} currently supports at most one marker in a style string"
                    )));
                }
            }
            '<' => {
                if marker.replace(MarkerStyle::TriangleLeft).is_some() {
                    return Err(RuntimeError::Unsupported(format!(
                        "{builtin_name} currently supports at most one marker in a style string"
                    )));
                }
            }
            '>' => {
                if marker.replace(MarkerStyle::TriangleRight).is_some() {
                    return Err(RuntimeError::Unsupported(format!(
                        "{builtin_name} currently supports at most one marker in a style string"
                    )));
                }
            }
            'p' => {
                if marker.replace(MarkerStyle::Pentagram).is_some() {
                    return Err(RuntimeError::Unsupported(format!(
                        "{builtin_name} currently supports at most one marker in a style string"
                    )));
                }
            }
            'h' => {
                if marker.replace(MarkerStyle::Hexagram).is_some() {
                    return Err(RuntimeError::Unsupported(format!(
                        "{builtin_name} currently supports at most one marker in a style string"
                    )));
                }
            }
            'r' => color = Some("#ff0000"),
            'g' => color = Some("#00aa00"),
            'b' => color = Some("#0000ff"),
            'c' => color = Some("#00cccc"),
            'm' => color = Some("#cc00cc"),
            'y' => color = Some("#cccc00"),
            'k' => color = Some("#000000"),
            'w' => color = Some("#ffffff"),
            _ => {
                return Err(RuntimeError::Unsupported(format!(
                    "{builtin_name} currently does not support the line-spec token `{ch}`"
                )));
            }
        }
        index += 1;
    }

    if marker.is_some() && line_style.is_none() {
        line_style = Some(LineStyle::None);
    }

    Ok(LineSpecStyle {
        color,
        line_style,
        marker,
    })
}

fn apply_line_spec_to_series(series: &mut PlotSeries, style: &LineSpecStyle) {
    if let Some(color) = style.color {
        series.color = color;
    }
    if let Some(line_style) = style.line_style {
        series.line_style = line_style;
    }
    if let Some(marker) = style.marker {
        series.marker = marker;
    }
}

fn rgb_to_static_color(rgb: [f64; 3]) -> Option<&'static str> {
    const COLORS: &[(&str, [f64; 3])] = &[
        ("#ff0000", [1.0, 0.0, 0.0]),
        ("#00aa00", [0.0, 170.0 / 255.0, 0.0]),
        ("#0000ff", [0.0, 0.0, 1.0]),
        ("#00cccc", [0.0, 0.8, 0.8]),
        ("#cc00cc", [0.8, 0.0, 0.8]),
        ("#cccc00", [0.8, 0.8, 0.0]),
        ("#000000", [0.0, 0.0, 0.0]),
        ("#ffffff", [1.0, 1.0, 1.0]),
        ("#1f77b4", [31.0 / 255.0, 119.0 / 255.0, 180.0 / 255.0]),
        ("#d62728", [214.0 / 255.0, 39.0 / 255.0, 40.0 / 255.0]),
        ("#2ca02c", [44.0 / 255.0, 160.0 / 255.0, 44.0 / 255.0]),
        ("#ff7f0e", [1.0, 127.0 / 255.0, 14.0 / 255.0]),
        ("#9467bd", [148.0 / 255.0, 103.0 / 255.0, 189.0 / 255.0]),
        ("#17becf", [23.0 / 255.0, 190.0 / 255.0, 207.0 / 255.0]),
        ("#8c564b", [140.0 / 255.0, 86.0 / 255.0, 75.0 / 255.0]),
        ("#222222", [34.0 / 255.0, 34.0 / 255.0, 34.0 / 255.0]),
    ];

    COLORS.iter().find_map(|(css, expected)| {
        expected
            .iter()
            .zip(rgb.iter())
            .all(|(left, right)| (*left - *right).abs() <= 1e-9)
            .then_some(*css)
    })
}

fn css_color_rgb(color: &str) -> Option<[f64; 3]> {
    let hex = color.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let red = u8::from_str_radix(&hex[0..2], 16).ok()? as f64 / 255.0;
    let green = u8::from_str_radix(&hex[2..4], 16).ok()? as f64 / 255.0;
    let blue = u8::from_str_radix(&hex[4..6], 16).ok()? as f64 / 255.0;
    Some([red, green, blue])
}

fn logical_query_value(
    flags: Vec<bool>,
    rows: usize,
    cols: usize,
    scalar_input: bool,
) -> Result<Value, RuntimeError> {
    if scalar_input {
        return Ok(Value::Logical(flags.first().copied().unwrap_or(false)));
    }

    Ok(Value::Matrix(MatrixValue::new(
        rows,
        cols,
        flags.into_iter().map(Value::Logical).collect(),
    )?))
}

fn on_off_value(enabled: bool) -> Value {
    Value::CharArray(if enabled { "on" } else { "off" }.to_string())
}

fn on_off_flag(value: &Value, builtin_name: &str) -> Result<bool, RuntimeError> {
    match value {
        Value::Logical(flag) => Ok(*flag),
        Value::CharArray(text) | Value::String(text) => match text.to_ascii_lowercase().as_str() {
            "on" => Ok(true),
            "off" => Ok(false),
            other => Err(RuntimeError::Unsupported(format!(
                "{builtin_name} currently expects `on` or `off` text values, found `{other}`"
            ))),
        },
        _ => Err(RuntimeError::TypeError(format!(
            "{builtin_name} currently expects logical or text `on`/`off` property values"
        ))),
    }
}

fn tick_values_value(values: &[f64]) -> Result<Value, RuntimeError> {
    if values.is_empty() {
        return Ok(Value::Matrix(MatrixValue::new(0, 0, Vec::new())?));
    }

    Ok(Value::Matrix(MatrixValue::new(
        1,
        values.len(),
        values.iter().copied().map(Value::Scalar).collect(),
    )?))
}

fn tick_labels_value(labels: &[String]) -> Result<Value, RuntimeError> {
    if labels.is_empty() {
        return Ok(Value::Matrix(MatrixValue::new(0, 0, Vec::new())?));
    }

    Ok(Value::Matrix(MatrixValue::new(
        1,
        labels.len(),
        labels
            .iter()
            .cloned()
            .map(Value::String)
            .collect::<Vec<_>>(),
    )?))
}

fn tick_vector(value: &Value, builtin_name: &str) -> Result<Vec<f64>, RuntimeError> {
    match value {
        Value::Matrix(matrix) if matrix.elements.is_empty() => Ok(Vec::new()),
        _ => numeric_vector(value, builtin_name),
    }
}

fn view_value(axes: &AxesState) -> Result<Value, RuntimeError> {
    Ok(Value::Matrix(MatrixValue::new(
        1,
        2,
        vec![
            Value::Scalar(axes.view_azimuth),
            Value::Scalar(axes.view_elevation),
        ],
    )?))
}

fn axis_value(axes: &AxesState) -> Result<Value, RuntimeError> {
    let three_d_range = axes_three_d_range(axes);
    let ((x_min, x_max), (y_min, y_max)) =
        resolved_limits_with_three_d(axes, three_d_range.as_ref());
    Ok(Value::Matrix(MatrixValue::new(
        1,
        4,
        vec![
            Value::Scalar(x_min),
            Value::Scalar(x_max),
            Value::Scalar(y_min),
            Value::Scalar(y_max),
        ],
    )?))
}

fn text_arg(value: &Value, builtin_name: &str) -> Result<String, RuntimeError> {
    match value {
        Value::CharArray(text) | Value::String(text) => Ok(text.clone()),
        _ => Err(RuntimeError::TypeError(format!(
            "{builtin_name} currently expects char or string text arguments"
        ))),
    }
}

fn text_labels_from_value(value: &Value, builtin_name: &str) -> Result<Vec<String>, RuntimeError> {
    match value {
        Value::Cell(cell) => cell
            .elements
            .iter()
            .map(|entry| text_arg(entry, builtin_name))
            .collect(),
        Value::Matrix(matrix) => matrix
            .elements
            .iter()
            .map(|entry| text_arg(entry, builtin_name))
            .collect(),
        other => Ok(vec![text_arg(other, builtin_name)?]),
    }
}

fn is_text_keyword(value: &Value, keyword: &str) -> Result<bool, RuntimeError> {
    match value {
        Value::CharArray(text) | Value::String(text) => Ok(text.eq_ignore_ascii_case(keyword)),
        _ => Ok(false),
    }
}

fn parse_axis_scale(value: &Value, builtin_name: &str) -> Result<AxisScale, RuntimeError> {
    match text_arg(value, builtin_name)?.to_ascii_lowercase().as_str() {
        "linear" => Ok(AxisScale::Linear),
        "log" => Ok(AxisScale::Log),
        other => Err(RuntimeError::Unsupported(format!(
            "{builtin_name} currently supports only `linear` or `log`, found `{other}`"
        ))),
    }
}

fn is_view_preset(value: &Value, preset: f64) -> Result<bool, RuntimeError> {
    match value {
        Value::Scalar(number) => Ok((*number - preset).abs() <= f64::EPSILON),
        Value::Logical(flag) => {
            let number = if *flag { 1.0 } else { 0.0 };
            Ok((number - preset).abs() <= f64::EPSILON)
        }
        Value::Matrix(matrix) if matrix.rows == 1 && matrix.cols == 1 => {
            Ok((matrix.get(0, 0).as_scalar()? - preset).abs() <= f64::EPSILON)
        }
        _ => Ok(false),
    }
}

fn default_legend_labels(axes: &AxesState) -> Vec<String> {
    axes.series
        .iter()
        .enumerate()
        .map(|(index, series)| {
            series
                .display_name
                .clone()
                .filter(|label| !label.is_empty())
                .unwrap_or_else(|| format!("Data {}", index + 1))
        })
        .collect()
}

fn parse_colormap_kind(value: &Value) -> Result<ColormapKind, RuntimeError> {
    match text_arg(value, "colormap")?.to_ascii_lowercase().as_str() {
        "parula" => Ok(ColormapKind::Parula),
        "gray" | "grey" => Ok(ColormapKind::Gray),
        "hot" => Ok(ColormapKind::Hot),
        "jet" => Ok(ColormapKind::Jet),
        "cool" => Ok(ColormapKind::Cool),
        "spring" => Ok(ColormapKind::Spring),
        "summer" => Ok(ColormapKind::Summer),
        "autumn" => Ok(ColormapKind::Autumn),
        "winter" => Ok(ColormapKind::Winter),
        other => Err(RuntimeError::Unsupported(format!(
            "colormap currently supports only `parula`, `gray`, `hot`, `jet`, `cool`, `spring`, `summer`, `autumn`, or `winter`, found `{other}`"
        ))),
    }
}

fn parse_shading_mode(value: &Value) -> Result<ShadingMode, RuntimeError> {
    match text_arg(value, "shading")?.to_ascii_lowercase().as_str() {
        "faceted" => Ok(ShadingMode::Faceted),
        "flat" => Ok(ShadingMode::Flat),
        "interp" => Ok(ShadingMode::Interp),
        other => Err(RuntimeError::Unsupported(format!(
            "shading currently supports only `faceted`, `flat`, or `interp`, found `{other}`"
        ))),
    }
}

fn colormap_outputs(kind: ColormapKind, output_arity: usize) -> Result<Vec<Value>, RuntimeError> {
    one_or_zero_outputs(colormap_matrix_value(kind)?, output_arity, "colormap")
}

fn colormap_matrix_value(kind: ColormapKind) -> Result<Value, RuntimeError> {
    let elements = colormap_palette(kind)
        .iter()
        .flat_map(|[r, g, b]| [Value::Scalar(*r), Value::Scalar(*g), Value::Scalar(*b)])
        .collect::<Vec<_>>();
    Ok(Value::Matrix(MatrixValue::new(
        colormap_palette(kind).len(),
        3,
        elements,
    )?))
}

fn colormap_palette(kind: ColormapKind) -> &'static [[f64; 3]] {
    match kind {
        ColormapKind::Parula => &[
            [0.2081, 0.1663, 0.5292],
            [0.1180, 0.2870, 0.7130],
            [0.0000, 0.4670, 0.7610],
            [0.0900, 0.6320, 0.6750],
            [0.3690, 0.7880, 0.3820],
            [0.6780, 0.8630, 0.1890],
            [0.9020, 0.8790, 0.1950],
            [0.9930, 0.9060, 0.1440],
        ],
        ColormapKind::Gray => &[
            [0.0, 0.0, 0.0],
            [0.143, 0.143, 0.143],
            [0.286, 0.286, 0.286],
            [0.429, 0.429, 0.429],
            [0.571, 0.571, 0.571],
            [0.714, 0.714, 0.714],
            [0.857, 0.857, 0.857],
            [1.0, 1.0, 1.0],
        ],
        ColormapKind::Hot => &[
            [0.0416, 0.0, 0.0],
            [0.35, 0.0, 0.0],
            [0.65, 0.1, 0.0],
            [0.9, 0.3, 0.0],
            [1.0, 0.55, 0.0],
            [1.0, 0.8, 0.15],
            [1.0, 0.95, 0.55],
            [1.0, 1.0, 1.0],
        ],
        ColormapKind::Jet => &[
            [0.0, 0.0, 0.5],
            [0.0, 0.0, 1.0],
            [0.0, 0.5, 1.0],
            [0.0, 1.0, 1.0],
            [0.5, 1.0, 0.5],
            [1.0, 1.0, 0.0],
            [1.0, 0.5, 0.0],
            [0.5, 0.0, 0.0],
        ],
        ColormapKind::Cool => &[
            [0.0, 1.0, 1.0],
            [0.143, 0.857, 1.0],
            [0.286, 0.714, 1.0],
            [0.429, 0.571, 1.0],
            [0.571, 0.429, 1.0],
            [0.714, 0.286, 1.0],
            [0.857, 0.143, 1.0],
            [1.0, 0.0, 1.0],
        ],
        ColormapKind::Spring => &[
            [1.0, 0.0, 1.0],
            [1.0, 0.143, 0.857],
            [1.0, 0.286, 0.714],
            [1.0, 0.429, 0.571],
            [1.0, 0.571, 0.429],
            [1.0, 0.714, 0.286],
            [1.0, 0.857, 0.143],
            [1.0, 1.0, 0.0],
        ],
        ColormapKind::Summer => &[
            [0.0, 0.5, 0.4],
            [0.143, 0.571, 0.4],
            [0.286, 0.643, 0.4],
            [0.429, 0.714, 0.4],
            [0.571, 0.786, 0.4],
            [0.714, 0.857, 0.4],
            [0.857, 0.929, 0.4],
            [1.0, 1.0, 0.4],
        ],
        ColormapKind::Autumn => &[
            [1.0, 0.0, 0.0],
            [1.0, 0.143, 0.0],
            [1.0, 0.286, 0.0],
            [1.0, 0.429, 0.0],
            [1.0, 0.571, 0.0],
            [1.0, 0.714, 0.0],
            [1.0, 0.857, 0.0],
            [1.0, 1.0, 0.0],
        ],
        ColormapKind::Winter => &[
            [0.0, 0.0, 1.0],
            [0.0, 0.143, 0.929],
            [0.0, 0.286, 0.857],
            [0.0, 0.429, 0.786],
            [0.0, 0.571, 0.714],
            [0.0, 0.714, 0.643],
            [0.0, 0.857, 0.571],
            [0.0, 1.0, 0.5],
        ],
    }
}

fn sample_colormap(kind: ColormapKind, normalized: f64) -> String {
    let palette = colormap_palette(kind);
    let clamped = normalized.clamp(0.0, 1.0);
    let scaled = clamped * (palette.len().saturating_sub(1) as f64);
    let lower = scaled.floor() as usize;
    let upper = scaled.ceil() as usize;
    if lower == upper {
        return rgb_string(palette[lower]);
    }

    let fraction = scaled - lower as f64;
    let mut blended = [0.0; 3];
    for channel in 0..3 {
        blended[channel] = palette[lower][channel]
            + (palette[upper][channel] - palette[lower][channel]) * fraction;
    }
    rgb_string(blended)
}

fn rgb_string(rgb: [f64; 3]) -> String {
    let components = rgb.map(|channel| (channel.clamp(0.0, 1.0) * 255.0).round() as u8);
    format!("rgb({},{},{})", components[0], components[1], components[2])
}

fn render_figure_svg(figure: &FigureState) -> String {
    render_figure_svg_with_size(figure, DEFAULT_RENDER_WIDTH, DEFAULT_RENDER_HEIGHT)
}

fn figure_window_title(handle: u32, figure: &FigureState) -> String {
    if figure.number_title {
        if figure.name.is_empty() {
            format!("Figure {handle}")
        } else {
            format!("Figure {handle}: {}", figure.name)
        }
    } else if figure.name.is_empty() {
        format!("Figure {handle}")
    } else {
        figure.name.clone()
    }
}

fn render_figure_svg_with_size(figure: &FigureState, width: f64, height: f64) -> String {
    let outer_left = 36.0;
    let outer_right = 24.0;
    let outer_top = 28.0;
    let outer_bottom = 30.0;
    let super_title_reserved = if figure.super_title.is_empty() {
        0.0
    } else {
        28.0
    };
    let gap_x = 28.0;
    let gap_y = 26.0;
    let cols = figure.layout_cols.max(1);
    let rows = figure.layout_rows.max(1);
    let usable_width = width - outer_left - outer_right - gap_x * (cols.saturating_sub(1) as f64);
    let usable_height = height
        - outer_top
        - outer_bottom
        - super_title_reserved
        - gap_y * (rows.saturating_sub(1) as f64);
    let cell_width = usable_width / cols as f64;
    let cell_height = usable_height / rows as f64;

    let mut out = String::new();
    out.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\">\n"
    ));
    out.push_str("  <rect width=\"100%\" height=\"100%\" fill=\"white\"/>\n");
    if !figure.super_title.is_empty() {
        out.push_str(&format!(
            "  <text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-size=\"20\" font-family=\"Segoe UI, Arial, sans-serif\" fill=\"#222222\">{}</text>\n",
            format_number(width / 2.0),
            format_number(24.0),
            svg_escape(&figure.super_title)
        ));
    }

    let mut rendered = std::collections::BTreeSet::new();
    for index in 1..=(rows * cols) {
        let row = (index - 1) / cols;
        let col = (index - 1) % cols;
        let grid_frame = AxesFrame {
            left: outer_left + col as f64 * (cell_width + gap_x),
            top: outer_top + super_title_reserved + row as f64 * (cell_height + gap_y),
            width: cell_width,
            height: cell_height,
            x_scale: AxisScale::Linear,
            y_scale: AxisScale::Linear,
        };
        let (frame, axes) = figure
            .axes
            .get(&index)
            .map(|slot| {
                (
                    slot.axes
                        .position
                        .map(|position| axes_frame_from_position(position, width, height))
                        .unwrap_or(grid_frame),
                    slot.axes.clone(),
                )
            })
            .unwrap_or((grid_frame, AxesState::default()));
        rendered.insert(index);
        let axes_handle = figure.axes.get(&index).map(|slot| slot.handle);
        render_axes_svg(&mut out, figure, axes_handle, &axes, frame);
    }

    for (index, slot) in &figure.axes {
        if rendered.contains(index) {
            continue;
        }
        let frame = slot
            .axes
            .position
            .map(|position| axes_frame_from_position(position, width, height))
            .unwrap_or(AxesFrame {
                left: outer_left,
                top: outer_top + super_title_reserved,
                width: usable_width,
                height: usable_height,
                x_scale: AxisScale::Linear,
                y_scale: AxisScale::Linear,
            });
        render_axes_svg(&mut out, figure, Some(slot.handle), &slot.axes, frame);
    }

    render_figure_annotations(&mut out, figure, width, height);

    out.push_str("</svg>\n");
    out
}

pub(crate) fn rendered_figures(state: &GraphicsState) -> Vec<RenderedFigure> {
    state
        .figures
        .iter()
        .map(|(handle, figure)| RenderedFigure {
            handle: *handle,
            title: figure_window_title(*handle, figure),
            visible: figure.visible,
            position: figure.position,
            window_style: figure.window_style.as_text().to_string(),
            svg: render_figure_svg(figure),
        })
        .collect()
}

pub(crate) fn apply_backend_figure_position(
    state: &mut GraphicsState,
    handle: u32,
    position: [f64; 4],
) -> Result<(), RuntimeError> {
    let figure = state.figures.get_mut(&handle).ok_or_else(|| {
        RuntimeError::MissingVariable(format!("figure handle `{handle}` does not exist"))
    })?;
    figure.position = position;
    Ok(())
}

pub(crate) fn figure_close_request_callback(
    state: &GraphicsState,
    handle: u32,
) -> Result<Option<Value>, RuntimeError> {
    let figure = state.figures.get(&handle).ok_or_else(|| {
        RuntimeError::MissingVariable(format!("figure handle `{handle}` does not exist"))
    })?;
    Ok(figure.close_request_fcn.clone())
}

pub(crate) fn figure_resize_callback_snapshot(
    state: &GraphicsState,
) -> BTreeMap<u32, ([f64; 4], Option<Value>)> {
    state
        .figures
        .iter()
        .map(|(handle, figure)| (*handle, (figure.position, figure.resize_fcn.clone())))
        .collect()
}

pub(crate) fn select_current_figure_handle(
    state: &mut GraphicsState,
    handle: u32,
) -> Result<(), RuntimeError> {
    if !state.figures.contains_key(&handle) {
        return Err(RuntimeError::MissingVariable(format!(
            "figure handle `{handle}` does not exist"
        )));
    }
    state.current_figure = Some(handle);
    Ok(())
}

pub(crate) fn close_figures_now(
    state: &mut GraphicsState,
    handles: &[u32],
) -> Result<(), RuntimeError> {
    for handle in handles {
        if state.figures.contains_key(handle) {
            delete_graphics_handle(state, *handle)?;
        }
    }
    Ok(())
}

fn axes_svg_group_attributes(
    figure: &FigureState,
    axes_handle: Option<u32>,
    axes: &AxesState,
    plot_frame: AxesFrame,
    x_limits: (f64, f64),
    y_limits: (f64, f64),
    three_d_range: Option<ThreeDRange>,
) -> String {
    let mut attributes = String::from(" class=\"matc-axes\"");
    if let Some(handle) = axes_handle {
        attributes.push_str(&format!(" data-matc-handle=\"{handle}\""));
        if let Some((group_index, mode)) = figure
            .linked_axes
            .iter()
            .enumerate()
            .find(|(_, group)| group.handles.contains(&handle))
            .map(|(index, group)| (index + 1, group.mode))
        {
            attributes.push_str(&format!(
                " data-matc-link-group=\"{}\" data-matc-link-mode=\"{}\"",
                group_index,
                mode.as_text()
            ));
        }
    }
    attributes.push_str(&format!(
        " data-matc-plot-frame=\"{},{},{},{}\" data-matc-xlim=\"{},{}\" data-matc-base-xlim=\"{},{}\" data-matc-ylim=\"{},{}\" data-matc-base-ylim=\"{},{}\" data-matc-xscale=\"{}\" data-matc-yscale=\"{}\" data-matc-grid=\"{}\"",
        format_number(plot_frame.left),
        format_number(plot_frame.top),
        format_number(plot_frame.width),
        format_number(plot_frame.height),
        format_number(x_limits.0),
        format_number(x_limits.1),
        format_number(x_limits.0),
        format_number(x_limits.1),
        format_number(y_limits.0),
        format_number(y_limits.1),
        format_number(y_limits.0),
        format_number(y_limits.1),
        axes.x_scale.as_text(),
        current_y_scale_for_side(axes, YAxisSide::Left).as_text(),
        if axes.grid_enabled { "on" } else { "off" }
    ));
    if let Some(range) = three_d_range {
        attributes.push_str(" data-matc-3d=\"true\"");
        attributes.push_str(&format!(
            " data-matc-view=\"{},{}\" data-matc-base-view=\"{},{}\" data-matc-3d-range=\"{},{},{},{},{},{}\"",
            format_number(axes.view_azimuth),
            format_number(axes.view_elevation),
            format_number(axes.view_azimuth),
            format_number(axes.view_elevation),
            format_number(range.x_range.0),
            format_number(range.x_range.1),
            format_number(range.y_range.0),
            format_number(range.y_range.1),
            format_number(range.z_range.0),
            format_number(range.z_range.1),
        ));
    }
    attributes
}

fn render_axes_svg(
    out: &mut String,
    figure: &FigureState,
    axes_handle: Option<u32>,
    axes: &AxesState,
    frame: AxesFrame,
) {
    let has_right_axis = axes_has_right_y_axis(axes);
    let subtitle_reserved = if axes.subtitle.is_empty() { 0.0 } else { 18.0 };
    let inner = AxesFrame {
        left: frame.left + 56.0,
        top: frame.top + 28.0 + subtitle_reserved,
        width: frame.width - if has_right_axis { 112.0 } else { 72.0 },
        height: frame.height - 78.0 - subtitle_reserved,
        x_scale: axes.x_scale,
        y_scale: axes.y_scale,
    };
    let mut plot_frame = inner;
    let colorbar_frame = if axes.colorbar_enabled {
        plot_frame.width = (plot_frame.width - 42.0).max(40.0);
        Some(AxesFrame {
            left: plot_frame.right() + 14.0,
            top: plot_frame.top + 6.0,
            width: 18.0,
            height: (plot_frame.height - 12.0).max(24.0),
            x_scale: AxisScale::Linear,
            y_scale: AxisScale::Linear,
        })
    } else {
        None
    };
    let three_d_range = axes_three_d_range(axes);
    let ((x_min, x_max), (left_y_min, left_y_max)) =
        resolved_limits_for_side(axes, YAxisSide::Left);
    let right_y_limits = resolved_y_limits_for_side(axes, YAxisSide::Right);
    plot_frame = adjusted_plot_frame(axes, plot_frame, x_min, x_max, left_y_min, left_y_max);
    let axes_group_attributes = axes_svg_group_attributes(
        figure,
        axes_handle,
        axes,
        plot_frame,
        (x_min, x_max),
        (left_y_min, left_y_max),
        three_d_range,
    );
    out.push_str(&format!("  <g{}>\n", axes_group_attributes));
    let clip_id = axes_plot_clip_id(axes_handle, plot_frame);
    out.push_str("  <defs>\n");
    out.push_str(&format!(
        "    <clipPath id=\"{}\"><rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/></clipPath>\n",
        clip_id,
        format_number(plot_frame.left),
        format_number(plot_frame.top),
        format_number(plot_frame.width),
        format_number(plot_frame.height)
    ));
    out.push_str("  </defs>\n");

    if axes.axis_visible {
        out.push_str(&format!(
            "  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"#fafafa\" stroke=\"#d0d0d0\"/>\n",
            format_number(frame.left),
            format_number(frame.top),
            format_number(frame.width),
            format_number(frame.height)
        ));
        out.push_str(&format!(
            "  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"white\" stroke=\"{}\"/>\n",
            format_number(plot_frame.left),
            format_number(plot_frame.top),
            format_number(plot_frame.width),
            format_number(plot_frame.height),
            if axes.box_enabled { "#cccccc" } else { "none" }
        ));
        out.push_str(&format!(
            "  <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#333333\" stroke-width=\"1.4\"/>\n",
            format_number(plot_frame.left),
            format_number(plot_frame.bottom()),
            format_number(plot_frame.right()),
            format_number(plot_frame.bottom())
        ));
        out.push_str(&format!(
            "  <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#333333\" stroke-width=\"1.4\"/>\n",
            format_number(plot_frame.left),
            format_number(plot_frame.top),
            format_number(plot_frame.left),
            format_number(plot_frame.bottom())
        ));

        let x_ticks = resolved_ticks(axes, TickKind::X);
        let x_labels = match &axes.xtick_labels {
            Some(labels) => Some(labels.clone()),
            None => None,
        };
        for (index, x_tick) in x_ticks.into_iter().enumerate() {
            if !tick_in_range(x_tick, x_min, x_max) {
                continue;
            }
            let x = scale_x(x_tick, x_min, x_max, plot_frame);
            if axes.grid_enabled {
                out.push_str(&format!(
                    "  <line class=\"matc-x-grid\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#ececec\" stroke-width=\"1\"/>\n",
                    format_number(x),
                    format_number(plot_frame.bottom()),
                    format_number(x),
                    format_number(plot_frame.top)
                ));
            }
            let label = match &x_labels {
                Some(labels) if labels.is_empty() => None,
                Some(labels) => labels.get(index).cloned(),
                None => default_tick_label_for_scale(x_tick, axes.x_scale),
            };
            if let Some(label) = label {
                render_tick_text_svg_with_class(
                    out,
                    x,
                    plot_frame.bottom() + 20.0,
                    "middle",
                    &label,
                    resolved_tick_angle(axes, TickKind::X),
                    Some("matc-x-tick-label"),
                );
            }
        }
        let y_ticks = resolved_ticks_for_side(axes, TickKind::Y, YAxisSide::Left);
        let y_labels = resolved_tick_labels_for_side(axes, TickKind::Y, YAxisSide::Left);
        let left_plot_frame =
            plot_frame.with_y_scale(current_y_scale_for_side(axes, YAxisSide::Left));
        for (index, y_tick) in y_ticks.into_iter().enumerate() {
            if !tick_in_range(y_tick, left_y_min, left_y_max) {
                continue;
            }
            let y = scale_y(y_tick, left_y_min, left_y_max, left_plot_frame);
            if axes.grid_enabled {
                out.push_str(&format!(
                    "  <line class=\"matc-y-grid\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#ececec\" stroke-width=\"1\"/>\n",
                    format_number(left_plot_frame.left),
                    format_number(y),
                    format_number(left_plot_frame.right()),
                    format_number(y)
                ));
            }
            if let Some(label) = y_labels.get(index) {
                render_tick_text_svg_with_class(
                    out,
                    plot_frame.left - 8.0,
                    y + 4.0,
                    "end",
                    label,
                    resolved_tick_angle_for_side(axes, TickKind::Y, YAxisSide::Left),
                    Some("matc-y-tick-label"),
                );
            }
        }
        if has_right_axis {
            out.push_str(&format!(
                "  <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#333333\" stroke-width=\"1.4\"/>\n",
                format_number(plot_frame.right()),
                format_number(plot_frame.top),
                format_number(plot_frame.right()),
                format_number(plot_frame.bottom())
            ));
            let y_ticks_right = resolved_ticks_for_side(axes, TickKind::Y, YAxisSide::Right);
            let y_labels_right = resolved_tick_labels_for_side(axes, TickKind::Y, YAxisSide::Right);
            let right_plot_frame =
                plot_frame.with_y_scale(current_y_scale_for_side(axes, YAxisSide::Right));
            for (index, y_tick) in y_ticks_right.into_iter().enumerate() {
                if !tick_in_range(y_tick, right_y_limits.0, right_y_limits.1) {
                    continue;
                }
                let y = scale_y(y_tick, right_y_limits.0, right_y_limits.1, right_plot_frame);
                if let Some(label) = y_labels_right.get(index) {
                    render_tick_text_svg(
                        out,
                        right_plot_frame.right() + 8.0,
                        y + 4.0,
                        "start",
                        label,
                        resolved_tick_angle_for_side(axes, TickKind::Y, YAxisSide::Right),
                    );
                }
            }
        }
        if three_d_range.is_some() {
            let (z_min, z_max) = resolved_z_limits(axes);
            let tick_x2 = plot_frame.right() + 4.0;
            let label_x = plot_frame.right() + 8.0;
            let z_ticks = resolved_ticks(axes, TickKind::Z);
            let z_labels = match &axes.ztick_labels {
                Some(labels) => Some(labels.clone()),
                None => None,
            };
            let z_plot_frame = plot_frame.with_y_scale(AxisScale::Linear);
            for (index, z_tick) in z_ticks.into_iter().enumerate() {
                if !tick_in_range(z_tick, z_min, z_max) {
                    continue;
                }
                let y = scale_y(z_tick, z_min, z_max, z_plot_frame);
                out.push_str(&format!(
                    "  <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#666666\" stroke-width=\"1\"/>\n",
                    format_number(plot_frame.right()),
                    format_number(y),
                    format_number(tick_x2),
                    format_number(y)
                ));
                let label = match &z_labels {
                    Some(labels) if labels.is_empty() => None,
                    Some(labels) => labels.get(index).cloned(),
                    None => Some(format_number(z_tick)),
                };
                if let Some(label) = label {
                    render_tick_text_svg(
                        out,
                        label_x,
                        y + 4.0,
                        "start",
                        &label,
                        resolved_tick_angle(axes, TickKind::Z),
                    );
                }
            }
        }
    } else {
        out.push_str(&format!(
            "  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"white\" stroke=\"none\"/>\n",
            format_number(plot_frame.left),
            format_number(plot_frame.top),
            format_number(plot_frame.width),
            format_number(plot_frame.height)
        ));
    }

    out.push_str(&format!("  <g clip-path=\"url(#{})\">\n", clip_id));
    for series in axes.series.iter().filter(|series| series.visible) {
        let (series_y_min, series_y_max) =
            if series_uses_secondary_y_axis(series) && series.y_axis_side == YAxisSide::Right {
                right_y_limits
            } else {
                (left_y_min, left_y_max)
            };
        let series_frame =
            plot_frame.with_y_scale(current_y_scale_for_side(axes, series.y_axis_side));
        render_series_svg(
            out,
            series,
            axes,
            series_frame,
            x_min,
            x_max,
            series_y_min,
            series_y_max,
            axes.colormap,
            axes.caxis,
            three_d_range.as_ref(),
        );
    }
    out.push_str("  </g>\n");

    render_legend_svg(out, axes, plot_frame);
    if let Some(colorbar_frame) = colorbar_frame {
        render_colorbar_svg(out, axes, colorbar_frame);
    }

    if !axes.title.is_empty() {
        out.push_str(&format!(
            "  <text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-size=\"17\" font-family=\"Segoe UI, Arial, sans-serif\" fill=\"#222222\">{}</text>\n",
            format_number(frame.left + frame.width / 2.0),
            format_number(frame.top + 18.0),
            svg_escape(&axes.title)
        ));
    }
    if !axes.subtitle.is_empty() {
        out.push_str(&format!(
            "  <text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-size=\"12\" font-family=\"Segoe UI, Arial, sans-serif\" fill=\"#5c6673\">{}</text>\n",
            format_number(frame.left + frame.width / 2.0),
            format_number(frame.top + if axes.title.is_empty() { 18.0 } else { 34.0 }),
            svg_escape(&axes.subtitle)
        ));
    }
    if axes.axis_visible && !axes.xlabel.is_empty() {
        out.push_str(&format!(
            "  <text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-size=\"14\" font-family=\"Segoe UI, Arial, sans-serif\" fill=\"#222222\">{}</text>\n",
            format_number(frame.left + frame.width / 2.0),
            format_number(frame.bottom() - 8.0),
            svg_escape(&axes.xlabel)
        ));
    }
    if axes.axis_visible && !axes.ylabel.is_empty() {
        let y_center = frame.top + frame.height / 2.0;
        out.push_str(&format!(
            "  <text x=\"{}\" y=\"{}\" transform=\"rotate(-90 {} {})\" text-anchor=\"middle\" font-size=\"14\" font-family=\"Segoe UI, Arial, sans-serif\" fill=\"#222222\">{}</text>\n",
            format_number(frame.left + 16.0),
            format_number(y_center),
            format_number(frame.left + 16.0),
            format_number(y_center),
            svg_escape(&axes.ylabel)
        ));
    }
    if axes.axis_visible && has_right_axis && !axes.ylabel_right.is_empty() {
        let y_center = frame.top + frame.height / 2.0;
        let x = frame.right() - 10.0;
        out.push_str(&format!(
            "  <text x=\"{}\" y=\"{}\" transform=\"rotate(-90 {} {})\" text-anchor=\"middle\" font-size=\"14\" font-family=\"Segoe UI, Arial, sans-serif\" fill=\"#222222\">{}</text>\n",
            format_number(x),
            format_number(y_center),
            format_number(x),
            format_number(y_center),
            svg_escape(&axes.ylabel_right)
        ));
    }
    if axes.axis_visible && three_d_range.is_some() && !axes.zlabel.is_empty() {
        let y_center = frame.top + frame.height / 2.0;
        let x = if let Some(colorbar) = colorbar_frame {
            (colorbar.right() + frame.right()) / 2.0
        } else {
            frame.right() - 14.0
        };
        out.push_str(&format!(
            "  <text x=\"{}\" y=\"{}\" transform=\"rotate(-90 {} {})\" text-anchor=\"middle\" font-size=\"14\" font-family=\"Segoe UI, Arial, sans-serif\" fill=\"#222222\">{}</text>\n",
            format_number(x),
            format_number(y_center),
            format_number(x),
            format_number(y_center),
            svg_escape(&axes.zlabel)
        ));
    }
    out.push_str("  </g>\n");
}

fn axes_plot_clip_id(axes_handle: Option<u32>, plot_frame: AxesFrame) -> String {
    match axes_handle {
        Some(handle) => format!("matc-axes-clip-{handle}"),
        None => format!(
            "matc-axes-clip-{}-{}-{}-{}",
            (plot_frame.left * 10.0).round() as i64,
            (plot_frame.top * 10.0).round() as i64,
            (plot_frame.width * 10.0).round() as i64,
            (plot_frame.height * 10.0).round() as i64
        ),
    }
}

fn render_figure_annotations(out: &mut String, figure: &FigureState, width: f64, height: f64) {
    for annotation in figure
        .annotations
        .iter()
        .filter(|annotation| annotation.visible)
    {
        match annotation.kind {
            AnnotationKind::Line => render_annotation_line(out, annotation, width, height),
            AnnotationKind::Arrow => render_annotation_arrow(out, annotation, width, height, false),
            AnnotationKind::DoubleArrow => {
                render_annotation_arrow(out, annotation, width, height, true)
            }
            AnnotationKind::TextArrow => {
                render_annotation_text_arrow(out, annotation, width, height)
            }
            AnnotationKind::TextBox => render_annotation_textbox(out, annotation, width, height),
            AnnotationKind::Rectangle => render_annotation_rect(out, annotation, width, height),
            AnnotationKind::Ellipse => render_annotation_ellipse(out, annotation, width, height),
        }
    }
}

fn annotation_line_points(
    annotation: &AnnotationObject,
    width: f64,
    height: f64,
) -> ((f64, f64), (f64, f64)) {
    let x1 = annotation.x.get(0).copied().unwrap_or(0.3) * width;
    let x2 = annotation.x.get(1).copied().unwrap_or(0.4) * width;
    let y1 = height - annotation.y.get(0).copied().unwrap_or(0.3) * height;
    let y2 = height - annotation.y.get(1).copied().unwrap_or(0.4) * height;
    ((x1, y1), (x2, y2))
}

fn annotation_rect_geometry(
    annotation: &AnnotationObject,
    width: f64,
    height: f64,
) -> (f64, f64, f64, f64) {
    let position = annotation.position.unwrap_or([0.3, 0.3, 0.1, 0.1]);
    let left = position[0] * width;
    let box_width = position[2] * width;
    let box_height = position[3] * height;
    let top = height - (position[1] + position[3]) * height;
    (left, top, box_width, box_height)
}

fn render_annotation_line(
    out: &mut String,
    annotation: &AnnotationObject,
    width: f64,
    height: f64,
) {
    let ((x1, y1), (x2, y2)) = annotation_line_points(annotation, width, height);
    let dash = annotation
        .line_style
        .stroke_dasharray()
        .map(|dash| format!(" stroke-dasharray=\"{dash}\""))
        .unwrap_or_default();
    out.push_str(&format!(
        "  <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"{}\"{} stroke-linecap=\"round\"/>\n",
        format_number(x1),
        format_number(y1),
        format_number(x2),
        format_number(y2),
        annotation.color,
        format_number(annotation.line_width),
        dash
    ));
}

fn render_annotation_arrow(
    out: &mut String,
    annotation: &AnnotationObject,
    width: f64,
    height: f64,
    both_ends: bool,
) {
    let ((x1, y1), (x2, y2)) = annotation_line_points(annotation, width, height);
    render_annotation_line(out, annotation, width, height);
    render_arrow_head(out, x1, y1, x2, y2, annotation.color, annotation.line_width);
    if both_ends {
        render_arrow_head(out, x2, y2, x1, y1, annotation.color, annotation.line_width);
    }
}

fn render_annotation_text_arrow(
    out: &mut String,
    annotation: &AnnotationObject,
    width: f64,
    height: f64,
) {
    render_annotation_arrow(out, annotation, width, height, false);
    if annotation.text.is_empty() {
        return;
    }
    let ((x1, y1), _) = annotation_line_points(annotation, width, height);
    out.push_str(&format!(
        "  <text x=\"{}\" y=\"{}\" font-size=\"{}\" font-family=\"Segoe UI, Arial, sans-serif\" fill=\"{}\">{}</text>\n",
        format_number(x1 + 6.0),
        format_number(y1 - 6.0),
        format_number(annotation.font_size),
        annotation.color,
        svg_escape(&annotation.text)
    ));
}

fn render_annotation_textbox(
    out: &mut String,
    annotation: &AnnotationObject,
    width: f64,
    height: f64,
) {
    let (left, top, box_width, box_height) = annotation_rect_geometry(annotation, width, height);
    let dash = annotation
        .line_style
        .stroke_dasharray()
        .map(|dash| format!(" stroke-dasharray=\"{dash}\""))
        .unwrap_or_default();
    out.push_str(&format!(
        "  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\"{}/>\n",
        format_number(left),
        format_number(top),
        format_number(box_width),
        format_number(box_height),
        annotation.face_color.unwrap_or("white"),
        annotation.color,
        format_number(annotation.line_width),
        dash
    ));
    if !annotation.text.is_empty() {
        out.push_str(&format!(
            "  <text x=\"{}\" y=\"{}\" font-size=\"{}\" font-family=\"Segoe UI, Arial, sans-serif\" fill=\"{}\">{}</text>\n",
            format_number(left + 8.0),
            format_number(top + 18.0),
            format_number(annotation.font_size),
            annotation.color,
            svg_escape(&annotation.text)
        ));
    }
}

fn render_annotation_rect(
    out: &mut String,
    annotation: &AnnotationObject,
    width: f64,
    height: f64,
) {
    let (left, top, box_width, box_height) = annotation_rect_geometry(annotation, width, height);
    let dash = annotation
        .line_style
        .stroke_dasharray()
        .map(|dash| format!(" stroke-dasharray=\"{dash}\""))
        .unwrap_or_default();
    out.push_str(&format!(
        "  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\"{}/>\n",
        format_number(left),
        format_number(top),
        format_number(box_width),
        format_number(box_height),
        annotation.face_color.unwrap_or("none"),
        annotation.color,
        format_number(annotation.line_width),
        dash
    ));
}

fn render_annotation_ellipse(
    out: &mut String,
    annotation: &AnnotationObject,
    width: f64,
    height: f64,
) {
    let (left, top, box_width, box_height) = annotation_rect_geometry(annotation, width, height);
    let dash = annotation
        .line_style
        .stroke_dasharray()
        .map(|dash| format!(" stroke-dasharray=\"{dash}\""))
        .unwrap_or_default();
    out.push_str(&format!(
        "  <ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\"{}/>\n",
        format_number(left + box_width / 2.0),
        format_number(top + box_height / 2.0),
        format_number(box_width / 2.0),
        format_number(box_height / 2.0),
        annotation.face_color.unwrap_or("none"),
        annotation.color,
        format_number(annotation.line_width),
        dash
    ));
}

fn render_arrow_head(
    out: &mut String,
    from_x: f64,
    from_y: f64,
    to_x: f64,
    to_y: f64,
    color: &str,
    line_width: f64,
) {
    let dx = to_x - from_x;
    let dy = to_y - from_y;
    let length = (dx * dx + dy * dy).sqrt();
    if length <= f64::EPSILON {
        return;
    }
    let head_length = (length * 0.2).clamp(6.0, 12.0);
    let head_angle = 26.0_f64.to_radians();
    let unit_x = dx / length;
    let unit_y = dy / length;
    let back_x = -unit_x;
    let back_y = -unit_y;
    let cos_angle = head_angle.cos();
    let sin_angle = head_angle.sin();
    let left_x = to_x + head_length * (back_x * cos_angle - back_y * sin_angle);
    let left_y = to_y + head_length * (back_x * sin_angle + back_y * cos_angle);
    let right_x = to_x + head_length * (back_x * cos_angle + back_y * sin_angle);
    let right_y = to_y + head_length * (-back_x * sin_angle + back_y * cos_angle);
    render_svg_line(
        out,
        to_x,
        to_y,
        left_x,
        left_y,
        Some(color),
        line_width.max(1.2),
    );
    render_svg_line(
        out,
        to_x,
        to_y,
        right_x,
        right_y,
        Some(color),
        line_width.max(1.2),
    );
}

fn resolved_limits(axes: &AxesState) -> ((f64, f64), (f64, f64)) {
    let three_d_range = axes_three_d_range(axes);
    resolved_limits_with_three_d(axes, three_d_range.as_ref())
}

fn resolved_limits_for_side(axes: &AxesState, side: YAxisSide) -> ((f64, f64), (f64, f64)) {
    let three_d_range = axes_three_d_range(axes);
    let (data_x, data_y) = data_limits_for_side(axes, side, three_d_range.as_ref())
        .unwrap_or(((0.0, 1.0), (0.0, 1.0)));
    let y_limits = match side {
        YAxisSide::Left => axes
            .ylim
            .unwrap_or_else(|| padded_range_for_scale(data_y.0, data_y.1, axes.y_scale)),
        YAxisSide::Right => axes
            .ylim_right
            .unwrap_or_else(|| padded_range_for_scale(data_y.0, data_y.1, axes.y_scale_right)),
    };
    (
        axes.xlim
            .unwrap_or_else(|| padded_range_for_scale(data_x.0, data_x.1, axes.x_scale)),
        y_limits,
    )
}

fn resolved_y_limits_for_side(axes: &AxesState, side: YAxisSide) -> (f64, f64) {
    resolved_limits_for_side(axes, side).1
}

fn resolved_z_limits(axes: &AxesState) -> (f64, f64) {
    if let Some(zlim) = axes.zlim {
        return zlim;
    }

    axes_three_d_range(axes)
        .map(|range| padded_range(range.z_range.0, range.z_range.1))
        .unwrap_or_else(|| padded_range(0.0, 1.0))
}

fn default_ticks(lower: f64, upper: f64) -> Vec<f64> {
    if !lower.is_finite() || !upper.is_finite() {
        return Vec::new();
    }
    if (upper - lower).abs() <= f64::EPSILON {
        return vec![lower];
    }

    (0..=4)
        .map(|tick| {
            let fraction = tick as f64 / 4.0;
            lower + fraction * (upper - lower)
        })
        .collect()
}

fn default_log_ticks(lower: f64, upper: f64) -> Vec<f64> {
    let min = lower.min(upper);
    let max = lower.max(upper);
    if !min.is_finite() || !max.is_finite() || min <= 0.0 || max <= 0.0 {
        return Vec::new();
    }
    if (max - min).abs() <= f64::EPSILON {
        return vec![min];
    }

    let start = min.log10().floor() as i32;
    let end = max.log10().ceil() as i32;
    let span = (end - start).max(0);
    let step = ((span as f64) / 6.0).ceil().max(1.0) as i32;
    let mut ticks = Vec::new();
    let mut exponent = start;
    while exponent <= end {
        let tick = 10f64.powi(exponent);
        if tick_in_range(tick, min, max) {
            ticks.push(tick);
        }
        exponent += step;
    }
    if ticks.is_empty() {
        vec![min, max]
    } else {
        ticks
    }
}

fn default_ticks_for_scale(lower: f64, upper: f64, scale: AxisScale) -> Vec<f64> {
    match scale {
        AxisScale::Linear => default_ticks(lower, upper),
        AxisScale::Log => default_log_ticks(lower, upper),
    }
}

fn default_tick_label_for_scale(value: f64, scale: AxisScale) -> Option<String> {
    match scale {
        AxisScale::Linear => Some(format_number(value)),
        AxisScale::Log => {
            if !value.is_finite() || value <= 0.0 {
                None
            } else {
                let exponent = value.log10();
                if (exponent.round() - exponent).abs() <= 1e-9 {
                    let rounded = exponent.round() as i32;
                    if rounded.abs() >= 3 {
                        Some(format!("10^{rounded}"))
                    } else {
                        Some(format_number(value))
                    }
                } else {
                    Some(format_number(value))
                }
            }
        }
    }
}

fn resolved_ticks(axes: &AxesState, kind: TickKind) -> Vec<f64> {
    let ((x_lower, x_upper), (y_lower, y_upper)) = resolved_limits(axes);
    match kind {
        TickKind::X => axes
            .xticks
            .clone()
            .unwrap_or_else(|| default_ticks_for_scale(x_lower, x_upper, axes.x_scale)),
        TickKind::Y => axes
            .yticks
            .clone()
            .unwrap_or_else(|| default_ticks_for_scale(y_lower, y_upper, axes.y_scale)),
        TickKind::Z => axes.zticks.clone().unwrap_or_else(|| {
            let (lower, upper) = resolved_z_limits(axes);
            default_ticks(lower, upper)
        }),
    }
}

fn resolved_ticks_for_side(axes: &AxesState, kind: TickKind, side: YAxisSide) -> Vec<f64> {
    let ((x_lower, x_upper), (y_lower, y_upper)) = resolved_limits_for_side(axes, side);
    match kind {
        TickKind::X => axes
            .xticks
            .clone()
            .unwrap_or_else(|| default_ticks_for_scale(x_lower, x_upper, axes.x_scale)),
        TickKind::Y => match side {
            YAxisSide::Left => axes
                .yticks
                .clone()
                .unwrap_or_else(|| default_ticks_for_scale(y_lower, y_upper, axes.y_scale)),
            YAxisSide::Right => axes
                .yticks_right
                .clone()
                .unwrap_or_else(|| default_ticks_for_scale(y_lower, y_upper, axes.y_scale_right)),
        },
        TickKind::Z => axes.zticks.clone().unwrap_or_else(|| {
            let (lower, upper) = resolved_z_limits(axes);
            default_ticks(lower, upper)
        }),
    }
}

fn resolved_tick_labels(axes: &AxesState, kind: TickKind) -> Vec<String> {
    match kind {
        TickKind::X => axes.xtick_labels.clone().unwrap_or_else(|| {
            resolved_ticks(axes, kind)
                .into_iter()
                .filter_map(|tick| default_tick_label_for_scale(tick, axes.x_scale))
                .collect()
        }),
        TickKind::Y => axes.ytick_labels.clone().unwrap_or_else(|| {
            resolved_ticks(axes, kind)
                .into_iter()
                .filter_map(|tick| default_tick_label_for_scale(tick, axes.y_scale))
                .collect()
        }),
        TickKind::Z => axes.ztick_labels.clone().unwrap_or_else(|| {
            resolved_ticks(axes, kind)
                .into_iter()
                .map(format_number)
                .collect()
        }),
    }
}

fn resolved_tick_labels_for_side(axes: &AxesState, kind: TickKind, side: YAxisSide) -> Vec<String> {
    match kind {
        TickKind::X => axes.xtick_labels.clone().unwrap_or_else(|| {
            resolved_ticks_for_side(axes, kind, side)
                .into_iter()
                .filter_map(|tick| default_tick_label_for_scale(tick, axes.x_scale))
                .collect()
        }),
        TickKind::Y => match side {
            YAxisSide::Left => axes.ytick_labels.clone().unwrap_or_else(|| {
                resolved_ticks_for_side(axes, kind, side)
                    .into_iter()
                    .filter_map(|tick| default_tick_label_for_scale(tick, axes.y_scale))
                    .collect()
            }),
            YAxisSide::Right => axes.ytick_labels_right.clone().unwrap_or_else(|| {
                resolved_ticks_for_side(axes, kind, side)
                    .into_iter()
                    .filter_map(|tick| default_tick_label_for_scale(tick, axes.y_scale_right))
                    .collect()
            }),
        },
        TickKind::Z => axes.ztick_labels.clone().unwrap_or_else(|| {
            resolved_ticks_for_side(axes, kind, side)
                .into_iter()
                .map(format_number)
                .collect()
        }),
    }
}

fn resolved_tick_angle(axes: &AxesState, kind: TickKind) -> f64 {
    match kind {
        TickKind::X => axes.xtick_angle,
        TickKind::Y => axes.ytick_angle,
        TickKind::Z => axes.ztick_angle,
    }
}

fn resolved_tick_angle_for_side(axes: &AxesState, kind: TickKind, side: YAxisSide) -> f64 {
    match kind {
        TickKind::X => axes.xtick_angle,
        TickKind::Y => match side {
            YAxisSide::Left => axes.ytick_angle,
            YAxisSide::Right => axes.ytick_angle_right,
        },
        TickKind::Z => axes.ztick_angle,
    }
}

fn resolved_ticks_active_side(axes: &AxesState, kind: TickKind) -> Vec<f64> {
    let side = if kind == TickKind::Y {
        axes.active_y_axis
    } else {
        YAxisSide::Left
    };
    resolved_ticks_for_side(axes, kind, side)
}

fn resolved_tick_labels_active_side(axes: &AxesState, kind: TickKind) -> Vec<String> {
    let side = if kind == TickKind::Y {
        axes.active_y_axis
    } else {
        YAxisSide::Left
    };
    resolved_tick_labels_for_side(axes, kind, side)
}

fn resolved_tick_angle_active_side(axes: &AxesState, kind: TickKind) -> f64 {
    let side = if kind == TickKind::Y {
        axes.active_y_axis
    } else {
        YAxisSide::Left
    };
    resolved_tick_angle_for_side(axes, kind, side)
}

fn sync_tick_label_override(axes: &mut AxesState, kind: TickKind, tick_count: usize) {
    let labels = match kind {
        TickKind::X => &mut axes.xtick_labels,
        TickKind::Y => match axes.active_y_axis {
            YAxisSide::Left => &mut axes.ytick_labels,
            YAxisSide::Right => &mut axes.ytick_labels_right,
        },
        TickKind::Z => &mut axes.ztick_labels,
    };

    if labels
        .as_ref()
        .is_some_and(|labels| !labels.is_empty() && labels.len() != tick_count)
    {
        *labels = None;
    }
}

fn tick_in_range(value: f64, lower: f64, upper: f64) -> bool {
    let min = lower.min(upper) - 1e-9;
    let max = lower.max(upper) + 1e-9;
    value >= min && value <= max
}

fn resolved_limits_with_three_d(
    axes: &AxesState,
    three_d_range: Option<&ThreeDRange>,
) -> ((f64, f64), (f64, f64)) {
    let (data_x, data_y) = data_limits(axes, three_d_range).unwrap_or(((0.0, 1.0), (0.0, 1.0)));
    (
        axes.xlim
            .unwrap_or_else(|| padded_range(data_x.0, data_x.1)),
        axes.ylim
            .unwrap_or_else(|| padded_range(data_y.0, data_y.1)),
    )
}

fn data_limits(
    axes: &AxesState,
    three_d_range: Option<&ThreeDRange>,
) -> Option<((f64, f64), (f64, f64))> {
    let mut x_min = f64::INFINITY;
    let mut x_max = f64::NEG_INFINITY;
    let mut y_min = f64::INFINITY;
    let mut y_max = f64::NEG_INFINITY;
    let mut saw_reference_line = false;

    for series in axes.series.iter().filter(|series| series.visible) {
        if let Some(reference_line) = &series.reference_line {
            saw_reference_line = true;
            match reference_line.orientation {
                ReferenceLineOrientation::Vertical => {
                    x_min = x_min.min(reference_line.value);
                    x_max = x_max.max(reference_line.value);
                }
                ReferenceLineOrientation::Horizontal => {
                    y_min = y_min.min(reference_line.value);
                    y_max = y_max.max(reference_line.value);
                }
            }
            continue;
        }
        let ((sx_min, sx_max), (sy_min, sy_max)) = series_data_limits(series, axes, three_d_range);
        x_min = x_min.min(sx_min);
        x_max = x_max.max(sx_max);
        y_min = y_min.min(sy_min);
        y_max = y_max.max(sy_max);
    }

    if saw_reference_line {
        if !x_min.is_finite() || !x_max.is_finite() {
            x_min = 0.0;
            x_max = 1.0;
        }
        if !y_min.is_finite() || !y_max.is_finite() {
            y_min = 0.0;
            y_max = 1.0;
        }
    }

    if x_min.is_finite() && x_max.is_finite() && y_min.is_finite() && y_max.is_finite() {
        Some(((x_min, x_max), (y_min, y_max)))
    } else {
        None
    }
}

fn data_limits_for_side(
    axes: &AxesState,
    side: YAxisSide,
    three_d_range: Option<&ThreeDRange>,
) -> Option<((f64, f64), (f64, f64))> {
    let mut x_min = f64::INFINITY;
    let mut x_max = f64::NEG_INFINITY;
    let mut y_min = f64::INFINITY;
    let mut y_max = f64::NEG_INFINITY;
    let mut saw_reference_line = false;

    for series in axes.series.iter().filter(|series| series.visible) {
        if let Some(reference_line) = &series.reference_line {
            saw_reference_line = true;
            match reference_line.orientation {
                ReferenceLineOrientation::Vertical => {
                    x_min = x_min.min(reference_line.value);
                    x_max = x_max.max(reference_line.value);
                }
                ReferenceLineOrientation::Horizontal => {
                    if series.y_axis_side == side {
                        y_min = y_min.min(reference_line.value);
                        y_max = y_max.max(reference_line.value);
                    }
                }
            }
            continue;
        }
        if series_uses_secondary_y_axis(series) && series.y_axis_side != side {
            continue;
        }
        let ((sx_min, sx_max), (sy_min, sy_max)) = series_data_limits(series, axes, three_d_range);
        x_min = x_min.min(sx_min);
        x_max = x_max.max(sx_max);
        y_min = y_min.min(sy_min);
        y_max = y_max.max(sy_max);
    }

    if saw_reference_line {
        if !x_min.is_finite() || !x_max.is_finite() {
            x_min = 0.0;
            x_max = 1.0;
        }
        if !y_min.is_finite() || !y_max.is_finite() {
            y_min = 0.0;
            y_max = 1.0;
        }
    }

    if x_min.is_finite() && x_max.is_finite() && y_min.is_finite() && y_max.is_finite() {
        Some(((x_min, x_max), (y_min, y_max)))
    } else {
        None
    }
}

fn current_y_ticks_mut<'a>(axes: &'a mut AxesState, side: YAxisSide) -> &'a mut Option<Vec<f64>> {
    match side {
        YAxisSide::Left => &mut axes.yticks,
        YAxisSide::Right => &mut axes.yticks_right,
    }
}

fn current_y_tick_labels_mut<'a>(
    axes: &'a mut AxesState,
    side: YAxisSide,
) -> &'a mut Option<Vec<String>> {
    match side {
        YAxisSide::Left => &mut axes.ytick_labels,
        YAxisSide::Right => &mut axes.ytick_labels_right,
    }
}

fn current_y_tick_angle_mut<'a>(axes: &'a mut AxesState, side: YAxisSide) -> &'a mut f64 {
    match side {
        YAxisSide::Left => &mut axes.ytick_angle,
        YAxisSide::Right => &mut axes.ytick_angle_right,
    }
}

fn current_y_limit_mut<'a>(axes: &'a mut AxesState, side: YAxisSide) -> &'a mut Option<(f64, f64)> {
    match side {
        YAxisSide::Left => &mut axes.ylim,
        YAxisSide::Right => &mut axes.ylim_right,
    }
}

fn current_y_scale_mut<'a>(axes: &'a mut AxesState, side: YAxisSide) -> &'a mut AxisScale {
    match side {
        YAxisSide::Left => &mut axes.y_scale,
        YAxisSide::Right => &mut axes.y_scale_right,
    }
}

fn current_y_scale_for_side(axes: &AxesState, side: YAxisSide) -> AxisScale {
    match side {
        YAxisSide::Left => axes.y_scale,
        YAxisSide::Right => axes.y_scale_right,
    }
}

fn series_uses_secondary_y_axis(series: &PlotSeries) -> bool {
    matches!(
        series.kind,
        SeriesKind::Line
            | SeriesKind::ErrorBar
            | SeriesKind::Scatter
            | SeriesKind::Quiver
            | SeriesKind::Area
            | SeriesKind::Stairs
            | SeriesKind::Bar
            | SeriesKind::Stem
            | SeriesKind::Histogram
    ) || matches!(
        series.reference_line.as_ref().map(|line| line.orientation),
        Some(ReferenceLineOrientation::Horizontal)
    )
}

fn axes_has_right_y_axis(axes: &AxesState) -> bool {
    axes.series.iter().any(|series| {
        series.visible
            && series_uses_secondary_y_axis(series)
            && series.y_axis_side == YAxisSide::Right
    }) || !axes.ylabel_right.is_empty()
        || axes.ylim_right.is_some()
        || axes.yticks_right.is_some()
        || axes.ytick_labels_right.is_some()
}

fn parse_image_mapping(value: &Value) -> Result<ImageMapping, RuntimeError> {
    match text_arg(value, "set")?.to_ascii_lowercase().as_str() {
        "scaled" => Ok(ImageMapping::Scaled),
        "direct" => Ok(ImageMapping::Direct),
        other => Err(RuntimeError::Unsupported(format!(
            "image `CDataMapping` currently supports only `scaled` or `direct`, found `{other}`"
        ))),
    }
}

fn parse_alpha_data_mapping(value: &Value) -> Result<AlphaDataMapping, RuntimeError> {
    match text_arg(value, "set")?.to_ascii_lowercase().as_str() {
        "none" => Ok(AlphaDataMapping::None),
        other => Err(RuntimeError::Unsupported(format!(
            "image `AlphaDataMapping` currently supports only `none`, found `{other}`"
        ))),
    }
}

fn parse_image_alpha_data(
    value: &Value,
    rows: usize,
    cols: usize,
) -> Result<ImageAlphaData, RuntimeError> {
    let (alpha_rows, alpha_cols, alpha_values) = numeric_matrix(value, "set")?;
    if alpha_rows == 1 && alpha_cols == 1 {
        return Ok(ImageAlphaData::Scalar(alpha_values[0]));
    }
    if alpha_rows != rows || alpha_cols != cols {
        return Err(RuntimeError::ShapeError(format!(
            "set currently expects image `AlphaData` to be a scalar or a {}x{} numeric matrix, found {}x{}",
            rows, cols, alpha_rows, alpha_cols
        )));
    }
    Ok(ImageAlphaData::Matrix(alpha_values))
}

fn resolved_image_alpha(image: &ImageSeriesData, index: usize) -> f64 {
    let raw = match &image.alpha_data {
        ImageAlphaData::Scalar(alpha) => *alpha,
        ImageAlphaData::Matrix(values) => values.get(index).copied().unwrap_or(1.0),
    };
    match image.alpha_mapping {
        AlphaDataMapping::None => raw.clamp(0.0, 1.0),
    }
}

fn svg_fill_opacity_attribute(opacity: f64) -> String {
    if (opacity - 1.0).abs() <= f64::EPSILON {
        String::new()
    } else {
        format!(" fill-opacity=\"{}\"", format_number(opacity))
    }
}

fn series_data_limits(
    series: &PlotSeries,
    axes: &AxesState,
    three_d_range: Option<&ThreeDRange>,
) -> ((f64, f64), (f64, f64)) {
    if series.pie.is_some() {
        return ((-1.35, 1.35), (-1.35, 1.35));
    }
    if let Some(quiver) = &series.quiver {
        let mut x_min = f64::INFINITY;
        let mut x_max = f64::NEG_INFINITY;
        let mut y_min = f64::INFINITY;
        let mut y_max = f64::NEG_INFINITY;
        for (x, y) in quiver.bases.iter().chain(quiver.tips.iter()) {
            x_min = x_min.min(*x);
            x_max = x_max.max(*x);
            y_min = y_min.min(*y);
            y_max = y_max.max(*y);
        }
        return ((x_min, x_max), (y_min, y_max));
    }
    if let Some(error_bar) = &series.error_bar {
        let mut x_min = f64::INFINITY;
        let mut x_max = f64::NEG_INFINITY;
        let mut y_min = f64::INFINITY;
        let mut y_max = f64::NEG_INFINITY;
        for (index, (x, y)) in series.x.iter().zip(&series.y).enumerate() {
            x_min = x_min.min(*x);
            x_max = x_max.max(*x);
            y_min = y_min.min(*y);
            y_max = y_max.max(*y);
            if let Some(lower) = &error_bar.vertical_lower {
                y_min = y_min.min(*y - lower[index]);
            }
            if let Some(upper) = &error_bar.vertical_upper {
                y_max = y_max.max(*y + upper[index]);
            }
            if let Some(left) = &error_bar.horizontal_lower {
                x_min = x_min.min(*x - left[index]);
            }
            if let Some(right) = &error_bar.horizontal_upper {
                x_max = x_max.max(*x + right[index]);
            }
        }
        return ((x_min, x_max), (y_min, y_max));
    }
    if let Some(histogram) = &series.histogram {
        let y_max = histogram.counts.iter().copied().fold(0.0, f64::max);
        return (
            (
                histogram.edges[0],
                histogram.edges[histogram.edges.len() - 1],
            ),
            (0.0, y_max),
        );
    }
    if let Some(histogram2) = &series.histogram2 {
        return (
            (
                histogram2.x_edges[0],
                histogram2.x_edges[histogram2.x_edges.len() - 1],
            ),
            (
                histogram2.y_edges[0],
                histogram2.y_edges[histogram2.y_edges.len() - 1],
            ),
        );
    }
    if let Some(image) = &series.image {
        return image_display_limits(image);
    }
    if let Some(text) = &series.text {
        return ((text.x, text.x), (text.y, text.y));
    }
    if let Some(rectangle) = &series.rectangle {
        return (
            (
                rectangle.x.min(rectangle.x + rectangle.width),
                rectangle.x.max(rectangle.x + rectangle.width),
            ),
            (
                rectangle.y.min(rectangle.y + rectangle.height),
                rectangle.y.max(rectangle.y + rectangle.height),
            ),
        );
    }
    if let Some(contour) = &series.contour {
        return (contour.x_domain, contour.y_domain);
    }
    if let Some(contour_fill) = &series.contour_fill {
        return (contour_fill.x_domain, contour_fill.y_domain);
    }
    if let Some(surface) = &series.surface {
        if let Some(range) = three_d_range {
            return surface_projected_limits(surface, range, axes);
        }
        return (surface.x_range, surface.y_range);
    }
    if let Some(three_d) = &series.three_d {
        if let Some(range) = three_d_range {
            return three_d_series_projected_limits(three_d, range, axes);
        }
        return (three_d.x_range, three_d.y_range);
    }

    let mut x_min = f64::INFINITY;
    let mut x_max = f64::NEG_INFINITY;
    let mut y_min = f64::INFINITY;
    let mut y_max = f64::NEG_INFINITY;

    for value in &series.x {
        x_min = x_min.min(*value);
        x_max = x_max.max(*value);
    }
    for value in &series.y {
        y_min = y_min.min(*value);
        y_max = y_max.max(*value);
    }

    match series.kind {
        SeriesKind::Line
        | SeriesKind::ReferenceLine
        | SeriesKind::ErrorBar
        | SeriesKind::Scatter
        | SeriesKind::Stairs
        | SeriesKind::Quiver => {}
        SeriesKind::Pie | SeriesKind::Pie3 => {}
        SeriesKind::Histogram | SeriesKind::Histogram2 => {}
        SeriesKind::Area => {
            y_min = y_min.min(0.0);
            y_max = y_max.max(0.0);
        }
        SeriesKind::Line3D | SeriesKind::Scatter3D | SeriesKind::Quiver3D | SeriesKind::Stem3D => {}
        SeriesKind::Stem => {
            y_min = y_min.min(0.0);
            y_max = y_max.max(0.0);
        }
        SeriesKind::Bar => {
            let width = bar_width(&series.x);
            x_min -= width / 2.0;
            x_max += width / 2.0;
            y_min = y_min.min(0.0);
            y_max = y_max.max(0.0);
        }
        SeriesKind::BarHorizontal => {
            let height = bar_width(&series.y);
            y_min -= height / 2.0;
            y_max += height / 2.0;
            x_min = x_min.min(0.0);
            x_max = x_max.max(0.0);
        }
        SeriesKind::Contour
        | SeriesKind::Contour3
        | SeriesKind::ContourFill
        | SeriesKind::Waterfall
        | SeriesKind::Ribbon
        | SeriesKind::Mesh
        | SeriesKind::Surface
        | SeriesKind::Image
        | SeriesKind::Text
        | SeriesKind::Rectangle
        | SeriesKind::Patch => {}
    }

    ((x_min, x_max), (y_min, y_max))
}

fn padded_range(min: f64, max: f64) -> (f64, f64) {
    if !min.is_finite() || !max.is_finite() {
        return (0.0, 1.0);
    }
    if (max - min).abs() > f64::EPSILON {
        return (min, max);
    }

    let padding = if min.abs() > 1.0 {
        min.abs() * 0.1
    } else {
        1.0
    };
    (min - padding, max + padding)
}

fn padded_range_for_scale(min: f64, max: f64, scale: AxisScale) -> (f64, f64) {
    match scale {
        AxisScale::Linear => padded_range(min, max),
        AxisScale::Log => {
            if !min.is_finite() || !max.is_finite() || min <= 0.0 || max <= 0.0 {
                return (0.1, 10.0);
            }
            if (max - min).abs() > f64::EPSILON {
                (min, max)
            } else {
                (min / 10.0, max * 10.0)
            }
        }
    }
}

fn scale_x(value: f64, min: f64, max: f64, frame: AxesFrame) -> f64 {
    let Some(value) = frame.x_scale.transform(value) else {
        return frame.left;
    };
    let Some(min) = frame.x_scale.transform(min) else {
        return frame.left;
    };
    let Some(max) = frame.x_scale.transform(max) else {
        return frame.right();
    };
    frame.left + ((value - min) / (max - min)) * frame.width
}

fn scale_y(value: f64, min: f64, max: f64, frame: AxesFrame) -> f64 {
    let Some(value) = frame.y_scale.transform(value) else {
        return frame.bottom();
    };
    let Some(min) = frame.y_scale.transform(min) else {
        return frame.bottom();
    };
    let Some(max) = frame.y_scale.transform(max) else {
        return frame.top;
    };
    frame.top + frame.height - ((value - min) / (max - min)) * frame.height
}

fn adjusted_plot_frame(
    axes: &AxesState,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
) -> AxesFrame {
    if axes.aspect_mode == AxisAspectMode::Square {
        let side = frame.width.min(frame.height).max(1.0);
        let left = frame.left + (frame.width - side) / 2.0;
        let top = frame.top + (frame.height - side) / 2.0;
        return AxesFrame {
            left,
            top,
            width: side,
            height: side,
            x_scale: frame.x_scale,
            y_scale: frame.y_scale,
        };
    }

    if axes.aspect_mode != AxisAspectMode::Equal {
        return frame;
    }

    let x_range = (x_max - x_min).abs();
    let y_range = (y_max - y_min).abs();
    if x_range <= f64::EPSILON
        || y_range <= f64::EPSILON
        || frame.width <= f64::EPSILON
        || frame.height <= f64::EPSILON
    {
        return frame;
    }

    let target_aspect = x_range / y_range;
    let frame_aspect = frame.width / frame.height;
    if !target_aspect.is_finite() || !frame_aspect.is_finite() {
        return frame;
    }

    if target_aspect > frame_aspect {
        let height = (frame.width / target_aspect).max(1.0);
        let top = frame.top + (frame.height - height) / 2.0;
        AxesFrame {
            top,
            height,
            ..frame
        }
    } else {
        let width = (frame.height * target_aspect).max(1.0);
        let left = frame.left + (frame.width - width) / 2.0;
        AxesFrame {
            left,
            width,
            ..frame
        }
    }
}

fn axes_frame_from_position(
    position: [f64; 4],
    figure_width: f64,
    figure_height: f64,
) -> AxesFrame {
    let left = position[0].clamp(0.0, 1.0) * figure_width;
    let width = position[2].clamp(0.0, 1.0) * figure_width;
    let height = position[3].clamp(0.0, 1.0) * figure_height;
    let bottom = position[1].clamp(0.0, 1.0) * figure_height;
    let top = figure_height - bottom - height;
    AxesFrame {
        left,
        top,
        width: width.max(1.0),
        height: height.max(1.0),
        x_scale: AxisScale::Linear,
        y_scale: AxisScale::Linear,
    }
}

#[derive(Debug, Clone, Copy)]
struct ContourPoint {
    x: f64,
    y: f64,
    z: f64,
}

fn contour_triangle_segment(
    first: ContourPoint,
    second: ContourPoint,
    third: ContourPoint,
    level: f64,
) -> Option<ContourSegment> {
    let mut points = Vec::new();
    push_contour_intersection(&mut points, first, second, level);
    push_contour_intersection(&mut points, second, third, level);
    push_contour_intersection(&mut points, third, first, level);
    if points.len() == 2 {
        Some(ContourSegment {
            start: points[0],
            end: points[1],
            level,
        })
    } else {
        None
    }
}

fn push_contour_intersection(
    points: &mut Vec<(f64, f64)>,
    start: ContourPoint,
    end: ContourPoint,
    level: f64,
) {
    let Some(point) = contour_edge_intersection(start, end, level) else {
        return;
    };
    if points
        .iter()
        .any(|existing| contour_points_close(*existing, point))
    {
        return;
    }
    points.push(point);
}

fn contour_edge_intersection(
    start: ContourPoint,
    end: ContourPoint,
    level: f64,
) -> Option<(f64, f64)> {
    let start_delta = start.z - level;
    let end_delta = end.z - level;
    if (start_delta < 0.0 && end_delta < 0.0) || (start_delta > 0.0 && end_delta > 0.0) {
        return None;
    }
    if start_delta.abs() <= 1e-9 && end_delta.abs() <= 1e-9 {
        return None;
    }

    let denominator = end.z - start.z;
    let fraction = if denominator.abs() <= 1e-12 {
        0.5
    } else {
        (level - start.z) / denominator
    };
    if !(-1e-9..=1.0 + 1e-9).contains(&fraction) {
        return None;
    }
    let clamped = fraction.clamp(0.0, 1.0);
    Some((
        start.x + clamped * (end.x - start.x),
        start.y + clamped * (end.y - start.y),
    ))
}

fn contour_points_close(left: (f64, f64), right: (f64, f64)) -> bool {
    (left.0 - right.0).abs() <= 1e-9 && (left.1 - right.1).abs() <= 1e-9
}

#[derive(Debug, Clone, Copy)]
enum SurfaceAxis {
    X,
    Y,
}

fn axes_three_d_range(axes: &AxesState) -> Option<ThreeDRange> {
    let mut x_min = f64::INFINITY;
    let mut x_max = f64::NEG_INFINITY;
    let mut y_min = f64::INFINITY;
    let mut y_max = f64::NEG_INFINITY;
    let mut z_min = f64::INFINITY;
    let mut z_max = f64::NEG_INFINITY;

    for series in axes.series.iter().filter(|series| series.visible) {
        if matches!(series.kind, SeriesKind::Pie3) && series.pie.is_some() {
            x_min = x_min.min(-1.35);
            x_max = x_max.max(1.35);
            y_min = y_min.min(-1.35);
            y_max = y_max.max(1.35);
            z_min = z_min.min(0.0);
            z_max = z_max.max(PIE3_HEIGHT);
        }
        if let Some(surface) = &series.surface {
            x_min = x_min.min(surface.x_range.0);
            x_max = x_max.max(surface.x_range.1);
            y_min = y_min.min(surface.y_range.0);
            y_max = y_max.max(surface.y_range.1);
            z_min = z_min.min(surface.z_range.0);
            z_max = z_max.max(surface.z_range.1);
        }
        if let Some(three_d) = &series.three_d {
            x_min = x_min.min(three_d.x_range.0);
            x_max = x_max.max(three_d.x_range.1);
            y_min = y_min.min(three_d.y_range.0);
            y_max = y_max.max(three_d.y_range.1);
            z_min = z_min.min(three_d.z_range.0);
            z_max = z_max.max(three_d.z_range.1);
            if series.kind == SeriesKind::Stem3D {
                z_min = z_min.min(0.0);
                z_max = z_max.max(0.0);
            }
        }
        if series.kind == SeriesKind::Contour3 {
            if let Some(contour) = &series.contour {
                x_min = x_min.min(contour.x_domain.0);
                x_max = x_max.max(contour.x_domain.1);
                y_min = y_min.min(contour.y_domain.0);
                y_max = y_max.max(contour.y_domain.1);
                z_min = z_min.min(contour.level_range.0);
                z_max = z_max.max(contour.level_range.1);
            }
        }
    }

    if x_min.is_finite()
        && x_max.is_finite()
        && y_min.is_finite()
        && y_max.is_finite()
        && z_min.is_finite()
        && z_max.is_finite()
    {
        let z_range = axes.zlim.unwrap_or((z_min, z_max));
        Some(ThreeDRange {
            x_range: (x_min, x_max),
            y_range: (y_min, y_max),
            z_range,
        })
    } else {
        None
    }
}

fn project_3d_point(
    x: f64,
    y: f64,
    z: f64,
    range: &ThreeDRange,
    axes: &AxesState,
) -> (f64, f64, f64) {
    let azimuth = axes.view_azimuth.to_radians();
    let elevation = (axes.view_elevation - 90.0).to_radians();
    let z_mid = (range.z_range.0 + range.z_range.1) / 2.0;
    let x_span = (range.x_range.1 - range.x_range.0).abs().max(1.0);
    let y_span = (range.y_range.1 - range.y_range.0).abs().max(1.0);
    let z_span = (range.z_range.1 - range.z_range.0).abs().max(1.0);
    let z_scale = 0.8 * ((x_span + y_span) / 2.0) / z_span;
    let centered_z = (z - z_mid) * z_scale;

    let x1 = x * azimuth.cos() + y * azimuth.sin();
    let y1 = -x * azimuth.sin() + y * azimuth.cos();
    let projected_y = y1 * elevation.cos() - centered_z * elevation.sin();
    let depth = y1 * elevation.sin() + centered_z * elevation.cos();
    (x1, projected_y, depth)
}

fn project_surface_patch(
    patch: &SurfacePatch,
    range: &ThreeDRange,
    axes: &AxesState,
) -> ([(f64, f64); 4], f64) {
    let mut points = [(0.0, 0.0); 4];
    let mut depth = 0.0;
    for (index, (x, y, z)) in patch.points.iter().enumerate() {
        let (projected_x, projected_y, projected_depth) = project_3d_point(*x, *y, *z, range, axes);
        points[index] = (projected_x, projected_y);
        depth += projected_depth;
    }
    (points, depth / patch.points.len() as f64)
}

fn project_polygon3d(
    points: &[(f64, f64, f64)],
    range: &ThreeDRange,
    axes: &AxesState,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
) -> (Vec<(f64, f64)>, f64) {
    let mut projected_points = Vec::with_capacity(points.len());
    let mut depth = 0.0;
    for (x, y, z) in points {
        let (projected_x, projected_y, projected_depth) = project_3d_point(*x, *y, *z, range, axes);
        projected_points.push((
            scale_x(projected_x, x_min, x_max, frame),
            scale_y(projected_y, y_min, y_max, frame),
        ));
        depth += projected_depth;
    }
    (
        projected_points,
        if points.is_empty() {
            0.0
        } else {
            depth / points.len() as f64
        },
    )
}

fn surface_projected_limits(
    surface: &SurfaceSeriesData,
    range: &ThreeDRange,
    axes: &AxesState,
) -> ((f64, f64), (f64, f64)) {
    let mut x_values = Vec::new();
    let mut y_values = Vec::new();
    for patch in &surface.patches {
        let (projected_points, _) = project_surface_patch(patch, range, axes);
        for (x, y) in projected_points {
            x_values.push(x);
            y_values.push(y);
        }
    }
    (finite_min_max(&x_values), finite_min_max(&y_values))
}

fn three_d_series_projected_limits(
    three_d: &ThreeDSeriesData,
    range: &ThreeDRange,
    axes: &AxesState,
) -> ((f64, f64), (f64, f64)) {
    let mut x_values = Vec::with_capacity(three_d.points.len());
    let mut y_values = Vec::with_capacity(three_d.points.len());
    for (x, y, z) in &three_d.points {
        let (projected_x, projected_y, _) = project_3d_point(*x, *y, *z, range, axes);
        x_values.push(projected_x);
        y_values.push(projected_y);
    }
    (finite_min_max(&x_values), finite_min_max(&y_values))
}

fn bar_width(x: &[f64]) -> f64 {
    if x.len() <= 1 {
        return 0.8;
    }

    let min_delta = x
        .windows(2)
        .map(|pair| (pair[1] - pair[0]).abs())
        .filter(|delta| *delta > f64::EPSILON)
        .fold(f64::INFINITY, f64::min);
    if min_delta.is_finite() {
        0.8 * min_delta
    } else {
        0.8
    }
}

fn render_series_svg(
    out: &mut String,
    series: &PlotSeries,
    axes: &AxesState,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
    colormap: ColormapKind,
    caxis_override: Option<(f64, f64)>,
    three_d_range: Option<&ThreeDRange>,
) {
    match series.kind {
        SeriesKind::Line => render_line_series(out, series, frame, x_min, x_max, y_min, y_max),
        SeriesKind::ReferenceLine => {
            render_reference_line_series(out, series, axes, frame, x_min, x_max, y_min, y_max)
        }
        SeriesKind::Line3D => render_line3d_series(
            out,
            series,
            axes,
            frame,
            x_min,
            x_max,
            y_min,
            y_max,
            three_d_range,
        ),
        SeriesKind::ErrorBar => {
            render_errorbar_series(out, series, frame, x_min, x_max, y_min, y_max)
        }
        SeriesKind::Scatter => render_scatter_series(
            out,
            series,
            frame,
            x_min,
            x_max,
            y_min,
            y_max,
            colormap,
            caxis_override,
        ),
        SeriesKind::Scatter3D => render_scatter3d_series(
            out,
            series,
            axes,
            frame,
            x_min,
            x_max,
            y_min,
            y_max,
            colormap,
            caxis_override,
            three_d_range,
        ),
        SeriesKind::Quiver => render_quiver_series(out, series, frame, x_min, x_max, y_min, y_max),
        SeriesKind::Quiver3D => render_quiver3d_series(
            out,
            series,
            axes,
            frame,
            x_min,
            x_max,
            y_min,
            y_max,
            three_d_range,
        ),
        SeriesKind::Pie => render_pie_series(out, series, frame, x_min, x_max, y_min, y_max),
        SeriesKind::Pie3 => render_pie3_series(
            out,
            series,
            axes,
            frame,
            x_min,
            x_max,
            y_min,
            y_max,
            three_d_range,
        ),
        SeriesKind::Histogram => {
            render_histogram_series(out, series, frame, x_min, x_max, y_min, y_max)
        }
        SeriesKind::Histogram2 => render_histogram2_series(
            out,
            series,
            frame,
            x_min,
            x_max,
            y_min,
            y_max,
            colormap,
            caxis_override,
        ),
        SeriesKind::Area => render_area_series(out, series, frame, x_min, x_max, y_min, y_max),
        SeriesKind::Stairs => render_stairs_series(out, series, frame, x_min, x_max, y_min, y_max),
        SeriesKind::Bar => render_bar_series(out, series, frame, x_min, x_max, y_min, y_max),
        SeriesKind::BarHorizontal => {
            render_barh_series(out, series, frame, x_min, x_max, y_min, y_max)
        }
        SeriesKind::Stem => render_stem_series(out, series, frame, x_min, x_max, y_min, y_max),
        SeriesKind::Stem3D => render_stem3d_series(
            out,
            series,
            axes,
            frame,
            x_min,
            x_max,
            y_min,
            y_max,
            three_d_range,
        ),
        SeriesKind::Contour => {
            render_contour_series(out, series, frame, x_min, x_max, y_min, y_max)
        }
        SeriesKind::Contour3 => render_contour3_series(
            out,
            series,
            axes,
            frame,
            x_min,
            x_max,
            y_min,
            y_max,
            three_d_range,
        ),
        SeriesKind::ContourFill => render_contour_fill_series(
            out,
            series,
            frame,
            x_min,
            x_max,
            y_min,
            y_max,
            colormap,
            caxis_override,
        ),
        SeriesKind::Waterfall => render_waterfall_series(
            out,
            series,
            axes,
            frame,
            x_min,
            x_max,
            y_min,
            y_max,
            three_d_range,
        ),
        SeriesKind::Ribbon => render_surface_series(
            out,
            series,
            axes,
            frame,
            x_min,
            x_max,
            y_min,
            y_max,
            colormap,
            caxis_override,
            three_d_range,
        ),
        SeriesKind::Mesh => render_mesh_series(
            out,
            series,
            axes,
            frame,
            x_min,
            x_max,
            y_min,
            y_max,
            colormap,
            caxis_override,
            three_d_range,
        ),
        SeriesKind::Surface => render_surface_series(
            out,
            series,
            axes,
            frame,
            x_min,
            x_max,
            y_min,
            y_max,
            colormap,
            caxis_override,
            three_d_range,
        ),
        SeriesKind::Image => render_image_series(
            out,
            series,
            frame,
            x_min,
            x_max,
            y_min,
            y_max,
            colormap,
            caxis_override,
        ),
        SeriesKind::Text => render_text_series(out, series, frame, x_min, x_max, y_min, y_max),
        SeriesKind::Rectangle => {
            render_rectangle_series(out, series, frame, x_min, x_max, y_min, y_max)
        }
        SeriesKind::Patch => render_patch_series(
            out,
            series,
            axes,
            frame,
            x_min,
            x_max,
            y_min,
            y_max,
            three_d_range,
        ),
    }
}

fn render_line3d_series(
    out: &mut String,
    series: &PlotSeries,
    axes: &AxesState,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
    three_d_range: Option<&ThreeDRange>,
) {
    let (Some(three_d), Some(range)) = (&series.three_d, three_d_range) else {
        return;
    };

    let projected = three_d
        .points
        .iter()
        .map(|(x, y, z)| {
            let (projected_x, projected_y, _) = project_3d_point(*x, *y, *z, range, axes);
            ViewerPoint {
                screen_x: scale_x(projected_x, x_min, x_max, frame),
                screen_y: scale_y(projected_y, y_min, y_max, frame),
                data_x: *x,
                data_y: *y,
                data_z: Some(*z),
            }
        })
        .collect::<Vec<_>>();
    render_styled_line_path(out, series, &projected);
}

fn render_scatter3d_series(
    out: &mut String,
    series: &PlotSeries,
    axes: &AxesState,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
    colormap: ColormapKind,
    caxis_override: Option<(f64, f64)>,
    three_d_range: Option<&ThreeDRange>,
) {
    let (Some(three_d), Some(range)) = (&series.three_d, three_d_range) else {
        return;
    };

    for (index, (x_value, y_value, z_value)) in three_d.points.iter().enumerate() {
        let (projected_x, projected_y, _) =
            project_3d_point(*x_value, *y_value, *z_value, range, axes);
        let point = ViewerPoint {
            screen_x: scale_x(projected_x, x_min, x_max, frame),
            screen_y: scale_y(projected_y, y_min, y_max, frame),
            data_x: *x_value,
            data_y: *y_value,
            data_z: Some(*z_value),
        };
        render_scatter_marker_svg(out, series, index, point, colormap, caxis_override);
    }
}

fn render_quiver3d_series(
    out: &mut String,
    series: &PlotSeries,
    axes: &AxesState,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
    three_d_range: Option<&ThreeDRange>,
) {
    let (Some(three_d), Some(range)) = (&series.three_d, three_d_range) else {
        return;
    };

    for points in three_d.points.chunks_exact(2) {
        let (base_x, base_y, _) =
            project_3d_point(points[0].0, points[0].1, points[0].2, range, axes);
        let (tip_x, tip_y, _) =
            project_3d_point(points[1].0, points[1].1, points[1].2, range, axes);
        let x1 = scale_x(base_x, x_min, x_max, frame);
        let y1 = scale_y(base_y, y_min, y_max, frame);
        let x2 = scale_x(tip_x, x_min, x_max, frame);
        let y2 = scale_y(tip_y, y_min, y_max, frame);
        let metadata = format!(
            "{} data-matc-color=\"{}\"",
            viewer_three_d_attributes(points, &[(x1, y1), (x2, y2)], "matc-3d-arrow"),
            series.color
        );
        render_quiver_arrow_with_attributes(
            out,
            series.color,
            x1,
            y1,
            x2,
            y2,
            Some(metadata.as_str()),
        );
    }
}

fn render_line_series(
    out: &mut String,
    series: &PlotSeries,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
) {
    let points = series
        .x
        .iter()
        .zip(&series.y)
        .map(|(x, y)| ViewerPoint {
            screen_x: scale_x(*x, x_min, x_max, frame),
            screen_y: scale_y(*y, y_min, y_max, frame),
            data_x: *x,
            data_y: *y,
            data_z: None,
        })
        .collect::<Vec<_>>();
    render_styled_line_path(out, series, &points);
}

fn render_reference_line_series(
    out: &mut String,
    series: &PlotSeries,
    axes: &AxesState,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
) {
    let Some(reference_line) = &series.reference_line else {
        return;
    };

    let points = match reference_line.orientation {
        ReferenceLineOrientation::Vertical => vec![
            ViewerPoint {
                screen_x: scale_x(reference_line.value, x_min, x_max, frame),
                screen_y: frame.top,
                data_x: reference_line.value,
                data_y: resolved_y_limits_for_side(axes, series.y_axis_side).0,
                data_z: None,
            },
            ViewerPoint {
                screen_x: scale_x(reference_line.value, x_min, x_max, frame),
                screen_y: frame.bottom(),
                data_x: reference_line.value,
                data_y: resolved_y_limits_for_side(axes, series.y_axis_side).1,
                data_z: None,
            },
        ],
        ReferenceLineOrientation::Horizontal => vec![
            ViewerPoint {
                screen_x: frame.left,
                screen_y: scale_y(reference_line.value, y_min, y_max, frame),
                data_x: x_min,
                data_y: reference_line.value,
                data_z: None,
            },
            ViewerPoint {
                screen_x: frame.right(),
                screen_y: scale_y(reference_line.value, y_min, y_max, frame),
                data_x: x_max,
                data_y: reference_line.value,
                data_z: None,
            },
        ],
    };
    render_styled_line_path(out, series, &points);

    if !reference_line.label.is_empty() {
        match reference_line.orientation {
            ReferenceLineOrientation::Vertical => out.push_str(&format!(
                "  <text x=\"{}\" y=\"{}\" font-size=\"12\" font-family=\"Segoe UI, Arial, sans-serif\" fill=\"{}\">{}</text>\n",
                format_number(points[0].screen_x + 6.0),
                format_number(frame.top + 16.0),
                series.color,
                svg_escape(&reference_line.label)
            )),
            ReferenceLineOrientation::Horizontal => out.push_str(&format!(
                "  <text x=\"{}\" y=\"{}\" text-anchor=\"end\" font-size=\"12\" font-family=\"Segoe UI, Arial, sans-serif\" fill=\"{}\">{}</text>\n",
                format_number(frame.right() - 6.0),
                format_number(points[0].screen_y - 6.0),
                series.color,
                svg_escape(&reference_line.label)
            )),
        }
    }
}

fn render_errorbar_series(
    out: &mut String,
    series: &PlotSeries,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
) {
    let Some(error_bar) = &series.error_bar else {
        render_line_series(out, series, frame, x_min, x_max, y_min, y_max);
        return;
    };

    let points = series
        .x
        .iter()
        .zip(&series.y)
        .map(|(x, y)| ViewerPoint {
            screen_x: scale_x(*x, x_min, x_max, frame),
            screen_y: scale_y(*y, y_min, y_max, frame),
            data_x: *x,
            data_y: *y,
            data_z: None,
        })
        .collect::<Vec<_>>();

    render_styled_line_path(out, series, &points);

    let cap_half_extent = 6.0;
    if let (Some(lower), Some(upper)) = (&error_bar.vertical_lower, &error_bar.vertical_upper) {
        for ((point, lower), upper) in points.iter().zip(lower).zip(upper) {
            let low_y = scale_y(point.data_y - *lower, y_min, y_max, frame);
            let high_y = scale_y(point.data_y + *upper, y_min, y_max, frame);
            render_svg_line(
                out,
                point.screen_x,
                high_y,
                point.screen_x,
                low_y,
                Some(series.color),
                1.4,
            );
            render_svg_line(
                out,
                point.screen_x - cap_half_extent,
                high_y,
                point.screen_x + cap_half_extent,
                high_y,
                Some(series.color),
                1.4,
            );
            render_svg_line(
                out,
                point.screen_x - cap_half_extent,
                low_y,
                point.screen_x + cap_half_extent,
                low_y,
                Some(series.color),
                1.4,
            );
        }
    }
    if let (Some(lower), Some(upper)) = (&error_bar.horizontal_lower, &error_bar.horizontal_upper) {
        for ((point, lower), upper) in points.iter().zip(lower).zip(upper) {
            let left_x = scale_x(point.data_x - *lower, x_min, x_max, frame);
            let right_x = scale_x(point.data_x + *upper, x_min, x_max, frame);
            render_svg_line(
                out,
                left_x,
                point.screen_y,
                right_x,
                point.screen_y,
                Some(series.color),
                1.4,
            );
            render_svg_line(
                out,
                left_x,
                point.screen_y - cap_half_extent,
                left_x,
                point.screen_y + cap_half_extent,
                Some(series.color),
                1.4,
            );
            render_svg_line(
                out,
                right_x,
                point.screen_y - cap_half_extent,
                right_x,
                point.screen_y + cap_half_extent,
                Some(series.color),
                1.4,
            );
        }
    }
}

fn render_scatter_series(
    out: &mut String,
    series: &PlotSeries,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
    colormap: ColormapKind,
    caxis_override: Option<(f64, f64)>,
) {
    for (index, (x_value, y_value)) in series.x.iter().zip(&series.y).enumerate() {
        let point = ViewerPoint {
            screen_x: scale_x(*x_value, x_min, x_max, frame),
            screen_y: scale_y(*y_value, y_min, y_max, frame),
            data_x: *x_value,
            data_y: *y_value,
            data_z: None,
        };
        render_scatter_marker_svg(out, series, index, point, colormap, caxis_override);
    }
}

fn render_styled_line_path(out: &mut String, series: &PlotSeries, points: &[ViewerPoint]) {
    if points.is_empty() {
        return;
    }

    if series.line_style != LineStyle::None {
        if points.len() == 1 {
            render_marker_svg(out, series, &points[0]);
            return;
        }

        let points_attribute = points
            .iter()
            .map(|point| {
                format!(
                    "{},{}",
                    format_number(point.screen_x),
                    format_number(point.screen_y)
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        let dash = series
            .line_style
            .stroke_dasharray()
            .map(|dash| format!(" stroke-dasharray=\"{dash}\""))
            .unwrap_or_default();
        let metadata = viewer_points_attributes(points, "matc-series-path");
        out.push_str(&format!(
            "  <polyline{} fill=\"none\" stroke=\"{}\" stroke-width=\"{}\"{} points=\"{}\"/>\n",
            metadata,
            series.color,
            format_number(series.line_width),
            dash,
            points_attribute
        ));
    }

    if series.marker != MarkerStyle::None || points.len() == 1 {
        for point in points {
            render_marker_svg(out, series, point);
        }
    }
}

fn render_marker_svg(out: &mut String, series: &PlotSeries, point: &ViewerPoint) {
    let stroke_color = series.marker_edge_color.resolve(series.color);
    let fill_color = series.marker_face_color.resolve(series.color);
    let metadata = viewer_point_attributes(point, "matc-datapoint");
    render_marker_svg_with_style(
        out,
        series,
        point.screen_x,
        point.screen_y,
        series.marker_size,
        stroke_color,
        fill_color,
        Some(metadata.as_str()),
    );
}

fn render_scatter_marker_svg(
    out: &mut String,
    series: &PlotSeries,
    index: usize,
    point: ViewerPoint,
    colormap: ColormapKind,
    caxis_override: Option<(f64, f64)>,
) {
    let Some(scatter) = &series.scatter else {
        render_marker_svg(out, series, &point);
        return;
    };

    let marker_size = scatter
        .marker_sizes
        .get(index)
        .copied()
        .unwrap_or(series.marker_size);
    let base_color = scatter_point_color(scatter, index, colormap, caxis_override, series.color);
    let stroke_color = series.marker_edge_color.resolve(base_color.as_str());
    let fill_color = series.marker_face_color.resolve(base_color.as_str());
    let metadata = viewer_point_attributes(&point, "matc-datapoint");
    render_marker_svg_with_style(
        out,
        series,
        point.screen_x,
        point.screen_y,
        marker_size,
        stroke_color,
        fill_color,
        Some(metadata.as_str()),
    );
}

fn scatter_point_color(
    scatter: &ScatterSeriesData,
    index: usize,
    colormap: ColormapKind,
    caxis_override: Option<(f64, f64)>,
    fallback: &'static str,
) -> String {
    match &scatter.colors {
        ScatterColors::Uniform(color) => (*color).to_string(),
        ScatterColors::Rgb(colors) => colors
            .get(index)
            .copied()
            .map(rgb_string)
            .unwrap_or_else(|| fallback.to_string()),
        ScatterColors::Colormapped(values) => {
            let range = caxis_override.unwrap_or_else(|| finite_min_max(values));
            let value = values.get(index).copied().unwrap_or(range.0);
            sample_colormap(colormap, normalized_color_value(value, range))
        }
    }
}

fn render_marker_svg_with_style(
    out: &mut String,
    series: &PlotSeries,
    x: f64,
    y: f64,
    marker_size: f64,
    stroke_color: Option<&str>,
    fill_color: Option<&str>,
    metadata_attributes: Option<&str>,
) {
    if let Some(attributes) = metadata_attributes {
        out.push_str(&format!("  <g{}>\n", attributes));
    }
    let size = (marker_size / 2.0).max(1.0);
    let stroke_width = (series.line_width / 2.0).max(1.0);
    match series.marker {
        MarkerStyle::None => out.push_str(&format!(
            "  <circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\" fill-opacity=\"0.85\"/>\n",
            format_number(x),
            format_number(y),
            format_number(size.max(4.0)),
            series.color
        )),
        MarkerStyle::Point => out.push_str(&format!(
            "  <circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\" fill-opacity=\"0.9\"/>\n",
            format_number(x),
            format_number(y),
            format_number((size / 2.5).max(1.2)),
            fill_color.or(stroke_color).unwrap_or(series.color)
        )),
        MarkerStyle::Circle => {
            render_svg_circle(out, x, y, size, stroke_color, fill_color, stroke_width)
        }
        MarkerStyle::XMark => {
            render_svg_line(
                out,
                x - size,
                y - size,
                x + size,
                y + size,
                stroke_color.or(fill_color),
                stroke_width,
            );
            render_svg_line(
                out,
                x - size,
                y + size,
                x + size,
                y - size,
                stroke_color.or(fill_color),
                stroke_width,
            );
        }
        MarkerStyle::Plus => {
            render_svg_line(
                out,
                x - size,
                y,
                x + size,
                y,
                stroke_color.or(fill_color),
                stroke_width,
            );
            render_svg_line(
                out,
                x,
                y - size,
                x,
                y + size,
                stroke_color.or(fill_color),
                stroke_width,
            );
        }
        MarkerStyle::Star => {
            let color = stroke_color.or(fill_color);
            render_svg_line(out, x - size, y, x + size, y, color, stroke_width);
            render_svg_line(out, x, y - size, x, y + size, color, stroke_width);
            render_svg_line(
                out,
                x - size,
                y - size,
                x + size,
                y + size,
                color,
                stroke_width,
            );
            render_svg_line(
                out,
                x - size,
                y + size,
                x + size,
                y - size,
                color,
                stroke_width,
            );
        }
        MarkerStyle::Square => render_svg_rect(
            out,
            x - size,
            y - size,
            size * 2.0,
            size * 2.0,
            stroke_color,
            fill_color,
            stroke_width,
        ),
        MarkerStyle::Diamond => render_svg_polygon(
            out,
            &[(x, y - size), (x + size, y), (x, y + size), (x - size, y)],
            stroke_color,
            fill_color,
            stroke_width,
        ),
        MarkerStyle::TriangleDown => render_svg_polygon(
            out,
            &[
                (x - size, y - size * 0.85),
                (x + size, y - size * 0.85),
                (x, y + size),
            ],
            stroke_color,
            fill_color,
            stroke_width,
        ),
        MarkerStyle::TriangleUp => render_svg_polygon(
            out,
            &[
                (x, y - size),
                (x + size, y + size * 0.85),
                (x - size, y + size * 0.85),
            ],
            stroke_color,
            fill_color,
            stroke_width,
        ),
        MarkerStyle::TriangleLeft => render_svg_polygon(
            out,
            &[
                (x - size, y),
                (x + size * 0.85, y - size),
                (x + size * 0.85, y + size),
            ],
            stroke_color,
            fill_color,
            stroke_width,
        ),
        MarkerStyle::TriangleRight => render_svg_polygon(
            out,
            &[
                (x + size, y),
                (x - size * 0.85, y - size),
                (x - size * 0.85, y + size),
            ],
            stroke_color,
            fill_color,
            stroke_width,
        ),
        MarkerStyle::Pentagram => render_svg_polygon(
            out,
            &star_polygon_points(x, y, size, 5, 0.45, -90.0),
            stroke_color,
            fill_color,
            stroke_width,
        ),
        MarkerStyle::Hexagram => {
            render_svg_polygon(
                out,
                &[
                    (x, y - size),
                    (x + size * 0.866, y + size * 0.5),
                    (x - size * 0.866, y + size * 0.5),
                ],
                stroke_color,
                fill_color,
                stroke_width,
            );
            render_svg_polygon(
                out,
                &[
                    (x, y + size),
                    (x + size * 0.866, y - size * 0.5),
                    (x - size * 0.866, y - size * 0.5),
                ],
                stroke_color,
                fill_color,
                stroke_width,
            );
        }
    }
    if metadata_attributes.is_some() {
        out.push_str("  </g>\n");
    }
}

fn render_svg_line(
    out: &mut String,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    color: Option<&str>,
    stroke_width: f64,
) {
    let Some(color) = color else {
        return;
    };
    out.push_str(&format!(
        "  <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"{}\" stroke-linecap=\"round\"/>\n",
        format_number(x1),
        format_number(y1),
        format_number(x2),
        format_number(y2),
        color,
        format_number(stroke_width),
    ));
}

fn render_svg_circle(
    out: &mut String,
    x: f64,
    y: f64,
    radius: f64,
    stroke: Option<&str>,
    fill: Option<&str>,
    stroke_width: f64,
) {
    out.push_str(&format!(
        "  <circle cx=\"{}\" cy=\"{}\" r=\"{}\"{}{} />\n",
        format_number(x),
        format_number(y),
        format_number(radius),
        svg_fill_attributes(fill),
        svg_stroke_attributes(stroke, stroke_width),
    ));
}

fn render_svg_rect(
    out: &mut String,
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    stroke: Option<&str>,
    fill: Option<&str>,
    stroke_width: f64,
) {
    out.push_str(&format!(
        "  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"{}{} />\n",
        format_number(x),
        format_number(y),
        format_number(width),
        format_number(height),
        svg_fill_attributes(fill),
        svg_stroke_attributes(stroke, stroke_width),
    ));
}

fn render_svg_polygon(
    out: &mut String,
    points: &[(f64, f64)],
    stroke: Option<&str>,
    fill: Option<&str>,
    stroke_width: f64,
) {
    out.push_str(&format!(
        "  <polygon points=\"{}\"{}{} />\n",
        svg_points_attribute(points),
        svg_fill_attributes(fill),
        svg_stroke_attributes(stroke, stroke_width),
    ));
}

fn svg_points_attribute(points: &[(f64, f64)]) -> String {
    points
        .iter()
        .map(|(x, y)| format!("{},{}", format_number(*x), format_number(*y)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn viewer_points_attributes(points: &[ViewerPoint], class_name: &str) -> String {
    format!(
        " class=\"{}\" data-matc-dim=\"{}\" data-matc-data=\"{}\" data-matc-screen=\"{}\"",
        class_name,
        if points.iter().any(|point| point.data_z.is_some()) {
            3
        } else {
            2
        },
        svg_escape(
            &points
                .iter()
                .map(viewer_point_data_text)
                .collect::<Vec<_>>()
                .join(";")
        ),
        svg_escape(
            &points
                .iter()
                .map(|point| {
                    format!(
                        "{},{}",
                        format_number(point.screen_x),
                        format_number(point.screen_y)
                    )
                })
                .collect::<Vec<_>>()
                .join(";")
        ),
    )
}

fn viewer_three_d_attributes(
    points: &[(f64, f64, f64)],
    screen_points: &[(f64, f64)],
    class_name: &str,
) -> String {
    format!(
        " class=\"{}\" data-matc-dim=\"3\" data-matc-data=\"{}\" data-matc-screen=\"{}\"",
        class_name,
        svg_escape(
            &points
                .iter()
                .map(|(x, y, z)| {
                    format!(
                        "{},{},{}",
                        format_number(*x),
                        format_number(*y),
                        format_number(*z)
                    )
                })
                .collect::<Vec<_>>()
                .join(";")
        ),
        svg_escape(
            &screen_points
                .iter()
                .map(|(x, y)| format!("{},{}", format_number(*x), format_number(*y)))
                .collect::<Vec<_>>()
                .join(";")
        ),
    )
}

fn viewer_point_attributes(point: &ViewerPoint, class_name: &str) -> String {
    viewer_points_attributes(std::slice::from_ref(point), class_name)
}

fn viewer_point_data_text(point: &ViewerPoint) -> String {
    match point.data_z {
        Some(z) => format!(
            "{},{},{}",
            format_number(point.data_x),
            format_number(point.data_y),
            format_number(z)
        ),
        None => format!(
            "{},{}",
            format_number(point.data_x),
            format_number(point.data_y)
        ),
    }
}

fn svg_fill_attributes(fill: Option<&str>) -> String {
    match fill {
        Some(color) => format!(" fill=\"{}\" fill-opacity=\"0.85\"", color),
        None => " fill=\"none\"".to_string(),
    }
}

fn svg_stroke_attributes(stroke: Option<&str>, stroke_width: f64) -> String {
    match stroke {
        Some(color) => format!(
            " stroke=\"{}\" stroke-width=\"{}\" stroke-linejoin=\"round\" stroke-linecap=\"round\"",
            color,
            format_number(stroke_width),
        ),
        None => " stroke=\"none\"".to_string(),
    }
}

fn star_polygon_points(
    center_x: f64,
    center_y: f64,
    radius: f64,
    points: usize,
    inner_ratio: f64,
    rotation_degrees: f64,
) -> Vec<(f64, f64)> {
    let rotation = rotation_degrees.to_radians();
    (0..points * 2)
        .map(|index| {
            let angle = rotation + std::f64::consts::PI * index as f64 / points as f64;
            let magnitude = if index % 2 == 0 {
                radius
            } else {
                radius * inner_ratio
            };
            (
                center_x + magnitude * angle.cos(),
                center_y + magnitude * angle.sin(),
            )
        })
        .collect()
}

fn render_text_series(
    out: &mut String,
    series: &PlotSeries,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
) {
    let Some(text) = &series.text else {
        return;
    };

    let x = scale_x(text.x, x_min, x_max, frame);
    let y = scale_y(text.y, y_min, y_max, frame);
    out.push_str(&format!(
        "  <text x=\"{}\" y=\"{}\" font-size=\"13\" font-family=\"Segoe UI, Arial, sans-serif\" fill=\"{}\">{}</text>\n",
        format_number(x),
        format_number(y),
        series.color,
        svg_escape(&text.label)
    ));
}

fn render_rectangle_series(
    out: &mut String,
    series: &PlotSeries,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
) {
    let Some(rectangle) = &series.rectangle else {
        return;
    };

    let left = scale_x(
        rectangle.x.min(rectangle.x + rectangle.width),
        x_min,
        x_max,
        frame,
    );
    let right = scale_x(
        rectangle.x.max(rectangle.x + rectangle.width),
        x_min,
        x_max,
        frame,
    );
    let top = scale_y(
        rectangle.y.max(rectangle.y + rectangle.height),
        y_min,
        y_max,
        frame,
    );
    let bottom = scale_y(
        rectangle.y.min(rectangle.y + rectangle.height),
        y_min,
        y_max,
        frame,
    );
    let width = (right - left).abs();
    let height = (bottom - top).abs();
    let fill = rectangle.face_color.unwrap_or("none");
    let dash = series
        .line_style
        .stroke_dasharray()
        .map(|dash| format!(" stroke-dasharray=\"{dash}\""))
        .unwrap_or_default();
    let stroke = if series.line_style == LineStyle::None {
        "none"
    } else {
        series.color
    };
    out.push_str(&format!(
        "  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\"{}/>\n",
        format_number(left.min(right)),
        format_number(top.min(bottom)),
        format_number(width),
        format_number(height),
        fill,
        stroke,
        format_number(series.line_width),
        dash
    ));
}

fn render_patch_series(
    out: &mut String,
    series: &PlotSeries,
    axes: &AxesState,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
    three_d_range: Option<&ThreeDRange>,
) {
    let Some(patch) = &series.patch else {
        return;
    };
    if let (Some(three_d), Some(range)) = (&series.three_d, three_d_range) {
        let projection = project_polygon3d(
            &three_d.points,
            range,
            axes,
            frame,
            x_min,
            x_max,
            y_min,
            y_max,
        );
        let metadata = viewer_three_d_attributes(&three_d.points, &projection.0, "matc-3d-patch");
        let points = projection
            .0
            .iter()
            .map(|(x, y)| format!("{},{}", format_number(*x), format_number(*y)))
            .collect::<Vec<_>>()
            .join(" ");
        let dash = series
            .line_style
            .stroke_dasharray()
            .map(|dash| format!(" stroke-dasharray=\"{dash}\""))
            .unwrap_or_default();
        let stroke = if series.line_style == LineStyle::None {
            "none"
        } else {
            series.color
        };
        let fill = patch.face_color.unwrap_or("none");
        out.push_str(&format!(
            "  <polygon{} points=\"{}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\"{} stroke-linejoin=\"round\"/>\n",
            metadata,
            points,
            fill,
            stroke,
            format_number(series.line_width),
            dash
        ));
        return;
    }
    let points = series
        .x
        .iter()
        .zip(&series.y)
        .map(|(x, y)| {
            format!(
                "{},{}",
                format_number(scale_x(*x, x_min, x_max, frame)),
                format_number(scale_y(*y, y_min, y_max, frame))
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    let dash = series
        .line_style
        .stroke_dasharray()
        .map(|dash| format!(" stroke-dasharray=\"{dash}\""))
        .unwrap_or_default();
    let stroke = if series.line_style == LineStyle::None {
        "none"
    } else {
        series.color
    };
    let fill = patch.face_color.unwrap_or("none");
    out.push_str(&format!(
        "  <polygon points=\"{}\" fill=\"{}\" stroke=\"{}\" stroke-width=\"{}\"{} stroke-linejoin=\"round\"/>\n",
        points,
        fill,
        stroke,
        format_number(series.line_width),
        dash
    ));
}

fn render_quiver_series(
    out: &mut String,
    series: &PlotSeries,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
) {
    let Some(quiver) = &series.quiver else {
        return;
    };

    for ((base_x, base_y), (tip_x, tip_y)) in quiver.bases.iter().zip(&quiver.tips) {
        let x1 = scale_x(*base_x, x_min, x_max, frame);
        let y1 = scale_y(*base_y, y_min, y_max, frame);
        let x2 = scale_x(*tip_x, x_min, x_max, frame);
        let y2 = scale_y(*tip_y, y_min, y_max, frame);
        render_quiver_arrow(out, series.color, x1, y1, x2, y2);
    }
}

fn render_quiver_arrow(out: &mut String, color: &str, x1: f64, y1: f64, x2: f64, y2: f64) {
    render_quiver_arrow_with_attributes(out, color, x1, y1, x2, y2, None);
}

fn render_quiver_arrow_with_attributes(
    out: &mut String,
    color: &str,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    attributes: Option<&str>,
) {
    if let Some(attributes) = attributes {
        out.push_str(&format!("  <g{}>\n", attributes));
    }
    let dx = x2 - x1;
    let dy = y2 - y1;
    let length = (dx * dx + dy * dy).sqrt();
    if length <= f64::EPSILON {
        out.push_str(&format!(
            "  <circle cx=\"{}\" cy=\"{}\" r=\"2.5\" fill=\"{}\" fill-opacity=\"0.85\"/>\n",
            format_number(x1),
            format_number(y1),
            color
        ));
        if attributes.is_some() {
            out.push_str("  </g>\n");
        }
        return;
    }

    out.push_str(&format!(
        "  <line class=\"matc-arrow-shaft\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"2.2\" stroke-linecap=\"round\"/>\n",
        format_number(x1),
        format_number(y1),
        format_number(x2),
        format_number(y2),
        color
    ));

    let head_length = (length * 0.22).clamp(6.0, 12.0);
    let head_angle = 26.0_f64.to_radians();
    let unit_x = dx / length;
    let unit_y = dy / length;
    let back_x = -unit_x;
    let back_y = -unit_y;
    let cos_angle = head_angle.cos();
    let sin_angle = head_angle.sin();
    let left_x = x2 + head_length * (back_x * cos_angle - back_y * sin_angle);
    let left_y = y2 + head_length * (back_x * sin_angle + back_y * cos_angle);
    let right_x = x2 + head_length * (back_x * cos_angle + back_y * sin_angle);
    let right_y = y2 + head_length * (-back_x * sin_angle + back_y * cos_angle);

    out.push_str(&format!(
        "  <line class=\"matc-arrow-head-left\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"2.2\" stroke-linecap=\"round\"/>\n",
        format_number(x2),
        format_number(y2),
        format_number(left_x),
        format_number(left_y),
        color
    ));
    out.push_str(&format!(
        "  <line class=\"matc-arrow-head-right\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"2.2\" stroke-linecap=\"round\"/>\n",
        format_number(x2),
        format_number(y2),
        format_number(right_x),
        format_number(right_y),
        color
    ));
    if attributes.is_some() {
        out.push_str("  </g>\n");
    }
}

fn render_pie_series(
    out: &mut String,
    series: &PlotSeries,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
) {
    let Some(pie) = &series.pie else {
        return;
    };

    for slice in &pie.slices {
        let points = pie_slice_points(slice)
            .into_iter()
            .map(|(x, y)| {
                format!(
                    "{},{}",
                    format_number(scale_x(x, x_min, x_max, frame)),
                    format_number(scale_y(y, y_min, y_max, frame))
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&format!(
            "  <polygon points=\"{}\" fill=\"{}\" fill-opacity=\"0.88\" stroke=\"white\" stroke-width=\"1.2\" stroke-linejoin=\"round\"/>\n",
            points, slice.color
        ));

        if !slice.label.is_empty() && (slice.end_angle - slice.start_angle).abs() > 1e-9 {
            let (label_x, label_y) = pie_label_point(slice);
            out.push_str(&format!(
                "  <text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-size=\"12\" font-family=\"Segoe UI, Arial, sans-serif\" fill=\"#222222\">{}</text>\n",
                format_number(scale_x(label_x, x_min, x_max, frame)),
                format_number(scale_y(label_y, y_min, y_max, frame)),
                svg_escape(&slice.label)
            ));
        }
    }
}

fn render_pie3_series(
    out: &mut String,
    series: &PlotSeries,
    axes: &AxesState,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
    three_d_range: Option<&ThreeDRange>,
) {
    let (Some(pie), Some(range)) = (&series.pie, three_d_range) else {
        return;
    };

    struct Pie3PatchRender {
        points: Vec<(f64, f64, f64)>,
        screen: Vec<(f64, f64)>,
        depth: f64,
        fill: String,
        stroke: &'static str,
        stroke_width: f64,
    }

    let mut patches = Vec::new();
    for slice in &pie.slices {
        let (center_x, center_y) = pie_center(slice);
        let rim = pie_slice_rim_points(slice);
        let top_points = std::iter::once((center_x, center_y, PIE3_HEIGHT))
            .chain(rim.iter().copied().map(|(x, y)| (x, y, PIE3_HEIGHT)))
            .collect::<Vec<_>>();
        let top_projection =
            project_polygon3d(&top_points, range, axes, frame, x_min, x_max, y_min, y_max);
        patches.push(Pie3PatchRender {
            points: top_points,
            screen: top_projection.0,
            depth: top_projection.1,
            fill: slice.color.to_string(),
            stroke: "white",
            stroke_width: 1.0,
        });

        let side_fill = pie3_side_color(slice.color);
        for edge in rim.windows(2) {
            let wall = vec![
                (edge[0].0, edge[0].1, 0.0),
                (edge[1].0, edge[1].1, 0.0),
                (edge[1].0, edge[1].1, PIE3_HEIGHT),
                (edge[0].0, edge[0].1, PIE3_HEIGHT),
            ];
            let projection =
                project_polygon3d(&wall, range, axes, frame, x_min, x_max, y_min, y_max);
            patches.push(Pie3PatchRender {
                points: wall,
                screen: projection.0,
                depth: projection.1,
                fill: side_fill.clone(),
                stroke: "none",
                stroke_width: 0.0,
            });
        }

        let slice_span = (slice.end_angle - slice.start_angle).abs();
        if slice_span < std::f64::consts::TAU - 1e-9 {
            if let (Some(start), Some(end)) = (rim.first().copied(), rim.last().copied()) {
                for rim_point in [start, end] {
                    let radial = vec![
                        (center_x, center_y, 0.0),
                        (rim_point.0, rim_point.1, 0.0),
                        (rim_point.0, rim_point.1, PIE3_HEIGHT),
                        (center_x, center_y, PIE3_HEIGHT),
                    ];
                    let projection =
                        project_polygon3d(&radial, range, axes, frame, x_min, x_max, y_min, y_max);
                    patches.push(Pie3PatchRender {
                        points: radial,
                        screen: projection.0,
                        depth: projection.1,
                        fill: side_fill.clone(),
                        stroke: "none",
                        stroke_width: 0.0,
                    });
                }
            }
        }
    }

    patches.sort_by(|left, right| {
        left.depth
            .partial_cmp(&right.depth)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    for patch in patches {
        if patch.screen.is_empty() {
            continue;
        }
        let metadata = viewer_three_d_attributes(&patch.points, &patch.screen, "matc-3d-patch");
        let points = patch
            .screen
            .iter()
            .map(|(x, y)| format!("{},{}", format_number(*x), format_number(*y)))
            .collect::<Vec<_>>()
            .join(" ");
        let stroke = if patch.stroke == "none" {
            String::from(" stroke=\"none\"")
        } else {
            format!(
                " stroke=\"{}\" stroke-width=\"{}\" stroke-linejoin=\"round\"",
                patch.stroke,
                format_number(patch.stroke_width)
            )
        };
        out.push_str(&format!(
            "  <polygon{} points=\"{}\" fill=\"{}\" fill-opacity=\"0.94\"{}/>\n",
            metadata, points, patch.fill, stroke
        ));
    }

    for slice in &pie.slices {
        if !slice.label.is_empty() && (slice.end_angle - slice.start_angle).abs() > 1e-9 {
            let (label_x, label_y) = pie_label_point(slice);
            let (projected_x, projected_y, _) =
                project_3d_point(label_x, label_y, PIE3_HEIGHT + 0.02, range, axes);
            out.push_str(&format!(
                "  <text x=\"{}\" y=\"{}\" text-anchor=\"middle\" font-size=\"12\" font-family=\"Segoe UI, Arial, sans-serif\" fill=\"#222222\">{}</text>\n",
                format_number(scale_x(projected_x, x_min, x_max, frame)),
                format_number(scale_y(projected_y, y_min, y_max, frame)),
                svg_escape(&slice.label)
            ));
        }
    }
}

fn render_histogram_series(
    out: &mut String,
    series: &PlotSeries,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
) {
    let Some(histogram) = &series.histogram else {
        return;
    };

    let baseline = scale_y(0.0, y_min, y_max, frame);
    for (index, count) in histogram.counts.iter().copied().enumerate() {
        if count <= 0.0 {
            continue;
        }
        let left = scale_x(histogram.edges[index], x_min, x_max, frame);
        let right = scale_x(histogram.edges[index + 1], x_min, x_max, frame);
        let y = scale_y(count, y_min, y_max, frame);
        let rect_y = y.min(baseline);
        let rect_height = (baseline - y).abs().max(1.0);
        out.push_str(&format!(
            "  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\" fill-opacity=\"0.75\" stroke=\"{}\" stroke-width=\"1\"/>\n",
            format_number(left),
            format_number(rect_y),
            format_number((right - left).abs()),
            format_number(rect_height),
            series.color,
            series.color
        ));
    }
}

fn render_histogram2_series(
    out: &mut String,
    series: &PlotSeries,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
    colormap: ColormapKind,
    caxis_override: Option<(f64, f64)>,
) {
    let Some(histogram2) = &series.histogram2 else {
        return;
    };

    let range = caxis_override.unwrap_or(histogram2.count_range);
    for row in 0..histogram2.x_edges.len().saturating_sub(1) {
        for col in 0..histogram2.y_edges.len().saturating_sub(1) {
            let value = histogram2.counts[row * (histogram2.y_edges.len() - 1) + col];
            let normalized = if (range.1 - range.0).abs() <= f64::EPSILON {
                0.5
            } else {
                (value - range.0) / (range.1 - range.0)
            };
            let fill = sample_colormap(colormap, normalized);
            let left = scale_x(histogram2.x_edges[row], x_min, x_max, frame);
            let right = scale_x(histogram2.x_edges[row + 1], x_min, x_max, frame);
            let top = scale_y(histogram2.y_edges[col + 1], y_min, y_max, frame);
            let bottom = scale_y(histogram2.y_edges[col], y_min, y_max, frame);
            out.push_str(&format!(
                "  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\" fill-opacity=\"0.82\" stroke=\"#666666\" stroke-width=\"0.8\"/>\n",
                format_number(left.min(right)),
                format_number(top.min(bottom)),
                format_number((right - left).abs()),
                format_number((bottom - top).abs()),
                fill
            ));
        }
    }
}

fn render_area_series(
    out: &mut String,
    series: &PlotSeries,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
) {
    let points = area_polygon_points(&series.x, &series.y)
        .into_iter()
        .map(|(x, y)| {
            format!(
                "{},{}",
                format_number(scale_x(x, x_min, x_max, frame)),
                format_number(scale_y(y, y_min, y_max, frame))
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    out.push_str(&format!(
        "  <polygon points=\"{}\" fill=\"{}\" fill-opacity=\"0.3\" stroke=\"{}\" stroke-width=\"2\" stroke-linejoin=\"round\"/>\n",
        points, series.color, series.color
    ));
}

fn render_stairs_series(
    out: &mut String,
    series: &PlotSeries,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
) {
    if series.x.len() == 1 {
        render_line_series(out, series, frame, x_min, x_max, y_min, y_max);
        return;
    }

    let points = stair_points(&series.x, &series.y)
        .into_iter()
        .map(|(x, y)| {
            format!(
                "{},{}",
                format_number(scale_x(x, x_min, x_max, frame)),
                format_number(scale_y(y, y_min, y_max, frame))
            )
        })
        .collect::<Vec<_>>()
        .join(" ");
    out.push_str(&format!(
        "  <polyline fill=\"none\" stroke=\"{}\" stroke-width=\"2.5\" points=\"{}\"/>\n",
        series.color, points
    ));
}

fn render_bar_series(
    out: &mut String,
    series: &PlotSeries,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
) {
    let width = bar_width(&series.x);
    let baseline = scale_y(0.0, y_min, y_max, frame);
    for (x_value, y_value) in series.x.iter().zip(&series.y) {
        let left = scale_x(*x_value - width / 2.0, x_min, x_max, frame);
        let right = scale_x(*x_value + width / 2.0, x_min, x_max, frame);
        let y = scale_y(*y_value, y_min, y_max, frame);
        let rect_y = y.min(baseline);
        let rect_height = (baseline - y).abs().max(1.0);
        out.push_str(&format!(
            "  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\" fill-opacity=\"0.75\" stroke=\"{}\" stroke-width=\"1\"/>\n",
            format_number(left),
            format_number(rect_y),
            format_number((right - left).abs()),
            format_number(rect_height),
            series.color,
            series.color
        ));
    }
}

fn render_barh_series(
    out: &mut String,
    series: &PlotSeries,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
) {
    let height = bar_width(&series.y);
    let baseline = scale_x(0.0, x_min, x_max, frame);
    for (x_value, y_value) in series.x.iter().zip(&series.y) {
        let left = scale_x((*x_value).min(0.0), x_min, x_max, frame);
        let right = scale_x((*x_value).max(0.0), x_min, x_max, frame);
        let top = scale_y(*y_value + height / 2.0, y_min, y_max, frame);
        let bottom = scale_y(*y_value - height / 2.0, y_min, y_max, frame);
        out.push_str(&format!(
            "  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\" fill-opacity=\"0.75\" stroke=\"{}\" stroke-width=\"1\"/>\n",
            format_number(left.min(right)),
            format_number(top.min(bottom)),
            format_number((right - left).abs().max(1.0)),
            format_number((bottom - top).abs().max(1.0)),
            series.color,
            series.color
        ));
        out.push_str(&format!(
            "  <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#ffffff\" stroke-opacity=\"0\"/>\n",
            format_number(baseline),
            format_number((top + bottom) / 2.0),
            format_number(right),
            format_number((top + bottom) / 2.0)
        ));
    }
}

fn render_stem_series(
    out: &mut String,
    series: &PlotSeries,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
) {
    let baseline = scale_y(0.0, y_min, y_max, frame);
    for (x_value, y_value) in series.x.iter().zip(&series.y) {
        let x = scale_x(*x_value, x_min, x_max, frame);
        let y = scale_y(*y_value, y_min, y_max, frame);
        out.push_str(&format!(
            "  <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"1.8\"/>\n",
            format_number(x),
            format_number(baseline),
            format_number(x),
            format_number(y),
            series.color
        ));
        out.push_str(&format!(
            "  <circle cx=\"{}\" cy=\"{}\" r=\"4\" fill=\"{}\"/>\n",
            format_number(x),
            format_number(y),
            series.color
        ));
    }
}

fn render_stem3d_series(
    out: &mut String,
    series: &PlotSeries,
    axes: &AxesState,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
    three_d_range: Option<&ThreeDRange>,
) {
    let (Some(three_d), Some(range)) = (&series.three_d, three_d_range) else {
        return;
    };

    for (x_value, y_value, z_value) in &three_d.points {
        let (base_x, base_y, _) = project_3d_point(*x_value, *y_value, 0.0, range, axes);
        let (tip_x, tip_y, _) = project_3d_point(*x_value, *y_value, *z_value, range, axes);
        let x1 = scale_x(base_x, x_min, x_max, frame);
        let y1 = scale_y(base_y, y_min, y_max, frame);
        let x2 = scale_x(tip_x, x_min, x_max, frame);
        let y2 = scale_y(tip_y, y_min, y_max, frame);
        let metadata = viewer_three_d_attributes(
            &[(*x_value, *y_value, 0.0), (*x_value, *y_value, *z_value)],
            &[(x1, y1), (x2, y2)],
            "matc-3d-stem",
        );
        out.push_str(&format!(
            "  <line{} x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"1.8\"/>\n",
            metadata,
            format_number(x1),
            format_number(y1),
            format_number(x2),
            format_number(y2),
            series.color
        ));
        render_marker_svg(
            out,
            series,
            &ViewerPoint {
                screen_x: x2,
                screen_y: y2,
                data_x: *x_value,
                data_y: *y_value,
                data_z: Some(*z_value),
            },
        );
    }
}

fn render_contour_series(
    out: &mut String,
    series: &PlotSeries,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
) {
    let Some(contour) = &series.contour else {
        return;
    };

    for segment in &contour.segments {
        let x1 = scale_x(segment.start.0, x_min, x_max, frame);
        let y1 = scale_y(segment.start.1, y_min, y_max, frame);
        let x2 = scale_x(segment.end.0, x_min, x_max, frame);
        let y2 = scale_y(segment.end.1, y_min, y_max, frame);
        out.push_str(&format!(
            "  <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"1.8\" stroke-linecap=\"round\"/>\n",
            format_number(x1),
            format_number(y1),
            format_number(x2),
            format_number(y2),
            series.color
        ));
    }
}

fn render_contour3_series(
    out: &mut String,
    series: &PlotSeries,
    axes: &AxesState,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
    three_d_range: Option<&ThreeDRange>,
) {
    let (Some(contour), Some(range)) = (&series.contour, three_d_range) else {
        return;
    };

    for segment in &contour.segments {
        let (start_x, start_y, _) =
            project_3d_point(segment.start.0, segment.start.1, segment.level, range, axes);
        let (end_x, end_y, _) =
            project_3d_point(segment.end.0, segment.end.1, segment.level, range, axes);
        let screen_points = [
            (
                scale_x(start_x, x_min, x_max, frame),
                scale_y(start_y, y_min, y_max, frame),
            ),
            (
                scale_x(end_x, x_min, x_max, frame),
                scale_y(end_y, y_min, y_max, frame),
            ),
        ];
        let metadata = viewer_three_d_attributes(
            &[
                (segment.start.0, segment.start.1, segment.level),
                (segment.end.0, segment.end.1, segment.level),
            ],
            &screen_points,
            "matc-series-path",
        );
        out.push_str(&format!(
            "  <polyline{} fill=\"none\" stroke=\"{}\" stroke-width=\"1.8\" stroke-linecap=\"round\" points=\"{},{} {},{}\"/>\n",
            metadata,
            series.color,
            format_number(screen_points[0].0),
            format_number(screen_points[0].1),
            format_number(screen_points[1].0),
            format_number(screen_points[1].1),
        ));
    }
}

fn render_contour_fill_series(
    out: &mut String,
    series: &PlotSeries,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
    colormap: ColormapKind,
    caxis_override: Option<(f64, f64)>,
) {
    let Some(contour_fill) = &series.contour_fill else {
        return;
    };

    let range = caxis_override.unwrap_or(contour_fill.level_range);
    for patch in &contour_fill.patches {
        let normalized = if (range.1 - range.0).abs() <= f64::EPSILON {
            0.5
        } else {
            (patch.color_value - range.0) / (range.1 - range.0)
        };
        let fill = sample_colormap(colormap, normalized);
        let points = patch
            .points
            .iter()
            .map(|(x, y)| {
                format!(
                    "{},{}",
                    format_number(scale_x(*x, x_min, x_max, frame)),
                    format_number(scale_y(*y, y_min, y_max, frame))
                )
            })
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&format!(
            "  <polygon points=\"{}\" fill=\"{}\" fill-opacity=\"0.94\" stroke=\"none\"/>\n",
            points, fill
        ));
    }
}

fn render_waterfall_series(
    out: &mut String,
    series: &PlotSeries,
    axes: &AxesState,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
    three_d_range: Option<&ThreeDRange>,
) {
    let (Some(_surface), Some(range), Some(grid)) = (
        &series.surface,
        three_d_range,
        series
            .surface
            .as_ref()
            .and_then(|surface| surface.grid.as_ref()),
    ) else {
        return;
    };

    let base_z = range.z_range.0.min(0.0);
    let curtain_fill = css_color_rgb(series.color)
        .map(|rgb| rgb_string([rgb[0] * 0.82, rgb[1] * 0.82, rgb[2] * 0.82]))
        .unwrap_or_else(|| series.color.to_string());

    struct WaterfallCurtain {
        points: Vec<(f64, f64, f64)>,
        screen: Vec<(f64, f64)>,
        depth: f64,
    }

    let mut curtains = Vec::new();
    for row in 0..grid.rows {
        let start = row * grid.cols;
        let end = start + grid.cols;
        let row_points = (start..end)
            .map(|index| {
                (
                    grid.x_values[index],
                    grid.y_values[index],
                    grid.z_values[index],
                )
            })
            .collect::<Vec<_>>();
        let projected = row_points
            .iter()
            .map(|(x, y, z)| {
                let (projected_x, projected_y, _) = project_3d_point(*x, *y, *z, range, axes);
                ViewerPoint {
                    screen_x: scale_x(projected_x, x_min, x_max, frame),
                    screen_y: scale_y(projected_y, y_min, y_max, frame),
                    data_x: *x,
                    data_y: *y,
                    data_z: Some(*z),
                }
            })
            .collect::<Vec<_>>();
        render_styled_line_path(out, series, &projected);

        for segment in row_points.windows(2) {
            let polygon = vec![
                (segment[0].0, segment[0].1, base_z),
                (segment[1].0, segment[1].1, base_z),
                segment[1],
                segment[0],
            ];
            let projection =
                project_polygon3d(&polygon, range, axes, frame, x_min, x_max, y_min, y_max);
            curtains.push(WaterfallCurtain {
                points: polygon,
                screen: projection.0,
                depth: projection.1,
            });
        }
    }

    curtains.sort_by(|left, right| {
        left.depth
            .partial_cmp(&right.depth)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for curtain in curtains {
        let metadata = viewer_three_d_attributes(&curtain.points, &curtain.screen, "matc-3d-patch");
        let points = curtain
            .screen
            .iter()
            .map(|(x, y)| format!("{},{}", format_number(*x), format_number(*y)))
            .collect::<Vec<_>>()
            .join(" ");
        out.push_str(&format!(
            "  <polygon{} points=\"{}\" fill=\"{}\" fill-opacity=\"0.26\" stroke=\"none\"/>\n",
            metadata, points, curtain_fill
        ));
    }
}

fn scale_surface_points(
    projected_points: &[(f64, f64); 4],
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
) -> [(f64, f64); 4] {
    let mut scaled = [(0.0, 0.0); 4];
    for (index, (x, y)) in projected_points.iter().enumerate() {
        scaled[index] = (
            scale_x(*x, x_min, x_max, frame),
            scale_y(*y, y_min, y_max, frame),
        );
    }
    scaled
}

fn polygon_points_attribute(points: &[(f64, f64); 4]) -> String {
    points
        .iter()
        .map(|(x, y)| format!("{},{}", format_number(*x), format_number(*y)))
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalized_color_value(value: f64, range: (f64, f64)) -> f64 {
    if (range.1 - range.0).abs() <= f64::EPSILON {
        0.5
    } else {
        (value - range.0) / (range.1 - range.0)
    }
}

fn gradient_fill_for_surface_patch(
    out: &mut String,
    points: &[(f64, f64); 4],
    patch: &SurfacePatch,
    color_range: (f64, f64),
    colormap: ColormapKind,
) -> String {
    let z_values = patch.points.map(|(_, _, z)| z);
    let mut min_index = 0usize;
    let mut max_index = 0usize;
    for index in 1..z_values.len() {
        if z_values[index] < z_values[min_index] {
            min_index = index;
        }
        if z_values[index] > z_values[max_index] {
            max_index = index;
        }
    }

    if (z_values[max_index] - z_values[min_index]).abs() <= f64::EPSILON {
        return sample_colormap(
            colormap,
            normalized_color_value(z_values[min_index], color_range),
        );
    }

    let gradient_id = format!("patch-grad-{}", out.len());
    let start_color = sample_colormap(
        colormap,
        normalized_color_value(z_values[min_index], color_range),
    );
    let end_color = sample_colormap(
        colormap,
        normalized_color_value(z_values[max_index], color_range),
    );
    out.push_str(&format!(
        "  <defs><linearGradient id=\"{}\" gradientUnits=\"userSpaceOnUse\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"><stop offset=\"0%\" stop-color=\"{}\"/><stop offset=\"100%\" stop-color=\"{}\"/></linearGradient></defs>\n",
        gradient_id,
        format_number(points[min_index].0),
        format_number(points[min_index].1),
        format_number(points[max_index].0),
        format_number(points[max_index].1),
        start_color,
        end_color
    ));
    format!("url(#{gradient_id})")
}

fn render_surface_series(
    out: &mut String,
    series: &PlotSeries,
    axes: &AxesState,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
    colormap: ColormapKind,
    caxis_override: Option<(f64, f64)>,
    three_d_range: Option<&ThreeDRange>,
) {
    let (Some(surface), Some(projection_range)) = (&series.surface, three_d_range) else {
        return;
    };

    let color_range = caxis_override.unwrap_or(surface.z_range);
    let mut patches = surface
        .patches
        .iter()
        .map(|patch| {
            let (projected_points, depth) = project_surface_patch(patch, projection_range, axes);
            (*patch, projected_points, depth)
        })
        .collect::<Vec<_>>();
    patches.sort_by(|left, right| {
        left.2
            .partial_cmp(&right.2)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for (patch, projected_points, _) in &patches {
        let scaled_points =
            scale_surface_points(projected_points, frame, x_min, x_max, y_min, y_max);
        let fill = match axes.shading_mode {
            ShadingMode::Interp => {
                gradient_fill_for_surface_patch(out, &scaled_points, patch, color_range, colormap)
            }
            _ => sample_colormap(
                colormap,
                normalized_color_value(patch.color_value, color_range),
            ),
        };
        let points = polygon_points_attribute(&scaled_points);
        let metadata = viewer_three_d_attributes(&patch.points, &scaled_points, "matc-3d-patch");
        match axes.shading_mode {
            ShadingMode::Faceted => out.push_str(&format!(
                "  <polygon{} points=\"{}\" fill=\"{}\" fill-opacity=\"0.96\" stroke=\"#444444\" stroke-opacity=\"0.45\" stroke-width=\"0.7\" stroke-linejoin=\"round\"/>\n",
                metadata, points, fill
            )),
            ShadingMode::Flat | ShadingMode::Interp => out.push_str(&format!(
                "  <polygon{} points=\"{}\" fill=\"{}\" fill-opacity=\"0.96\" stroke=\"none\"/>\n",
                metadata, points, fill
            )),
        }
    }
}

fn render_mesh_series(
    out: &mut String,
    series: &PlotSeries,
    axes: &AxesState,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
    colormap: ColormapKind,
    caxis_override: Option<(f64, f64)>,
    three_d_range: Option<&ThreeDRange>,
) {
    let (Some(surface), Some(projection_range)) = (&series.surface, three_d_range) else {
        return;
    };

    let color_range = caxis_override.unwrap_or(surface.z_range);
    let mut patches = surface
        .patches
        .iter()
        .map(|patch| {
            let (projected_points, depth) = project_surface_patch(patch, projection_range, axes);
            (*patch, projected_points, depth)
        })
        .collect::<Vec<_>>();
    patches.sort_by(|left, right| {
        left.2
            .partial_cmp(&right.2)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    for (patch, projected_points, _) in &patches {
        let scaled_points =
            scale_surface_points(projected_points, frame, x_min, x_max, y_min, y_max);
        let points = polygon_points_attribute(&scaled_points);
        let metadata = viewer_three_d_attributes(&patch.points, &scaled_points, "matc-3d-patch");
        match axes.shading_mode {
            ShadingMode::Faceted => {
                let stroke = sample_colormap(
                    colormap,
                    normalized_color_value(patch.color_value, color_range),
                );
                out.push_str(&format!(
                    "  <polygon{} points=\"{}\" fill=\"none\" stroke=\"{}\" stroke-opacity=\"0.88\" stroke-width=\"1\" stroke-linejoin=\"round\"/>\n",
                    metadata, points, stroke
                ));
            }
            ShadingMode::Flat => {
                let fill = sample_colormap(
                    colormap,
                    normalized_color_value(patch.color_value, color_range),
                );
                out.push_str(&format!(
                    "  <polygon{} points=\"{}\" fill=\"{}\" fill-opacity=\"0.96\" stroke=\"none\"/>\n",
                    metadata, points, fill
                ));
            }
            ShadingMode::Interp => {
                let fill = gradient_fill_for_surface_patch(
                    out,
                    &scaled_points,
                    patch,
                    color_range,
                    colormap,
                );
                out.push_str(&format!(
                    "  <polygon{} points=\"{}\" fill=\"{}\" fill-opacity=\"0.96\" stroke=\"none\"/>\n",
                    metadata, points, fill
                ));
            }
        }
    }
}

fn render_image_series(
    out: &mut String,
    series: &PlotSeries,
    frame: AxesFrame,
    x_min: f64,
    x_max: f64,
    y_min: f64,
    y_max: f64,
    colormap: ColormapKind,
    caxis_override: Option<(f64, f64)>,
) {
    let Some(image) = &series.image else {
        return;
    };

    let x_edges = image_coordinate_edges(&image.x_data);
    let y_edges = image_coordinate_edges(&image.y_data);
    if let Some(rgb_values) = &image.rgb_values {
        for row in 0..image.rows {
            for col in 0..image.cols {
                let index = row * image.cols + col;
                let fill = rgb_string(rgb_values[index]);
                let opacity = svg_fill_opacity_attribute(resolved_image_alpha(image, index));
                let left = scale_x(x_edges[col], x_min, x_max, frame);
                let right = scale_x(x_edges[col + 1], x_min, x_max, frame);
                let top = scale_y(y_edges[row + 1], y_min, y_max, frame);
                let bottom = scale_y(y_edges[row], y_min, y_max, frame);
                out.push_str(&format!(
                    "  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"{}/>\n",
                    format_number(left.min(right)),
                    format_number(top.min(bottom)),
                    format_number((right - left).abs() + 0.2),
                    format_number((bottom - top).abs() + 0.2),
                    fill,
                    opacity
                ));
            }
        }
        return;
    }

    let direct_upper = colormap_palette(colormap).len() as f64;
    for row in 0..image.rows {
        for col in 0..image.cols {
            let index = row * image.cols + col;
            let value = image.values[index];
            let normalized = match image.mapping {
                ImageMapping::Scaled => {
                    let range = caxis_override.unwrap_or(image.display_range);
                    if (range.1 - range.0).abs() <= f64::EPSILON {
                        0.5
                    } else {
                        (value - range.0) / (range.1 - range.0)
                    }
                }
                ImageMapping::Direct => {
                    if direct_upper <= 1.0 {
                        0.0
                    } else {
                        (value - 1.0) / (direct_upper - 1.0)
                    }
                }
            };
            let fill = sample_colormap(colormap, normalized);
            let opacity = svg_fill_opacity_attribute(resolved_image_alpha(image, index));
            let left = scale_x(x_edges[col], x_min, x_max, frame);
            let right = scale_x(x_edges[col + 1], x_min, x_max, frame);
            let top = scale_y(y_edges[row + 1], y_min, y_max, frame);
            let bottom = scale_y(y_edges[row], y_min, y_max, frame);
            out.push_str(&format!(
                "  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\"{}/>\n",
                format_number(left.min(right)),
                format_number(top.min(bottom)),
                format_number((right - left).abs() + 0.2),
                format_number((bottom - top).abs() + 0.2),
                fill,
                opacity
            ));
        }
    }
}

fn render_legend_svg(out: &mut String, axes: &AxesState, frame: AxesFrame) {
    let Some(labels) = &axes.legend else {
        return;
    };
    if labels.is_empty() {
        return;
    }

    let row_height = 20.0;
    let horizontal = axes.legend_orientation == LegendOrientation::Horizontal;
    let item_width = 78.0;
    let padding = 10.0;
    let (box_width, box_height) = if horizontal {
        (
            (padding * 2.0 + labels.len() as f64 * item_width).clamp(120.0, frame.width - 20.0),
            30.0,
        )
    } else {
        (
            (frame.width * 0.34).clamp(100.0, 148.0),
            12.0 + labels.len() as f64 * row_height,
        )
    };
    let (box_x, box_y) = legend_box_origin(frame, box_width, box_height, axes.legend_location);
    out.push_str(&format!(
        "  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"white\" fill-opacity=\"0.94\" stroke=\"#bbbbbb\"/>\n",
        format_number(box_x),
        format_number(box_y),
        format_number(box_width),
        format_number(box_height)
    ));

    for (index, (label, series)) in labels.iter().zip(&axes.series).enumerate() {
        let (sample_x, sample_y, text_x, text_y) = if horizontal {
            let x = box_x + padding + index as f64 * item_width;
            (x, box_y + 16.0, x + 28.0, box_y + 20.0)
        } else {
            let y = box_y + 16.0 + index as f64 * row_height;
            (box_x + 10.0, y, box_x + 38.0, y + 4.0)
        };
        render_legend_sample(out, series, sample_x, sample_y);
        out.push_str(&format!(
            "  <text x=\"{}\" y=\"{}\" font-size=\"12\" font-family=\"Segoe UI, Arial, sans-serif\" fill=\"#222222\">{}</text>\n",
            format_number(text_x),
            format_number(text_y),
            svg_escape(label)
        ));
    }
}

fn legend_box_origin(
    frame: AxesFrame,
    box_width: f64,
    box_height: f64,
    location: LegendLocation,
) -> (f64, f64) {
    let margin = 10.0;
    let x = match location {
        LegendLocation::North | LegendLocation::South | LegendLocation::Best => {
            frame.right() - box_width - margin
        }
        LegendLocation::East | LegendLocation::Northeast | LegendLocation::Southeast => {
            frame.right() - box_width - margin
        }
        LegendLocation::West | LegendLocation::Northwest | LegendLocation::Southwest => {
            frame.left + margin
        }
    };
    let y = match location {
        LegendLocation::North
        | LegendLocation::Northeast
        | LegendLocation::Northwest
        | LegendLocation::Best => frame.top + margin,
        LegendLocation::South | LegendLocation::Southeast | LegendLocation::Southwest => {
            frame.bottom() - box_height - margin
        }
        LegendLocation::East | LegendLocation::West => {
            frame.top + (frame.height - box_height) / 2.0
        }
    };
    (x, y)
}

fn render_colorbar_svg(out: &mut String, axes: &AxesState, frame: AxesFrame) {
    if !axes
        .series
        .iter()
        .filter(|series| series.visible)
        .any(|series| {
            series.image.is_some()
                || series.histogram2.is_some()
                || series.contour_fill.is_some()
                || series.surface.is_some()
        })
    {
        return;
    }
    let (lower, upper) = effective_caxis(axes);

    let slices: usize = 24;
    for slice in 0..slices {
        let fraction = slice as f64 / (slices.saturating_sub(1) as f64);
        let fill = sample_colormap(axes.colormap, 1.0 - fraction);
        let y = frame.top + slice as f64 * (frame.height / slices as f64);
        out.push_str(&format!(
            "  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"{}\" stroke=\"none\"/>\n",
            format_number(frame.left),
            format_number(y),
            format_number(frame.width),
            format_number(frame.height / slices as f64 + 0.2),
            fill
        ));
    }
    out.push_str(&format!(
        "  <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\" fill=\"none\" stroke=\"#777777\" stroke-width=\"0.8\"/>\n",
        format_number(frame.left),
        format_number(frame.top),
        format_number(frame.width),
        format_number(frame.height)
    ));
    out.push_str(&format!(
        "  <text x=\"{}\" y=\"{}\" font-size=\"10\" fill=\"#444444\">{}</text>\n",
        format_number(frame.right() + 6.0),
        format_number(frame.top + 4.0),
        svg_escape(&format_number(upper))
    ));
    out.push_str(&format!(
        "  <text x=\"{}\" y=\"{}\" font-size=\"10\" fill=\"#444444\">{}</text>\n",
        format_number(frame.right() + 6.0),
        format_number(frame.bottom()),
        svg_escape(&format_number(lower))
    ));
}

fn colormapped_display_range(axes: &AxesState) -> Option<(f64, f64)> {
    axes.series
        .iter()
        .filter(|series| series.visible)
        .find_map(|series| {
            if let Some(image) = &series.image {
                image.rgb_values.as_ref().map(|_| None).unwrap_or_else(|| {
                    Some(match image.mapping {
                        ImageMapping::Scaled => image.display_range,
                        ImageMapping::Direct => (1.0, colormap_palette(axes.colormap).len() as f64),
                    })
                })
            } else if let Some(scatter) = &series.scatter {
                match &scatter.colors {
                    ScatterColors::Colormapped(values) => Some(finite_min_max(values)),
                    ScatterColors::Uniform(_) | ScatterColors::Rgb(_) => None,
                }
            } else if let Some(histogram2) = &series.histogram2 {
                Some(histogram2.count_range)
            } else if let Some(contour_fill) = &series.contour_fill {
                Some(contour_fill.level_range)
            } else {
                series.surface.as_ref().map(|surface| surface.z_range)
            }
        })
}

fn effective_caxis(axes: &AxesState) -> (f64, f64) {
    axes.caxis
        .or_else(|| colormapped_display_range(axes))
        .unwrap_or((0.0, 1.0))
}

fn image_coordinate_edges(data: &[f64]) -> Vec<f64> {
    match data.len() {
        0 => vec![0.0, 1.0],
        1 => vec![data[0] - 0.5, data[0] + 0.5],
        _ => {
            let mut edges = Vec::with_capacity(data.len() + 1);
            edges.push(data[0] - (data[1] - data[0]) / 2.0);
            for pair in data.windows(2) {
                edges.push((pair[0] + pair[1]) / 2.0);
            }
            edges.push(data[data.len() - 1] + (data[data.len() - 1] - data[data.len() - 2]) / 2.0);
            edges
        }
    }
}

fn image_display_limits(image: &ImageSeriesData) -> ((f64, f64), (f64, f64)) {
    let x_edges = image_coordinate_edges(&image.x_data);
    let y_edges = image_coordinate_edges(&image.y_data);
    (
        (
            x_edges.first().copied().unwrap_or(0.5),
            x_edges.last().copied().unwrap_or(image.cols as f64 + 0.5),
        ),
        (
            y_edges.first().copied().unwrap_or(0.5),
            y_edges.last().copied().unwrap_or(image.rows as f64 + 0.5),
        ),
    )
}

fn render_legend_sample(out: &mut String, series: &PlotSeries, x: f64, y: f64) {
    match series.kind {
        SeriesKind::Line
        | SeriesKind::ReferenceLine
        | SeriesKind::Line3D
        | SeriesKind::ErrorBar
        | SeriesKind::Stem3D
        | SeriesKind::Contour
        | SeriesKind::Contour3 => {
            out.push_str(&format!(
                "  <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"2.5\"/>\n",
                format_number(x),
                format_number(y),
                format_number(x + 20.0),
                format_number(y),
                series.color
            ));
        }
        SeriesKind::Quiver | SeriesKind::Quiver3D => {
            render_quiver_arrow(out, series.color, x + 2.0, y + 3.0, x + 20.0, y - 3.0);
        }
        SeriesKind::Pie | SeriesKind::Pie3 => {
            out.push_str(&format!(
                "  <circle cx=\"{}\" cy=\"{}\" r=\"7\" fill=\"{}\" fill-opacity=\"0.88\" stroke=\"white\" stroke-width=\"1\"/>\n",
                format_number(x + 10.0),
                format_number(y),
                SERIES_COLORS[0]
            ));
        }
        SeriesKind::Histogram => {
            out.push_str(&format!(
                "  <rect x=\"{}\" y=\"{}\" width=\"16\" height=\"12\" fill=\"{}\" fill-opacity=\"0.75\" stroke=\"{}\" stroke-width=\"1\"/>\n",
                format_number(x + 2.0),
                format_number(y - 6.0),
                series.color,
                series.color
            ));
        }
        SeriesKind::Histogram2 => {
            out.push_str(&format!(
                "  <rect x=\"{}\" y=\"{}\" width=\"16\" height=\"12\" fill=\"{}\" fill-opacity=\"0.82\" stroke=\"#666666\" stroke-width=\"0.8\"/>\n",
                format_number(x + 2.0),
                format_number(y - 6.0),
                sample_colormap(ColormapKind::Parula, 0.75)
            ));
        }
        SeriesKind::Area => {
            out.push_str(&format!(
                "  <rect x=\"{}\" y=\"{}\" width=\"16\" height=\"12\" fill=\"{}\" fill-opacity=\"0.3\" stroke=\"{}\" stroke-width=\"1.1\"/>\n",
                format_number(x + 2.0),
                format_number(y - 6.0),
                series.color,
                series.color
            ));
        }
        SeriesKind::Stairs => {
            out.push_str(&format!(
                "  <polyline fill=\"none\" stroke=\"{}\" stroke-width=\"2.2\" points=\"{},{} {},{} {},{}\"/>\n",
                series.color,
                format_number(x + 2.0),
                format_number(y + 4.0),
                format_number(x + 10.0),
                format_number(y + 4.0),
                format_number(x + 10.0),
                format_number(y - 5.0)
            ));
        }
        SeriesKind::Scatter | SeriesKind::Scatter3D => {
            out.push_str(&format!(
                "  <circle cx=\"{}\" cy=\"{}\" r=\"5\" fill=\"{}\" fill-opacity=\"0.85\"/>\n",
                format_number(x + 10.0),
                format_number(y),
                series.color
            ));
        }
        SeriesKind::Bar => {
            out.push_str(&format!(
                "  <rect x=\"{}\" y=\"{}\" width=\"16\" height=\"12\" fill=\"{}\" fill-opacity=\"0.75\" stroke=\"{}\" stroke-width=\"1\"/>\n",
                format_number(x + 2.0),
                format_number(y - 6.0),
                series.color,
                series.color
            ));
        }
        SeriesKind::BarHorizontal => {
            out.push_str(&format!(
                "  <rect x=\"{}\" y=\"{}\" width=\"16\" height=\"12\" fill=\"{}\" fill-opacity=\"0.75\" stroke=\"{}\" stroke-width=\"1\"/>\n",
                format_number(x + 2.0),
                format_number(y - 6.0),
                series.color,
                series.color
            ));
        }
        SeriesKind::Stem => {
            out.push_str(&format!(
                "  <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"1.8\"/>\n",
                format_number(x + 10.0),
                format_number(y + 5.0),
                format_number(x + 10.0),
                format_number(y - 6.0),
                series.color
            ));
            out.push_str(&format!(
                "  <circle cx=\"{}\" cy=\"{}\" r=\"4\" fill=\"{}\"/>\n",
                format_number(x + 10.0),
                format_number(y - 6.0),
                series.color
            ));
        }
        SeriesKind::ContourFill => {
            out.push_str(&format!(
                "  <rect x=\"{}\" y=\"{}\" width=\"16\" height=\"12\" fill=\"{}\" stroke=\"none\"/>\n",
                format_number(x + 2.0),
                format_number(y - 6.0),
                sample_colormap(ColormapKind::Parula, 0.62)
            ));
        }
        SeriesKind::Mesh => {
            out.push_str(&format!(
                "  <polyline fill=\"none\" stroke=\"{}\" stroke-width=\"1.6\" points=\"{},{} {},{} {},{}\"/>\n",
                sample_colormap(ColormapKind::Parula, 0.6),
                format_number(x + 2.0),
                format_number(y + 5.0),
                format_number(x + 10.0),
                format_number(y - 2.0),
                format_number(x + 18.0),
                format_number(y + 4.0)
            ));
        }
        SeriesKind::Waterfall => {
            out.push_str(&format!(
                "  <polyline fill=\"none\" stroke=\"{}\" stroke-width=\"1.8\" points=\"{},{} {},{} {},{}\"/>\n",
                series.color,
                format_number(x + 2.0),
                format_number(y + 4.0),
                format_number(x + 10.0),
                format_number(y - 3.0),
                format_number(x + 18.0),
                format_number(y + 2.0)
            ));
        }
        SeriesKind::Ribbon => {
            out.push_str(&format!(
                "  <rect x=\"{}\" y=\"{}\" width=\"16\" height=\"12\" fill=\"{}\" fill-opacity=\"0.88\" stroke=\"#555555\" stroke-width=\"0.8\"/>\n",
                format_number(x + 2.0),
                format_number(y - 6.0),
                sample_colormap(ColormapKind::Parula, 0.58)
            ));
        }
        SeriesKind::Surface => {
            out.push_str(&format!(
                "  <rect x=\"{}\" y=\"{}\" width=\"16\" height=\"12\" fill=\"{}\" stroke=\"#555555\" stroke-width=\"0.8\"/>\n",
                format_number(x + 2.0),
                format_number(y - 6.0),
                sample_colormap(ColormapKind::Parula, 0.68)
            ));
        }
        SeriesKind::Image => {
            out.push_str(&format!(
                "  <rect x=\"{}\" y=\"{}\" width=\"16\" height=\"12\" fill=\"{}\" stroke=\"#666666\" stroke-width=\"0.8\"/>\n",
                format_number(x + 2.0),
                format_number(y - 6.0),
                sample_colormap(ColormapKind::Parula, 0.65)
            ));
        }
        SeriesKind::Text => {
            out.push_str(&format!(
                "  <text x=\"{}\" y=\"{}\" font-size=\"12\" font-family=\"Segoe UI, Arial, sans-serif\" fill=\"{}\">T</text>\n",
                format_number(x + 5.0),
                format_number(y + 4.0),
                series.color
            ));
        }
        SeriesKind::Rectangle => {
            let fill = series
                .rectangle
                .as_ref()
                .and_then(|rectangle| rectangle.face_color)
                .unwrap_or("none");
            out.push_str(&format!(
                "  <rect x=\"{}\" y=\"{}\" width=\"16\" height=\"12\" fill=\"{}\" stroke=\"{}\" stroke-width=\"1\"/>\n",
                format_number(x + 2.0),
                format_number(y - 6.0),
                fill,
                series.color
            ));
        }
        SeriesKind::Patch => {
            let fill = series
                .patch
                .as_ref()
                .and_then(|patch| patch.face_color)
                .unwrap_or("none");
            out.push_str(&format!(
                "  <rect x=\"{}\" y=\"{}\" width=\"16\" height=\"12\" fill=\"{}\" stroke=\"{}\" stroke-width=\"1\"/>\n",
                format_number(x + 2.0),
                format_number(y - 6.0),
                fill,
                series.color
            ));
        }
    }
}

fn render_tick_text_svg(out: &mut String, x: f64, y: f64, anchor: &str, label: &str, angle: f64) {
    render_tick_text_svg_with_class(out, x, y, anchor, label, angle, None);
}

fn render_tick_text_svg_with_class(
    out: &mut String,
    x: f64,
    y: f64,
    anchor: &str,
    label: &str,
    angle: f64,
    class_name: Option<&str>,
) {
    let transform = if angle.abs() > f64::EPSILON {
        format!(
            " transform=\"rotate({} {} {})\"",
            format_number(-angle),
            format_number(x),
            format_number(y)
        )
    } else {
        String::new()
    };
    let class_attr = class_name
        .map(|class_name| format!(" class=\"{class_name}\""))
        .unwrap_or_default();

    out.push_str(&format!(
        "  <text{} x=\"{}\" y=\"{}\"{} text-anchor=\"{}\" font-size=\"11\" fill=\"#444444\">{}</text>\n",
        class_attr,
        format_number(x),
        format_number(y),
        transform,
        anchor,
        svg_escape(label)
    ));
}

fn stair_points(x: &[f64], y: &[f64]) -> Vec<(f64, f64)> {
    if x.is_empty() {
        return Vec::new();
    }
    if x.len() == 1 {
        return vec![(x[0], y[0])];
    }

    let mut points = Vec::with_capacity(x.len() * 2 - 1);
    points.push((x[0], y[0]));
    for index in 1..x.len() {
        points.push((x[index], y[index - 1]));
        points.push((x[index], y[index]));
    }
    points
}

fn area_polygon_points(x: &[f64], y: &[f64]) -> Vec<(f64, f64)> {
    if x.is_empty() {
        return Vec::new();
    }

    let mut points = Vec::with_capacity(x.len() + 2);
    points.push((x[0], 0.0));
    for (&x_value, &y_value) in x.iter().zip(y) {
        points.push((x_value, y_value));
    }
    points.push((x[x.len() - 1], 0.0));
    points
}

fn format_number(value: f64) -> String {
    let rounded = if value.abs() < 1e-9 { 0.0 } else { value };
    if rounded.fract().abs() < 1e-9 {
        format!("{}", rounded as i64)
    } else {
        format!("{rounded:.4}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    }
}

fn svg_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LabelKind {
    Title,
    Subtitle,
    XLabel,
    YLabel,
    ZLabel,
}

impl LabelKind {
    fn builtin_name(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Subtitle => "subtitle",
            Self::XLabel => "xlabel",
            Self::YLabel => "ylabel",
            Self::ZLabel => "zlabel",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScaleKind {
    X,
    Y,
}

impl ScaleKind {
    fn builtin_name(self) -> &'static str {
        match self {
            Self::X => "xscale",
            Self::Y => "yscale",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LimitKind {
    X,
    Y,
    Z,
}

impl LimitKind {
    fn builtin_name(self) -> &'static str {
        match self {
            Self::X => "xlim",
            Self::Y => "ylim",
            Self::Z => "zlim",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TickKind {
    X,
    Y,
    Z,
}

impl TickKind {
    fn axis_name(self) -> &'static str {
        match self {
            Self::X => "X",
            Self::Y => "Y",
            Self::Z => "Z",
        }
    }

    fn builtin_name(self) -> &'static str {
        match self {
            Self::X => "xticks",
            Self::Y => "yticks",
            Self::Z => "zticks",
        }
    }

    fn labels_builtin_name(self) -> &'static str {
        match self {
            Self::X => "xticklabels",
            Self::Y => "yticklabels",
            Self::Z => "zticklabels",
        }
    }

    fn angle_builtin_name(self) -> &'static str {
        match self {
            Self::X => "xtickangle",
            Self::Y => "ytickangle",
            Self::Z => "ztickangle",
        }
    }
}
