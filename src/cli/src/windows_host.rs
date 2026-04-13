use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    fs,
    mem::size_of,
    path::{Path, PathBuf},
    process::{self, Stdio},
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use tao::{
    dpi::{LogicalPosition, LogicalSize, PhysicalPosition},
    event::{Event, StartCause, WindowEvent},
    event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy, EventLoopWindowTarget},
    platform::windows::WindowExtWindows,
    window::{Window, WindowBuilder, WindowId},
};
use windows::{
    core::{PCWSTR, PWSTR},
    Win32::{
        Foundation::HWND,
        UI::Controls::Dialogs::{
            GetSaveFileNameW, OFN_EXPLORER, OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT,
            OFN_PATHMUSTEXIST, OPENFILENAMEW,
        },
    },
};
use wry::{WebView, WebViewBuilder};

const POLL_INTERVAL: Duration = Duration::from_millis(250);
const POSITION_WRITE_DEBOUNCE: Duration = Duration::from_millis(150);
const HOST_STARTUP_TIMEOUT: Duration = Duration::from_millis(2500);
const HOST_STATUS_FILE: &str = "host-status.json";
const DOCK_WINDOW_TITLE: &str = "MATC Figure Viewer - Docked Figures";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct HostLaunchState {
    state: String,
    #[serde(default)]
    message: String,
}

impl HostLaunchState {
    fn starting() -> Self {
        Self {
            state: "starting".to_string(),
            message: String::new(),
        }
    }

    fn ready() -> Self {
        Self {
            state: "ready".to_string(),
            message: String::new(),
        }
    }

    fn error(message: impl Into<String>) -> Self {
        Self {
            state: "error".to_string(),
            message: message.into(),
        }
    }

    fn is_ready(&self) -> bool {
        self.state == "ready"
    }

    fn is_error(&self) -> bool {
        self.state == "error"
    }
}

#[derive(Debug, Clone, Deserialize)]
struct SessionManifest {
    title: String,
    revision: u128,
    #[serde(default)]
    figures: Vec<FigureManifest>,
}

#[derive(Debug, Clone, Deserialize)]
struct FigureManifest {
    handle: u32,
    title: String,
    visible: bool,
    #[serde(rename = "window_style")]
    window_style: String,
    position: [f64; 4],
    page: String,
    #[serde(default)]
    browser_page: String,
    #[serde(default, rename = "host_page")]
    host_page: String,
    #[serde(rename = "svg")]
    svg: String,
}

impl FigureManifest {
    fn browser_page_name(&self) -> &str {
        if self.browser_page.is_empty() {
            &self.page
        } else {
            &self.browser_page
        }
    }

    fn host_page_name(&self) -> &str {
        if self.host_page.is_empty() {
            self.browser_page_name()
        } else {
            &self.host_page
        }
    }

