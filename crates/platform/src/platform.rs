use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

use anyhow::Result;
use futures::channel::oneshot;
use gpui::{
    Action, ActivationPolicy, ActivityGuard, AnyWindowHandle, AppLifecyclePhase,
    BackgroundExecutor, ClipboardItem, ClipboardReadError, CursorStyle, ForegroundExecutor,
    GraphicalEnvironment, Keymap, Menu, MenuItem, OwnedMenu, PathPromptOptions, Platform,
    PlatformDisplay, PlatformGestures, PlatformInfo, PlatformKeyboardLayout,
    PlatformKeyboardMapper, PlatformTextSystem, PlatformWindow, PowerMode, ScreenCaptureSource,
    SystemNotification, SystemNotificationResponse, Task, ThermalState, WindowAppearance,
    WindowButtonLayout, WindowParams, WindowingRequest,
};
use gpui_macos::MacPlatform;
use smallvec::SmallVec;

use crate::AppleTextSystem;

pub fn application() -> gpui::Application {
    gpui::Application::with_platform(Rc::new(MiraPlatform {
        mac: MacPlatform::new(false),
        text_system: Arc::new(AppleTextSystem::new()),
    }))
}

struct MiraPlatform {
    mac: MacPlatform,
    text_system: Arc<AppleTextSystem>,
}

// Forward every method, including those with default bodies, so MacPlatform's own versions run.
impl Platform for MiraPlatform {
    fn platform_info(&self) -> PlatformInfo {
        self.mac.platform_info()
    }

    fn background_executor(&self) -> BackgroundExecutor {
        self.mac.background_executor()
    }

    fn foreground_executor(&self) -> ForegroundExecutor {
        self.mac.foreground_executor()
    }

    fn text_system(&self) -> Arc<dyn PlatformTextSystem> {
        self.text_system.clone()
    }

    fn run(&self, on_finish_launching: Box<dyn 'static + FnOnce()>) {
        self.mac.run(on_finish_launching)
    }

    fn quit(&self) {
        self.mac.quit()
    }

    fn set_initial_windowing(&self, request: WindowingRequest) {
        self.mac.set_initial_windowing(request)
    }

    fn request_windowing(&self, request: WindowingRequest) -> Task<anyhow::Result<()>> {
        self.mac.request_windowing(request)
    }

    fn restart(&self, binary_path: Option<PathBuf>, arguments: Vec<OsString>) {
        self.mac.restart(binary_path, arguments)
    }

    fn activate(&self, ignoring_other_apps: bool) {
        self.mac.activate(ignoring_other_apps)
    }

    fn set_activation_policy(&self, policy: ActivationPolicy) {
        self.mac.set_activation_policy(policy)
    }

    fn hide(&self) {
        self.mac.hide()
    }

    fn hide_other_apps(&self) {
        self.mac.hide_other_apps()
    }

    fn unhide_other_apps(&self) {
        self.mac.unhide_other_apps()
    }

    fn displays(&self) -> Vec<Rc<dyn PlatformDisplay>> {
        self.mac.displays()
    }

    fn primary_display(&self) -> Option<Rc<dyn PlatformDisplay>> {
        self.mac.primary_display()
    }

    fn on_displays_changed(&self, callback: Box<dyn FnMut()>) {
        self.mac.on_displays_changed(callback)
    }

    fn active_window(&self) -> Option<AnyWindowHandle> {
        self.mac.active_window()
    }

    fn window_stack(&self) -> Option<Vec<AnyWindowHandle>> {
        self.mac.window_stack()
    }

    fn is_screen_capture_supported(&self) -> bool {
        self.mac.is_screen_capture_supported()
    }

    fn screen_capture_sources(
        &self,
    ) -> oneshot::Receiver<anyhow::Result<Vec<Rc<dyn ScreenCaptureSource>>>> {
        self.mac.screen_capture_sources()
    }