    fn is_docked(&self) -> bool {
        self.window_style.eq_ignore_ascii_case("docked")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum HostWindowKey {
    Figure(u32),
    Dock,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostCommand {
    SaveSvg,
    OpenInBrowser,
    Refresh,
    Close,
    About,
    Pan,
    Rotate,
    Brush,
    ClearBrush,
    DataTips,
    ClearTips,
    ZoomIn,
    ZoomOut,
    ResetView,
}

impl HostCommand {
    fn parse(raw: &str) -> Option<Self> {
        match raw {
            "save-svg" => Some(Self::SaveSvg),
            "open-browser" => Some(Self::OpenInBrowser),
            "refresh" => Some(Self::Refresh),
            "close" => Some(Self::Close),
            "about" => Some(Self::About),
            "pan" => Some(Self::Pan),
            "rotate" => Some(Self::Rotate),
            "brush" => Some(Self::Brush),
            "clear-brush" => Some(Self::ClearBrush),
            "datatips" => Some(Self::DataTips),
            "clear-tips" => Some(Self::ClearTips),
            "zoom-in" => Some(Self::ZoomIn),
            "zoom-out" => Some(Self::ZoomOut),
            "reset" => Some(Self::ResetView),
            _ => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::SaveSvg => "Save SVG",
            Self::OpenInBrowser => "Open in Browser",
            Self::Refresh => "Refresh",
            Self::Close => "Close",
            Self::About => "About",
            Self::Pan => "Pan",
            Self::Rotate => "Rotate",
            Self::Brush => "Brush",
            Self::ClearBrush => "Clear Brush",
            Self::DataTips => "Data Tips",
            Self::ClearTips => "Clear Tips",
            Self::ZoomIn => "Zoom In",
            Self::ZoomOut => "Zoom Out",
            Self::ResetView => "Reset View",
        }
    }

    fn js_function(self) -> Option<&'static str> {
        match self {
            Self::Pan => Some("matcToggleActiveFigurePanMode"),
            Self::Rotate => Some("matcToggleActiveFigureRotateMode"),
            Self::Brush => Some("matcToggleActiveFigureBrushMode"),
            Self::ClearBrush => Some("matcClearActiveFigureBrush"),
            Self::DataTips => Some("matcToggleActiveFigureDataTips"),
            Self::ClearTips => Some("matcClearActiveFigureDataTips"),
            Self::ZoomIn => Some("matcZoomInActiveFigure"),
            Self::ZoomOut => Some("matcZoomOutActiveFigure"),
            Self::ResetView => Some("matcResetActiveFigure"),
            _ => None,
        }
    }
}

#[allow(dead_code)]
#[derive(Debug, Deserialize)]
struct ToolbarCommandEnvelope {
    #[serde(default)]
    r#type: String,
    #[serde(default)]
    command: String,
}

#[derive(Debug, Clone, Deserialize)]
struct ContentStatePayload {
    #[serde(rename = "type", default)]
    message_type: String,
    #[serde(rename = "activeHandle")]
    active_handle: Option<u32>,
    #[serde(default)]
    readout: String,
    #[serde(default)]
    mode: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
struct CommandResultPayload {
    status: String,
    #[serde(default)]
    message: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Serialize)]
struct ToolbarStatePayload {
    title: String,
    status: String,
    active_handle: Option<u32>,
    mode: String,
}

enum HostUserEvent {
    #[allow(dead_code)]
    ToolbarCommand {
        key: HostWindowKey,
        command: HostCommand,
    },
    ContentState {
        key: HostWindowKey,
        payload: ContentStatePayload,
    },
    #[allow(dead_code)]
    CommandResult {
        key: HostWindowKey,
        label: &'static str,
        payload: CommandResultPayload,
    },
}

struct HostWindow {
    toolbar: Option<WebView>,
    content: WebView,
    window: Window,
    content_page: String,
    active_handle: Option<u32>,
    readout: String,
    mode: String,
    managed_handles: BTreeSet<u32>,
    position_owner: Option<u32>,
    pending_position: Option<(Instant, [f64; 4])>,
}

struct WebViewFigureHost {
    session_dir: PathBuf,
    status_path: PathBuf,
    proxy: EventLoopProxy<HostUserEvent>,
    windows: BTreeMap<HostWindowKey, HostWindow>,
    window_ids: HashMap<WindowId, HostWindowKey>,
    current_figures: BTreeMap<u32, FigureManifest>,
    session_title: String,
    last_revision: Option<u128>,
    last_session_miss: u32,
    saw_session: bool,
    ready_sent: bool,
    locally_closed_at_revision: BTreeMap<u32, u128>,
    failed_keys_at_revision: BTreeMap<HostWindowKey, u128>,
}

pub fn launch_internal_host(session_dir: &Path, fallback_path: &Path) -> bool {
    let status_path = host_status_path(session_dir);
    if write_host_status(&status_path, &HostLaunchState::starting()).is_err() {
        return false;
    }
    let mut child = match spawn_internal_host_process(session_dir, fallback_path) {
        Some(child) => child,
        None => {
            let _ = write_host_status(
                &status_path,
                &HostLaunchState::error("failed to spawn figure host"),
            );
            return false;
        }
    };
    let launched = wait_for_host_ready(&status_path, HOST_STARTUP_TIMEOUT, &mut child);
    if launched && read_host_status(&status_path).is_none_or(|state| !state.is_ready()) {
        let _ = write_host_status(&status_path, &HostLaunchState::ready());
    }
    launched
}

fn spawn_internal_host_process(session_dir: &Path, fallback_path: &Path) -> Option<process::Child> {
    let Ok(exe) = env_current_exe() else {
        return None;
    };
    let mut command = process::Command::new(&exe);
    command
        .arg("__figure-host")
        .arg(session_dir)
        .arg(fallback_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command.spawn().ok()
}

pub fn run_internal_host(session_dir: PathBuf, _fallback_path: PathBuf) -> Result<(), String> {
    let mut builder = EventLoopBuilder::<HostUserEvent>::with_user_event();
    let event_loop = builder.build();
    let proxy = event_loop.create_proxy();
    let status_path = host_status_path(&session_dir);
    let mut host = WebViewFigureHost::new(session_dir, status_path, proxy.clone());
    event_loop.run(move |event, target, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + POLL_INTERVAL);
        match event {
            Event::NewEvents(StartCause::Init) | Event::MainEventsCleared => {
                if let Err(error) = host.poll(target) {
                    eprintln!("{error}");
                    host.mark_error(&error);
                }
                if let Err(error) = host.flush_pending_position_writes() {
                    eprintln!("{error}");
                    host.mark_error(&error);
                }
                if host.should_exit() {
                    *control_flow = ControlFlow::Exit;
                }
            }
            Event::UserEvent(user_event) => {
                if let Err(error) = host.handle_user_event(user_event) {
                    eprintln!("{error}");
                    host.mark_error(&error);
                }
            }
            Event::WindowEvent {
                window_id, event, ..
            } => {
                if let Err(error) = host.handle_window_event(window_id, event) {
                    eprintln!("{error}");
                    host.mark_error(&error);
                }
            }
            _ => {}
        }
    });
    #[allow(unreachable_code)]
    Ok(())
}

impl WebViewFigureHost {
    fn new(
        session_dir: PathBuf,
        status_path: PathBuf,
        proxy: EventLoopProxy<HostUserEvent>,
    ) -> Self {
        Self {
            session_dir,
            status_path,
            proxy,
            windows: BTreeMap::new(),
            window_ids: HashMap::new(),
            current_figures: BTreeMap::new(),
            session_title: "MATC Figure Viewer".to_string(),
            last_revision: None,
            last_session_miss: 0,
            saw_session: false,
            ready_sent: false,
            locally_closed_at_revision: BTreeMap::new(),
            failed_keys_at_revision: BTreeMap::new(),
        }
    }

    fn should_exit(&self) -> bool {
        if !self.windows.is_empty() {
            return false;
        }
        if self.last_session_miss > 8 {
            return true;
        }
        let Some(revision) = self.last_revision else {
            return false;
        };
        let visible_figures = self
            .current_figures
            .values()
            .filter(|figure| figure.visible)
            .collect::<Vec<_>>();
        visible_figures.is_empty()
            || visible_figures.iter().all(|figure| {
                if figure.is_docked() {
                    self.should_suppress_key_for_revision(HostWindowKey::Dock, revision)
                } else {
                    self.should_suppress_handle_for_revision(figure.handle, revision)
                        || self.should_suppress_key_for_revision(
                            HostWindowKey::Figure(figure.handle),
                            revision,
                        )
                }
            })
    }

    fn mark_ready_if_needed(&mut self) {
        if self.ready_sent || self.windows.is_empty() {
            return;
        }
        let _ = write_host_status(&self.status_path, &HostLaunchState::ready());
        self.ready_sent = true;
    }

    fn mark_error(&mut self, message: &str) {
        if self.ready_sent {
            return;
        }
        let _ = write_host_status(&self.status_path, &HostLaunchState::error(message));
    }

    fn note_session_revision(&mut self, revision: u128) {
        self.last_revision = Some(revision);
        self.locally_closed_at_revision
            .retain(|_, closed_revision| *closed_revision >= revision);
        self.failed_keys_at_revision
            .retain(|_, failed_revision| *failed_revision >= revision);
    }

    fn note_local_close(&mut self, handle: u32) {
        self.locally_closed_at_revision
            .insert(handle, self.last_revision.unwrap_or_default());
    }

    fn should_suppress_handle_for_revision(&self, handle: u32, revision: u128) -> bool {
        self.locally_closed_at_revision
            .get(&handle)
            .is_some_and(|closed_revision| *closed_revision >= revision)
    }

    fn should_suppress_key_for_revision(&self, key: HostWindowKey, revision: u128) -> bool {
        self.failed_keys_at_revision
            .get(&key)
            .is_some_and(|failed_revision| *failed_revision >= revision)
    }

    fn note_failed_key(&mut self, key: HostWindowKey) {
        self.failed_keys_at_revision
            .insert(key, self.last_revision.unwrap_or_default());
    }

    fn poll(&mut self, target: &EventLoopWindowTarget<HostUserEvent>) -> Result<(), String> {
        let Some(session) = self.read_session()? else {
            self.last_session_miss = self.last_session_miss.saturating_add(1);
            return Ok(());
        };

        self.saw_session = true;
        self.last_session_miss = 0;
        let revision_changed = self.last_revision != Some(session.revision);
        self.session_title = session.title.clone();
        self.current_figures = session
            .figures
            .iter()
            .cloned()
            .map(|figure| (figure.handle, figure))
            .collect();
        self.note_session_revision(session.revision);

        let visible_normal = session
            .figures
            .iter()
            .filter(|figure| figure.visible && !figure.is_docked())
            .cloned()
            .collect::<Vec<_>>();
        let visible_docked = session
            .figures
            .iter()
            .filter(|figure| figure.visible && figure.is_docked())
            .cloned()
            .collect::<Vec<_>>();

        let mut wanted = visible_normal
            .iter()
            .map(|figure| HostWindowKey::Figure(figure.handle))
            .collect::<BTreeSet<_>>();
        if !visible_docked.is_empty() {
            wanted.insert(HostWindowKey::Dock);
        }

        let existing_keys = self.windows.keys().copied().collect::<Vec<_>>();
        for key in existing_keys {
            if !wanted.contains(&key) {
                self.remove_window(key);
                if let HostWindowKey::Figure(handle) = key {
                    self.locally_closed_at_revision.remove(&handle);
                }
            }
        }

        for figure in visible_normal {
            if self.should_suppress_handle_for_revision(figure.handle, session.revision) {
                continue;
            }
            self.sync_normal_window(target, figure, revision_changed)?;
        }

        self.sync_dock_window(target, &visible_docked, revision_changed)?;
        self.mark_ready_if_needed();
        Ok(())
    }

    fn sync_normal_window(
        &mut self,
        target: &EventLoopWindowTarget<HostUserEvent>,
        figure: FigureManifest,
        revision_changed: bool,
    ) -> Result<(), String> {
        let key = HostWindowKey::Figure(figure.handle);
        if self.should_suppress_key_for_revision(key, self.last_revision.unwrap_or_default()) {
            return Ok(());
        }
        if let Some(window) = self.windows.get_mut(&key) {
            window.managed_handles.clear();
            window.managed_handles.insert(figure.handle);
            window.position_owner = Some(figure.handle);
            window.active_handle = Some(figure.handle);
            window.window.set_title(&figure.title);
            if revision_changed {
                apply_window_bounds(&window.window, figure.position);
            }
            let next_content_page = figure.host_page_name().to_string();
            if revision_changed || window.content_page != next_content_page {
                window.content_page = next_content_page;
                let html = read_host_page_html(&self.session_dir, &window.content_page)?;
                let _ = window.content.load_html(&html);
            }
            self.sync_toolbar_state(key)?;
            return Ok(());
        }

        let host_window = match self.create_window_shell(
            target,
            key,
            &figure.title,
            figure.position,
            figure.host_page_name(),
            Some(figure.handle),
            [figure.handle].into_iter().collect(),
        ) {
            Ok(window) => window,
            Err(error) => {
                self.note_failed_key(key);
                return Err(error);
            }
        };
        self.insert_window(key, host_window);
        self.failed_keys_at_revision.remove(&key);
        self.sync_toolbar_state(key)?;
        Ok(())
    }

    fn sync_dock_window(
        &mut self,
        target: &EventLoopWindowTarget<HostUserEvent>,
        figures: &[FigureManifest],
        revision_changed: bool,
    ) -> Result<(), String> {
        let key = HostWindowKey::Dock;
        if figures.is_empty() {
            self.remove_window(key);
            self.failed_keys_at_revision.remove(&key);
            return Ok(());
        }
        if self.should_suppress_key_for_revision(key, self.last_revision.unwrap_or_default()) {
            return Ok(());
        }

        let managed_handles = figures
            .iter()
            .map(|figure| figure.handle)
            .collect::<BTreeSet<_>>();
        let dock_position = figures
            .first()
            .map(|figure| figure.position)
            .unwrap_or([80.0, 80.0, 1360.0, 960.0]);
        let dock_title = self.dock_window_title(None);

        if let Some(window) = self.windows.get_mut(&key) {
            window.managed_handles = managed_handles;
            if window
                .active_handle
                .is_none_or(|handle| !window.managed_handles.contains(&handle))
            {
                window.active_handle = window.managed_handles.iter().copied().next();
            }
            window.position_owner = None;
            window.window.set_title(&dock_title);
            if revision_changed {
                apply_window_bounds(&window.window, dock_position);
            }
            let next_content_page = "dock_index.html".to_string();
            if revision_changed || window.content_page != next_content_page {
                window.content_page = next_content_page;
                let html = read_host_page_html(&self.session_dir, &window.content_page)?;
                let _ = window.content.load_html(&html);
            }
            self.sync_toolbar_state(key)?;
            return Ok(());
        }

        let host_window = match self.create_window_shell(
            target,
            key,
            &dock_title,
            dock_position,
            "dock_index.html",
            None,
            managed_handles,
        ) {
            Ok(window) => window,
            Err(error) => {
                self.note_failed_key(key);
                return Err(error);
            }
        };
        self.insert_window(key, host_window);
        self.failed_keys_at_revision.remove(&key);
        self.sync_toolbar_state(key)?;
        Ok(())
    }