    fn open_window(
        &self,
        handle: AnyWindowHandle,
        options: WindowParams,
    ) -> anyhow::Result<Box<dyn PlatformWindow>> {
        self.mac.open_window(handle, options)
    }

    fn window_appearance(&self) -> WindowAppearance {
        self.mac.window_appearance()
    }

    fn set_window_appearance(&self, appearance: Option<WindowAppearance>) {
        self.mac.set_window_appearance(appearance)
    }

    fn button_layout(&self) -> Option<WindowButtonLayout> {
        self.mac.button_layout()
    }

    fn open_url(&self, url: &str) {
        self.mac.open_url(url)
    }

    fn on_open_urls(&self, callback: Box<dyn FnMut(Vec<String>)>) {
        self.mac.on_open_urls(callback)
    }

    fn register_url_scheme(&self, url: &str) -> Task<Result<()>> {
        self.mac.register_url_scheme(url)
    }

    fn prompt_for_paths(
        &self,
        options: PathPromptOptions,
    ) -> oneshot::Receiver<Result<Option<Vec<PathBuf>>>> {
        self.mac.prompt_for_paths(options)
    }

    fn prompt_for_new_path(
        &self,
        directory: &Path,
        suggested_name: Option<&str>,
    ) -> oneshot::Receiver<Result<Option<PathBuf>>> {
        self.mac.prompt_for_new_path(directory, suggested_name)
    }

    fn can_select_mixed_files_and_dirs(&self) -> bool {
        self.mac.can_select_mixed_files_and_dirs()
    }

    fn reveal_path(&self, path: &Path) {
        self.mac.reveal_path(path)
    }

    fn open_with_system(&self, path: &Path) {
        self.mac.open_with_system(path)
    }

    fn on_quit(&self, callback: Box<dyn FnMut() -> bool>) {
        self.mac.on_quit(callback)
    }

    fn on_reopen(&self, callback: Box<dyn FnMut()>) {
        self.mac.on_reopen(callback)
    }

    fn on_system_sleep(&self, callback: Box<dyn FnMut()>) {
        self.mac.on_system_sleep(callback)
    }

    fn on_system_wake(&self, callback: Box<dyn FnMut()>) {
        self.mac.on_system_wake(callback)
    }

    fn on_app_lifecycle(&self, callback: Box<dyn FnMut(AppLifecyclePhase)>) {
        self.mac.on_app_lifecycle(callback)
    }

    fn on_memory_warning(&self, callback: Box<dyn FnMut()>) {
        self.mac.on_memory_warning(callback)
    }

    fn gestures(&self) -> Option<Rc<dyn PlatformGestures>> {
        self.mac.gestures()
    }

    fn set_menus(&self, menus: Vec<Menu>, keymap: &Keymap) {
        self.mac.set_menus(menus, keymap)
    }

    fn get_menus(&self) -> Option<Vec<OwnedMenu>> {
        self.mac.get_menus()
    }

    fn set_dock_menu(&self, menu: Vec<MenuItem>, keymap: &Keymap) {
        self.mac.set_dock_menu(menu, keymap)
    }

    fn perform_dock_menu_action(&self, action: usize) {
        self.mac.perform_dock_menu_action(action)
    }

    fn add_recent_document(&self, path: &Path) {
        self.mac.add_recent_document(path)
    }

    fn update_jump_list(
        &self,
        menus: Vec<MenuItem>,
        entries: Vec<SmallVec<[PathBuf; 2]>>,
    ) -> Task<Vec<SmallVec<[PathBuf; 2]>>> {
        self.mac.update_jump_list(menus, entries)
    }

    fn on_app_menu_action(&self, callback: Box<dyn FnMut(&dyn Action)>) {
        self.mac.on_app_menu_action(callback)
    }

    fn on_will_open_app_menu(&self, callback: Box<dyn FnMut()>) {
        self.mac.on_will_open_app_menu(callback)
    }

    fn on_validate_app_menu_command(&self, callback: Box<dyn FnMut(&dyn Action) -> bool>) {
        self.mac.on_validate_app_menu_command(callback)
    }

    fn thermal_state(&self) -> ThermalState {
        self.mac.thermal_state()
    }

    fn on_thermal_state_change(&self, callback: Box<dyn FnMut()>) {
        self.mac.on_thermal_state_change(callback)
    }

    fn prevent_idle_sleep(&self, reason: &str) -> Task<Result<ActivityGuard>> {
        self.mac.prevent_idle_sleep(reason)
    }

    fn power_mode(&self) -> PowerMode {
        self.mac.power_mode()
    }

    fn on_power_mode_change(&self, callback: Box<dyn FnMut()>) {
        self.mac.on_power_mode_change(callback)
    }

    fn set_app_identity(&self, identifier: &str, name: &str) {
        self.mac.set_app_identity(identifier, name)
    }

    fn show_system_notification(&self, notification: SystemNotification) {
        self.mac.show_system_notification(notification)
    }

    fn dismiss_system_notification(&self, tag: &str) {
        self.mac.dismiss_system_notification(tag)
    }

    fn on_system_notification_response(
        &self,
        callback: Box<dyn FnMut(SystemNotificationResponse)>,
    ) {
        self.mac.on_system_notification_response(callback)
    }

    fn compositor_name(&self) -> &'static str {
        self.mac.compositor_name()
    }

    fn graphical_environment(&self) -> Option<GraphicalEnvironment> {
        self.mac.graphical_environment()
    }

    fn app_path(&self) -> Result<PathBuf> {
        self.mac.app_path()
    }

    fn path_for_auxiliary_executable(&self, name: &str) -> Result<PathBuf> {
        self.mac.path_for_auxiliary_executable(name)
    }

    fn set_cursor_style(&self, style: CursorStyle) {
        self.mac.set_cursor_style(style)
    }

    fn hide_cursor_until_mouse_moves(&self) {
        self.mac.hide_cursor_until_mouse_moves()
    }

    fn is_cursor_visible(&self) -> bool {
        self.mac.is_cursor_visible()
    }

    fn should_auto_hide_scrollbars(&self) -> bool {
        self.mac.should_auto_hide_scrollbars()
    }

    fn read_from_clipboard(&self) -> Option<ClipboardItem> {
        self.mac.read_from_clipboard()
    }

    fn write_to_clipboard(&self, item: ClipboardItem) {
        self.mac.write_to_clipboard(item)
    }

    fn read_from_clipboard_async(&self) -> Task<Result<Option<ClipboardItem>, ClipboardReadError>> {
        self.mac.read_from_clipboard_async()
    }

    fn read_from_find_pasteboard(&self) -> Option<ClipboardItem> {
        self.mac.read_from_find_pasteboard()
    }

    fn write_to_find_pasteboard(&self, item: ClipboardItem) {
        self.mac.write_to_find_pasteboard(item)
    }

    fn write_credentials(&self, url: &str, username: &str, password: &[u8]) -> Task<Result<()>> {
        self.mac.write_credentials(url, username, password)
    }

    fn write_credentials_if_not_exists(
        &self,
        url: &str,
        username: &str,
        password: &[u8],
    ) -> Task<Result<bool>> {
        self.mac
            .write_credentials_if_not_exists(url, username, password)
    }

    fn read_credentials(&self, url: &str) -> Task<Result<Option<(String, Vec<u8>)>>> {
        self.mac.read_credentials(url)
    }

    fn delete_credentials(&self, url: &str) -> Task<Result<()>> {
        self.mac.delete_credentials(url)
    }

    fn keyboard_layout(&self) -> Box<dyn PlatformKeyboardLayout> {
        self.mac.keyboard_layout()
    }

    fn keyboard_mapper(&self) -> Rc<dyn PlatformKeyboardMapper> {
        self.mac.keyboard_mapper()
    }

    fn on_keyboard_layout_change(&self, callback: Box<dyn FnMut()>) {
        self.mac.on_keyboard_layout_change(callback)
    }
}