    fn create_window_shell(
        &mut self,
        target: &EventLoopWindowTarget<HostUserEvent>,
        key: HostWindowKey,
        title: &str,
        position: [f64; 4],
        content_page: &str,
        position_owner: Option<u32>,
        managed_handles: BTreeSet<u32>,
    ) -> Result<HostWindow, String> {
        let window = WindowBuilder::new()
            .with_title(title.to_string())
            .with_inner_size(LogicalSize::new(position[2], position[3]))
            .with_position(LogicalPosition::new(position[0], position[1]))
            .build(target)
            .map_err(|error| format!("failed to build figure window: {error}"))?;
        apply_window_bounds(&window, position);

        let content_proxy = self.proxy.clone();
        let content_html = read_host_page_html(&self.session_dir, content_page)?;
        let content = WebViewBuilder::new()
            .with_html(&content_html)
            .with_ipc_handler(move |request| {
                if let Some(payload) = parse_content_state(request.body()) {
                    let _ = content_proxy.send_event(HostUserEvent::ContentState { key, payload });
                }
            })
            .build(&window)
            .map_err(|error| format!("failed to build content WebView: {error}"))?;

        Ok(HostWindow {
            content,
            window,
            toolbar: None,
            content_page: content_page.to_string(),
            active_handle: managed_handles.iter().copied().next(),
            readout: "Loading figure viewer...".to_string(),
            mode: "inspect".to_string(),
            managed_handles,
            position_owner,
            pending_position: None,
        })
    }

    fn insert_window(&mut self, key: HostWindowKey, window: HostWindow) {
        self.window_ids.insert(window.window.id(), key);
        self.windows.insert(key, window);
    }

    fn remove_window(&mut self, key: HostWindowKey) {
        if let Some(window) = self.windows.remove(&key) {
            self.window_ids.remove(&window.window.id());
        }
    }

    fn handle_user_event(&mut self, event: HostUserEvent) -> Result<(), String> {
        match event {
            HostUserEvent::ToolbarCommand { key, command } => {
                self.handle_toolbar_command(key, command)
            }
            HostUserEvent::ContentState { key, payload } => self.handle_content_state(key, payload),
            HostUserEvent::CommandResult {
                key,
                label,
                payload,
            } => self.handle_command_result(key, label, payload),
        }
    }

    fn handle_toolbar_command(
        &mut self,
        key: HostWindowKey,
        command: HostCommand,
    ) -> Result<(), String> {
        match command {
            HostCommand::SaveSvg => self.save_active_figure_svg(key),
            HostCommand::OpenInBrowser => self.open_active_figure_in_browser(key),
            HostCommand::Refresh => self.refresh_window_content(key),
            HostCommand::Close => self.close_active_window(key),
            HostCommand::About => {
                self.set_status_text(key, "MATC Figure Viewer | WebView2 host".to_string())
            }
            content_command => self.dispatch_content_command(key, content_command),
        }
    }

    fn handle_content_state(
        &mut self,
        key: HostWindowKey,
        payload: ContentStatePayload,
    ) -> Result<(), String> {
        if payload.message_type != "matc-host-state" {
            return Ok(());
        }
        let dock_title = if key == HostWindowKey::Dock {
            Some(self.dock_window_title(payload.active_handle))
        } else {
            None
        };
        if let Some(window) = self.windows.get_mut(&key) {
            if payload.active_handle.is_some() {
                window.active_handle = payload.active_handle;
            }
            if !payload.readout.is_empty() {
                window.readout = payload.readout;
            }
            if !payload.mode.is_empty() {
                window.mode = payload.mode;
            }
            if let Some(title) = dock_title {
                window.window.set_title(&title);
            }
        }
        self.sync_toolbar_state(key)
    }

    fn handle_command_result(
        &mut self,
        key: HostWindowKey,
        label: &'static str,
        payload: CommandResultPayload,
    ) -> Result<(), String> {
        if payload.status == "ok" {
            return Ok(());
        }
        let suffix = if payload.message.is_empty() {
            payload.status
        } else {
            format!("{} | {}", payload.status, payload.message)
        };
        self.set_status_text(key, format!("{label} | {suffix}"))
    }

    fn handle_window_event(
        &mut self,
        window_id: WindowId,
        event: WindowEvent<'_>,
    ) -> Result<(), String> {
        let Some(key) = self.window_ids.get(&window_id).copied() else {
            return Ok(());
        };
        match event {
            WindowEvent::CloseRequested => self.handle_close_requested(key)?,
            WindowEvent::Moved(position) => {
                if let Some(window) = self.windows.get_mut(&key) {
                    if window.position_owner.is_some() {
                        let size = window.window.outer_size();
                        window.pending_position = Some((
                            Instant::now(),
                            [
                                position.x as f64,
                                position.y as f64,
                                size.width as f64,
                                size.height as f64,
                            ],
                        ));
                    }
                }
            }
            WindowEvent::Resized(size) => {
                if let Some(window) = self.windows.get_mut(&key) {
                    if window.position_owner.is_some() {
                        let position = window
                            .window
                            .outer_position()
                            .unwrap_or(PhysicalPosition::new(0, 0));
                        window.pending_position = Some((
                            Instant::now(),
                            [
                                position.x as f64,
                                position.y as f64,
                                size.width as f64,
                                size.height as f64,
                            ],
                        ));
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_close_requested(&mut self, key: HostWindowKey) -> Result<(), String> {
        let handles = self
            .windows
            .get(&key)
            .map(|window| window.managed_handles.iter().copied().collect::<Vec<_>>())
            .unwrap_or_default();
        for handle in handles {
            self.write_close_event(handle)?;
            self.note_local_close(handle);
        }
        self.remove_window(key);
        Ok(())
    }

    fn flush_pending_position_writes(&mut self) -> Result<(), String> {
        let now = Instant::now();
        let pending = self
            .windows
            .iter()
            .filter_map(|(key, window)| {
                let handle = window.position_owner?;
                window
                    .pending_position
                    .filter(|(instant, _)| now.duration_since(*instant) >= POSITION_WRITE_DEBOUNCE)
                    .map(|(_, position)| (*key, handle, position))
            })
            .collect::<Vec<_>>();

        for (key, handle, position) in pending {
            self.write_position_event(handle, position)?;
            if let Some(window) = self.windows.get_mut(&key) {
                window.pending_position = None;
            }
        }
        Ok(())
    }

    fn dispatch_content_command(
        &mut self,
        key: HostWindowKey,
        command: HostCommand,
    ) -> Result<(), String> {
        let Some(js_function) = command.js_function() else {
            return Ok(());
        };
        let Some(window) = self.windows.get(&key) else {
            return Ok(());
        };
        let proxy = self.proxy.clone();
        let label = command.label();
        let script = format!(
            "(function(){{try{{var fn=window['{js_function}'];if(typeof fn==='function'){{fn();return {{status:'ok'}};}}return {{status:'missing',message:'viewer function not found'}};}}catch(error){{return {{status:'error',message:String(error)}};}}}})()"
        );
        window
            .content
            .evaluate_script_with_callback(&script, move |raw| {
                let payload = serde_json::from_str::<CommandResultPayload>(&raw).unwrap_or(
                    CommandResultPayload {
                        status: "error".to_string(),
                        message: raw,
                    },
                );
                let _ = proxy.send_event(HostUserEvent::CommandResult {
                    key,
                    label,
                    payload,
                });
            })
            .map_err(|error| {
                format!(
                    "failed to dispatch viewer command `{}`: {error}",
                    command.label()
                )
            })
    }

    fn refresh_window_content(&mut self, key: HostWindowKey) -> Result<(), String> {
        let Some(window) = self.windows.get_mut(&key) else {
            return Ok(());
        };
        let html = read_host_page_html(&self.session_dir, &window.content_page)?;
        window
            .content
            .load_html(&html)
            .map_err(|error| format!("failed to refresh figure page: {error}"))?;
        self.set_status_text(key, "Refreshing figure view...".to_string())
    }

    fn close_active_window(&mut self, key: HostWindowKey) -> Result<(), String> {
        match key {
            HostWindowKey::Figure(handle) => {
                self.write_close_event(handle)?;
                self.note_local_close(handle);
                self.remove_window(key);
                Ok(())
            }
            HostWindowKey::Dock => {
                let active = self
                    .windows
                    .get(&key)
                    .and_then(|window| window.active_handle)
                    .or_else(|| {
                        self.windows
                            .get(&key)
                            .and_then(|window| window.managed_handles.iter().copied().next())
                    });
                if let Some(handle) = active {
                    self.write_close_event(handle)?;
                    self.note_local_close(handle);
                    self.set_status_text(key, format!("Closed Figure {}", handle))?;
                    Ok(())
                } else {
                    self.remove_window(key);
                    Ok(())
                }
            }
        }
    }

    fn open_active_figure_in_browser(&mut self, key: HostWindowKey) -> Result<(), String> {
        let Some(handle) = self.resolve_active_handle(key) else {
            return self.set_status_text(key, "Open in Browser | no active figure".to_string());
        };
        let Some(figure) = self.current_figures.get(&handle) else {
            return self.set_status_text(key, "Open in Browser | figure not found".to_string());
        };
        let path = self.session_dir.join(figure.browser_page_name());
        open_browser_target(&path)?;
        self.set_status_text(key, format!("Open in Browser | Figure {}", handle))
    }

    fn save_active_figure_svg(&mut self, key: HostWindowKey) -> Result<(), String> {
        let Some(handle) = self.resolve_active_handle(key) else {
            return self.set_status_text(key, "Save SVG | no active figure".to_string());
        };
        let Some(figure) = self.current_figures.get(&handle) else {
            return self.set_status_text(key, "Save SVG | figure not found".to_string());
        };
        let source = self.session_dir.join(&figure.svg);
        if !source.exists() {
            return self.set_status_text(key, "Save SVG | source file missing".to_string());
        }
        let default_name = Path::new(&figure.svg)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("figure.svg");
        let owner = self
            .windows
            .get(&key)
            .ok_or_else(|| "missing host window".to_string())?;
        let Some(destination) = prompt_save_svg_path(&owner.window, default_name) else {
            return self.set_status_text(key, "Save SVG | cancelled".to_string());
        };
        fs::copy(&source, &destination).map_err(|error| {
            format!("failed to save SVG to `{}`: {error}", destination.display())
        })?;
        self.set_status_text(key, format!("Save SVG | {}", destination.display()))
    }

    fn resolve_active_handle(&self, key: HostWindowKey) -> Option<u32> {
        self.windows.get(&key).and_then(|window| {
            window
                .active_handle
                .or_else(|| window.managed_handles.iter().copied().next())
        })
    }

    fn set_status_text(&mut self, key: HostWindowKey, status: String) -> Result<(), String> {
        if let Some(window) = self.windows.get_mut(&key) {
            window.readout = status;
        }
        self.sync_toolbar_state(key)
    }

    fn sync_toolbar_state(&mut self, key: HostWindowKey) -> Result<(), String> {
        let has_toolbar = self
            .windows
            .get(&key)
            .and_then(|window| window.toolbar.as_ref())
            .is_some();
        if !has_toolbar {
            return Ok(());
        }
        let payload = self.toolbar_state_payload(key)?;
        let script = format!(
            "window.matcToolbarSetState({});",
            serde_json::to_string(&payload)
                .map_err(|error| format!("failed to serialize toolbar state: {error}"))?
        );
        if let Some(window) = self.windows.get(&key) {
            window
                .toolbar
                .as_ref()
                .expect("toolbar presence checked")
                .evaluate_script(&script)
                .map_err(|error| format!("failed to update toolbar state: {error}"))?;
        }
        Ok(())
    }

    fn toolbar_state_payload(&self, key: HostWindowKey) -> Result<ToolbarStatePayload, String> {
        let window = self
            .windows
            .get(&key)
            .ok_or_else(|| "missing host window".to_string())?;
        let title = match key {
            HostWindowKey::Figure(handle) => self
                .current_figures
                .get(&handle)
                .map(|figure| figure.title.clone())
                .unwrap_or_else(|| format!("Figure {}", handle)),
            HostWindowKey::Dock => self.dock_window_title(window.active_handle),
        };
        Ok(ToolbarStatePayload {
            title,
            status: window.readout.clone(),
            active_handle: window.active_handle,
            mode: window.mode.clone(),
        })
    }

    fn dock_window_title(&self, active_handle: Option<u32>) -> String {
        if let Some(handle) = active_handle {
            if let Some(figure) = self.current_figures.get(&handle) {
                return format!("{DOCK_WINDOW_TITLE}: {}", figure.title);
            }
        }
        DOCK_WINDOW_TITLE.to_string()
    }

    fn read_session(&self) -> Result<Option<SessionManifest>, String> {
        let path = self.session_dir.join("session.json");
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&path).map_err(|error| {
            format!(
                "failed to read session manifest `{}`: {error}",
                path.display()
            )
        })?;
        serde_json::from_str(&raw).map(Some).map_err(|error| {
            format!(
                "failed to parse session manifest `{}`: {error}",
                path.display()
            )
        })
    }

    fn write_close_event(&self, handle: u32) -> Result<(), String> {
        let path = self.session_dir.join(format!("event-close-{handle}.txt"));
        fs::write(&path, "1")
            .map_err(|error| format!("failed to write close event `{}`: {error}", path.display()))
    }

    fn write_position_event(&self, handle: u32, position: [f64; 4]) -> Result<(), String> {
        let path = self
            .session_dir
            .join(format!("event-position-{handle}.txt"));
        let text = format!(
            "{},{},{},{}",
            position[0], position[1], position[2], position[3]
        );
        fs::write(&path, text).map_err(|error| {
            format!(
                "failed to write position event `{}`: {error}",
                path.display()
            )
        })
    }
}

fn host_status_path(session_dir: &Path) -> PathBuf {
    session_dir.join(HOST_STATUS_FILE)
}

fn write_host_status(path: &Path, state: &HostLaunchState) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let text = serde_json::to_string(state)
        .map_err(|error| format!("failed to serialize host status: {error}"))?;
    fs::write(path, text)
        .map_err(|error| format!("failed to write host status `{}`: {error}", path.display()))
}

fn read_host_status(path: &Path) -> Option<HostLaunchState> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn wait_for_host_ready(path: &Path, timeout: Duration, child: &mut process::Child) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if let Some(state) = read_host_status(path) {
            if state.is_ready() {
                return true;
            }
            if state.is_error() {
                return false;
            }
        }
        if let Ok(Some(_status)) = child.try_wait() {
            return false;
        }
        thread::sleep(Duration::from_millis(50));
    }
    child
        .try_wait()
        .map(|status| status.is_none())
        .unwrap_or(false)
}

fn apply_window_bounds(window: &Window, position: [f64; 4]) {
    let size = LogicalSize::new(position[2].max(1.0), position[3].max(1.0));
    window.set_inner_size(size);
    window.set_outer_position(LogicalPosition::new(position[0], position[1]));
}

#[allow(dead_code)]
fn render_host_toolbar_html(title: &str) -> String {
    r#"<!doctype html>
<html>
<head>
<meta charset="utf-8">
<style>
*{box-sizing:border-box;}
html,body{margin:0;padding:0;background:#e8edf5;color:#15202c;font-family:Segoe UI,Arial,sans-serif;overflow:hidden;}
body{display:flex;flex-direction:column;height:100vh;border-bottom:1px solid #b9c5d4;}
.matc-host-head{display:flex;justify-content:space-between;align-items:center;padding:8px 12px 4px 12px;background:linear-gradient(180deg,#f7fafe 0%,#e4ebf5 100%);border-bottom:1px solid #c4cfdd;}
.matc-host-title{font-size:14px;font-weight:600;}
.matc-host-active{font-size:12px;color:#526273;}
.matc-host-row{display:flex;flex-wrap:wrap;gap:6px;padding:6px 10px;background:#edf2f9;border-bottom:1px solid #d0d8e3;}
.matc-host-row.nav{background:#eef3fa;}
button{border:1px solid #aeb9c9;border-radius:6px;padding:5px 10px;background:linear-gradient(180deg,#ffffff 0%,#e7edf6 100%);font-size:12px;font-weight:600;color:#15202c;cursor:pointer;}
button:hover{border-color:#8fa4bd;background:linear-gradient(180deg,#ffffff 0%,#dbe6f5 100%);}
button:active{transform:translateY(1px);}
button.mode-active{border-color:#4f89c7;background:linear-gradient(180deg,#ffffff 0%,#d6e7fb 100%);color:#143e67;}
.matc-host-status{display:flex;align-items:center;justify-content:space-between;gap:12px;padding:6px 10px;background:#f8fafc;color:#526273;font-size:12px;font-variant-numeric:tabular-nums;white-space:nowrap;overflow:hidden;}
.matc-host-status span{overflow:hidden;text-overflow:ellipsis;}
</style>
</head>
<body>
<div class="matc-host-head"><span id="matc-title" class="matc-host-title">__TITLE__</span><span id="matc-active" class="matc-host-active">Handle -</span></div>
<div class="matc-host-row menu">
  <button type="button" data-command="save-svg">Save SVG</button>
  <button type="button" data-command="open-browser">Open in Browser</button>
  <button type="button" data-command="refresh">Refresh</button>
  <button type="button" data-command="close">Close</button>
  <button type="button" data-command="about">About</button>
</div>
<div class="matc-host-row nav">
  <button type="button" data-command="pan">Pan</button>
  <button type="button" data-command="rotate">Rotate</button>
  <button type="button" data-command="brush">Brush</button>
  <button type="button" data-command="clear-brush">Clear Brush</button>
  <button type="button" data-command="datatips">Data Tips</button>
  <button type="button" data-command="clear-tips">Clear Tips</button>
  <button type="button" data-command="zoom-in">Zoom In</button>
  <button type="button" data-command="zoom-out">Zoom Out</button>
  <button type="button" data-command="reset">Reset View</button>
</div>
<div class="matc-host-status"><span id="matc-status">Loading figure viewer...</span><span id="matc-mode">Mode: inspect</span></div>
<script>
function post(command){if(window.ipc&&window.ipc.postMessage){window.ipc.postMessage(JSON.stringify({type:'command',command:command}));}}
document.addEventListener('click',function(event){var button=event.target.closest('button[data-command]');if(!button){return;}post(button.getAttribute('data-command'));});
window.matcToolbarSetState=function(payload){
  if(typeof payload==='string'){try{payload=JSON.parse(payload);}catch(error){payload={status:String(payload)};}}
  payload=payload||{};
  document.getElementById('matc-title').textContent=payload.title||'__TITLE__';
  document.getElementById('matc-status').textContent=payload.status||'Loading figure viewer...';
  document.getElementById('matc-active').textContent=payload.active_handle?('Handle '+payload.active_handle):'Handle -';
  document.getElementById('matc-mode').textContent=payload.mode?('Mode: '+payload.mode):'Mode: inspect';
  var activeMode=(payload.mode||'').toLowerCase();
  var buttons=document.querySelectorAll('button[data-command]');
  for(var i=0;i<buttons.length;i++){var button=buttons[i];var command=button.getAttribute('data-command');var isActive=(command==='pan'&&activeMode==='pan')||(command==='rotate'&&activeMode==='rotate')||(command==='brush'&&activeMode==='brush')||(command==='datatips'&&activeMode==='datatip');button.className=isActive?'mode-active':'';}
};
</script>
</body>
</html>"#
    .replace("__TITLE__", &html_escape(title))
}

#[allow(dead_code)]
fn parse_toolbar_command(raw: &str) -> Option<HostCommand> {
    let envelope = serde_json::from_str::<ToolbarCommandEnvelope>(raw).ok()?;
    if envelope.r#type != "command" {
        return None;
    }
    HostCommand::parse(&envelope.command)
}

fn parse_content_state(raw: &str) -> Option<ContentStatePayload> {
    serde_json::from_str(raw).ok()
}

fn open_browser_target(path: &Path) -> Result<(), String> {
    let Some(target) = crate::file_url_from_path(path) else {
        return Err(format!(
            "failed to build browser URL for `{}`",
            path.display()
        ));
    };
    if let Some(browser) = crate::preferred_app_browser_path() {
        process::Command::new(browser)
            .arg(format!("--app={target}"))
            .arg("--new-window")
            .arg("--window-size=1280,900")
            .arg("--no-first-run")
            .spawn()
            .map_err(|error| format!("failed to open browser target `{target}`: {error}"))?;
        return Ok(());
    }
    process::Command::new("cmd")
        .arg("/C")
        .arg("start")
        .arg("")
        .arg(target)
        .spawn()
        .map_err(|error| format!("failed to open browser target: {error}"))?;
    Ok(())
}

fn read_host_page_html(session_dir: &Path, page_name: &str) -> Result<String, String> {
    let path = session_dir.join(page_name);
    fs::read_to_string(&path)
        .map_err(|error| format!("failed to read host page `{}`: {error}", path.display()))
}

#[allow(dead_code)]
fn prompt_save_svg_path(window: &Window, default_name: &str) -> Option<PathBuf> {
    let mut file_buffer = [0u16; 260];
    let default_wide = utf16_null_terminated(default_name);
    for (index, value) in default_wide
        .iter()
        .copied()
        .take(file_buffer.len().saturating_sub(1))
        .enumerate()
    {
        file_buffer[index] = value;
    }

    let filter = utf16_multi_sz("SVG files (*.svg)\0*.svg\0All files (*.*)\0*.*\0\0");
    let def_ext = utf16_null_terminated("svg");
    let mut open = OPENFILENAMEW::default();
    open.lStructSize = size_of::<OPENFILENAMEW>() as u32;
    open.hwndOwner = HWND(window.hwnd() as _);
    open.lpstrFilter = PCWSTR(filter.as_ptr());
    open.lpstrFile = PWSTR(file_buffer.as_mut_ptr());
    open.nMaxFile = file_buffer.len() as u32;
    open.lpstrDefExt = PCWSTR(def_ext.as_ptr());
    open.Flags = OFN_EXPLORER | OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR;

    let accepted = unsafe { GetSaveFileNameW(&mut open).as_bool() };
    if !accepted {
        return None;
    }

    let end = file_buffer
        .iter()
        .position(|value| *value == 0)
        .unwrap_or(file_buffer.len());
    let path = String::from_utf16_lossy(&file_buffer[..end]);
    if path.is_empty() {
        None
    } else {
        Some(PathBuf::from(path))
    }
}

#[allow(dead_code)]
fn utf16_null_terminated(text: &str) -> Vec<u16> {
    let mut wide = text.encode_utf16().collect::<Vec<_>>();
    wide.push(0);
    wide
}

#[allow(dead_code)]
fn utf16_multi_sz(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn env_current_exe() -> Result<PathBuf, std::io::Error> {
    std::env::current_exe()
}

#[allow(dead_code)]
fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use tao::platform::windows::EventLoopBuilderExtWindows;

    #[test]
    fn parses_session_manifest_with_browser_and_host_pages() {
        let json = r#"{"title":"MATC","revision":42,"figures":[{"handle":1,"title":"Figure 1","visible":true,"window_style":"normal","position":[1,2,3,4],"page":"figure-1.html","browser_page":"figure-1.html","host_page":"host-figure-1.html","svg":"figure-1.svg"}]}"#;
        let manifest: SessionManifest = serde_json::from_str(json).expect("parse manifest");
        assert_eq!(manifest.revision, 42);
        assert_eq!(manifest.figures.len(), 1);
        assert_eq!(manifest.figures[0].browser_page_name(), "figure-1.html");
        assert_eq!(manifest.figures[0].host_page_name(), "host-figure-1.html");
        assert!(!manifest.figures[0].is_docked());
    }

    #[test]
    fn host_page_falls_back_to_browser_page_when_missing() {
        let json = r#"{"title":"MATC","revision":42,"figures":[{"handle":1,"title":"Figure 1","visible":true,"window_style":"normal","position":[1,2,3,4],"page":"figure-1.html","browser_page":"figure-1.html","svg":"figure-1.svg"}]}"#;
        let manifest: SessionManifest = serde_json::from_str(json).expect("parse manifest");
        assert_eq!(manifest.figures[0].host_page_name(), "figure-1.html");
    }

    #[test]
    fn local_close_suppresses_reopen_until_revision_advances() {
        let temp_dir = env::temp_dir().join("matc_windows_host_test");
        let mut builder = EventLoopBuilder::<HostUserEvent>::with_user_event();
        builder.with_any_thread(true);
        let proxy = builder.build().create_proxy();
        let mut host =
            WebViewFigureHost::new(temp_dir.clone(), temp_dir.join(HOST_STATUS_FILE), proxy);
        host.note_session_revision(100);
        host.note_local_close(7);
        assert!(host.should_suppress_handle_for_revision(7, 100));
        assert!(host.should_suppress_handle_for_revision(7, 99));
        host.note_session_revision(101);
        assert!(!host.should_suppress_handle_for_revision(7, 101));
    }

    #[test]
    fn failed_window_creation_suppresses_retry_until_revision_advances() {
        let temp_dir = env::temp_dir().join("matc_windows_host_test_failed");
        let mut builder = EventLoopBuilder::<HostUserEvent>::with_user_event();
        builder.with_any_thread(true);
        let proxy = builder.build().create_proxy();
        let mut host =
            WebViewFigureHost::new(temp_dir.clone(), temp_dir.join(HOST_STATUS_FILE), proxy);
        host.note_session_revision(200);
        host.note_failed_key(HostWindowKey::Figure(9));
        assert!(host.should_suppress_key_for_revision(HostWindowKey::Figure(9), 200));
        host.note_session_revision(201);
        assert!(!host.should_suppress_key_for_revision(HostWindowKey::Figure(9), 201));
    }

    #[test]
    fn toolbar_commands_map_to_expected_viewer_functions() {
        assert_eq!(
            HostCommand::Pan.js_function(),
            Some("matcToggleActiveFigurePanMode")
        );
        assert_eq!(
            HostCommand::Rotate.js_function(),
            Some("matcToggleActiveFigureRotateMode")
        );
        assert_eq!(
            HostCommand::Brush.js_function(),
            Some("matcToggleActiveFigureBrushMode")
        );
        assert_eq!(
            HostCommand::ZoomIn.js_function(),
            Some("matcZoomInActiveFigure")
        );
        assert_eq!(
            HostCommand::ResetView.js_function(),
            Some("matcResetActiveFigure")
        );
        assert_eq!(HostCommand::SaveSvg.js_function(), None);
    }

    #[test]
    fn parses_toolbar_command_payloads() {
        assert_eq!(
            parse_toolbar_command(r#"{"type":"command","command":"zoom-in"}"#),
            Some(HostCommand::ZoomIn)
        );
        assert_eq!(parse_toolbar_command(r#"{"type":"state"}"#), None);
    }

    #[test]
    fn host_status_path_and_states_round_trip() {
        let dir = env::temp_dir().join("matc_windows_host_test_status");
        let path = host_status_path(&dir);
        assert!(path.ends_with(HOST_STATUS_FILE));
        let ready = HostLaunchState::ready();
        write_host_status(&path, &ready).expect("write status");
        assert_eq!(read_host_status(&path), Some(ready));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn toolbar_html_includes_expected_controls() {
        let html = render_host_toolbar_html("Figure 1");
        assert!(html.contains("data-command=\"save-svg\""), "{html}");
        assert!(html.contains("data-command=\"open-browser\""), "{html}");
        assert!(html.contains("data-command=\"pan\""), "{html}");
        assert!(html.contains("data-command=\"reset\""), "{html}");
        assert!(html.contains("matcToolbarSetState"), "{html}");
    }

    #[test]
    fn dock_title_prefers_active_figure_when_available() {
        let dir = env::temp_dir().join("matc_windows_host_test_title");
        let mut builder = EventLoopBuilder::<HostUserEvent>::with_user_event();
        builder.with_any_thread(true);
        let proxy = builder.build().create_proxy();
        let mut host = WebViewFigureHost::new(dir.clone(), dir.join(HOST_STATUS_FILE), proxy);
        host.current_figures.insert(
            2,
            FigureManifest {
                handle: 2,
                title: "Docked Wave".to_string(),
                visible: true,
                window_style: "docked".to_string(),
                position: [1.0, 2.0, 3.0, 4.0],
                page: "figure-2.html".to_string(),
                browser_page: "figure-2.html".to_string(),
                host_page: "host-figure-2.html".to_string(),
                svg: "figure-2.svg".to_string(),
            },
        );
        assert_eq!(
            host.dock_window_title(Some(2)),
            format!("{DOCK_WINDOW_TITLE}: Docked Wave")
        );
    }
}
