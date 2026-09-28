//! User-driven Accessibility setup. The drag exports only the running app's
//! file URL; only System Settings can grant the permission.

use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::{AnyThread, DefinedClass, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSAccessibility, NSAlert, NSButton, NSControl, NSDragOperation, NSDraggingContext,
    NSDraggingItem, NSDraggingSession, NSDraggingSource, NSEvent, NSFloatingWindowLevel, NSFont,
    NSImageScaling, NSImageView, NSPasteboardWriting, NSResponder, NSScreen, NSTextField, NSView,
    NSWindow, NSWorkspace,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSObject, NSObjectProtocol, NSPoint, NSRect, NSRunLoop,
    NSRunLoopCommonModes, NSSize, NSString, NSTimer, NSURL,
};
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

/// Use the enclosing bundle only for its actual executable, not an unrelated
/// `.app` ancestor (for example a build directory under an editor bundle).
fn drag_path(executable: &Path) -> PathBuf {
    let contents = executable.parent().and_then(Path::parent);
    if let Some(contents) = contents
        && contents.file_name().is_some_and(|name| name == "Contents")
        && executable
            .parent()
            .is_some_and(|dir| dir.ends_with("MacOS"))
        && let Some(bundle) = contents.parent()
        && bundle
            .extension()
            .is_some_and(|extension| extension == "app")
    {
        return bundle.to_owned();
    }
    executable.to_owned()
}

struct DragIvars {
    url: Retained<NSURL>,
}

define_class!(
    // SAFETY: NSImageView has no additional subclassing requirements.
    #[unsafe(super(NSImageView, NSControl, NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "AiubPermissionAppIcon"]
    #[ivars = DragIvars]
    struct AppIcon;

    unsafe impl NSObjectProtocol for AppIcon {}

    unsafe impl NSDraggingSource for AppIcon {
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn operation(&self, _session: &NSDraggingSession, _context: NSDraggingContext) -> NSDragOperation {
            NSDragOperation::Copy
        }
    }

    impl AppIcon {
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, _event: &NSEvent) {}

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let item = NSDraggingItem::initWithPasteboardWriter(
                NSDraggingItem::alloc(),
                ProtocolObject::<dyn NSPasteboardWriting>::from_ref::<NSURL>(&self.ivars().url),
            );
            let image = self.image();
            // SAFETY: AppKit accepts NSImage as the drag preview contents.
            unsafe { item.setDraggingFrame_contents(self.bounds(), image.as_deref().map(|i| i.as_ref())) };
            self.beginDraggingSessionWithItems_event_source(
                &NSArray::from_slice(&[item.as_ref()]), event,
                ProtocolObject::<dyn NSDraggingSource>::from_ref(self),
            );
        }
    }
);

impl AppIcon {
    fn new(mtm: MainThreadMarker, path: &Path) -> Retained<Self> {
        let path = NSString::from_str(&path.to_string_lossy());
        let this = Self::alloc(mtm).set_ivars(DragIvars {
            url: NSURL::fileURLWithPath(&path),
        });
        // SAFETY: initializes the allocated NSImageView with a valid frame.
        let this: Retained<Self> = unsafe {
            msg_send![super(this), initWithFrame: NSRect::new(NSPoint::new(8.0, 86.0), NSSize::new(64.0, 64.0))]
        };
        this.setImage(Some(&NSWorkspace::sharedWorkspace().iconForFile(&path)));
        this.setImageScaling(NSImageScaling::ScaleProportionallyUpOrDown);
        this.setAccessibilityLabel(Some(&NSString::from_str(
            "Arrastar AI Usage para a lista de Acessibilidade",
        )));
        this
    }
}

struct ActionIvars {
    status: Retained<NSTextField>,
    done: Retained<NSButton>,
    check: Box<dyn Fn() -> bool>,
    changed: Box<dyn Fn()>,
    last_granted: Cell<bool>,
    presented: Cell<bool>,
    window: Retained<NSWindow>,
}

define_class!(
    // SAFETY: NSObject has no additional subclassing requirements.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "AiubPermissionActions"]
    #[ivars = ActionIvars]
    struct Actions;

    impl Actions {
        #[unsafe(method(checkAccess:))]
        fn check_access(&self, timer: Option<&NSTimer>) {
            if self.ivars().presented.get() && !self.ivars().window.isVisible() {
                if let Some(timer) = timer { timer.invalidate(); }
                return;
            }
            let granted = (self.ivars().check)();
            if self.ivars().last_granted.replace(granted) != granted {
                (self.ivars().changed)();
            }
            self.ivars().done.setEnabled(granted);
            self.ivars().status.setStringValue(&NSString::from_str(if granted {
                "Acesso permitido. A barra já pode usar o espaço livre."
            } else {
                "Aguardando autorização do macOS…"
            }));
        }

        #[unsafe(method(dismissGuide:))]
        fn dismiss_guide(&self, _sender: &NSButton) {
            self.ivars().window.orderOut(None);
        }

        #[unsafe(method(openSettings:))]
        fn open_settings(&self, _sender: &NSButton) {
            let url = NSURL::URLWithString(&NSString::from_str(
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
            )).expect("static System Settings URL");
            if !NSWorkspace::sharedWorkspace().openURL(&url) {
                self.ivars().status.setStringValue(&NSString::from_str(
                    "Abra Ajustes do Sistema > Privacidade e Segurança > Acessibilidade.",
                ));
            }
        }
    }
);

fn label(mtm: MainThreadMarker, text: &str, frame: NSRect) -> Retained<NSTextField> {
    let label = NSTextField::wrappingLabelWithString(&NSString::from_str(text), mtm);
    label.setFrame(frame);
    label
}

thread_local! {
    static GUIDE: RefCell<Option<Guide>> = const { RefCell::new(None) };
}

pub(super) fn show(changed: Box<dyn Fn()>) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Ok(executable) = std::env::current_exe() else {
        return;
    };
    let path = drag_path(&executable);
    GUIDE.with_borrow_mut(|current| {
        if let Some(guide) = current.as_ref()
            && guide.alert.window().isVisible()
        {
            guide.alert.window().makeKeyAndOrderFront(None);
            return;
        }
        let guide = Guide::new(mtm, &path, Box::new(super::trusted), changed);
        guide.present(mtm);
        *current = Some(guide);
    });
}

pub(crate) struct Guide {
    pub(crate) alert: Retained<NSAlert>,
    timer: Retained<NSTimer>,
    _actions: Retained<Actions>,
}

impl Guide {
    pub(crate) fn new(
        mtm: MainThreadMarker,
        path: &Path,
        check: Box<dyn Fn() -> bool>,
        changed: Box<dyn Fn()>,
    ) -> Self {
        let alert = NSAlert::new(mtm);
        alert.setMessageText(&NSString::from_str("Centralizar a barra de provedores"));
        alert.setInformativeText(&NSString::from_str(
        "O AI Usage precisa da Acessibilidade para ler a posição dos menus e encontrar o espaço livre na barra. Essa permissão também permite controlar elementos de outros aplicativos.",
    ));
        let done = alert.addButtonWithTitle(&NSString::from_str("Concluído"));
        done.setEnabled(false);
        let cancel = alert.addButtonWithTitle(&NSString::from_str("Agora não"));
        let view = NSView::initWithFrame(
            NSView::alloc(mtm),
            NSRect::new(NSPoint::ZERO, NSSize::new(440.0, 190.0)),
        );
        let icon = AppIcon::new(mtm, path);
        view.addSubview(&icon);
        view.addSubview(&label(mtm,
        "1. Abra os ajustes do macOS.\n2. Arraste este ícone para a lista.\n3. Ative o AI Usage e confirme no macOS.",
        NSRect::new(NSPoint::new(88.0, 88.0), NSSize::new(344.0, 68.0)),
    ));
        let location = label(
            mtm,
            &path.to_string_lossy(),
            NSRect::new(NSPoint::new(8.0, 58.0), NSSize::new(424.0, 28.0)),
        );
        location.setFont(Some(&NSFont::systemFontOfSize(11.0)));
        view.addSubview(&location);
        let status = label(
            mtm,
            "Aguardando autorização do macOS…",
            NSRect::new(NSPoint::new(8.0, 0.0), NSSize::new(424.0, 24.0)),
        );
        view.addSubview(&status);
        let target = Actions::alloc(mtm).set_ivars(ActionIvars {
            status: status.clone(),
            done: done.clone(),
            check,
            changed,
            last_granted: Cell::new(false),
            presented: Cell::new(false),
            window: alert.window(),
        });
        // SAFETY: NSObject initialization and a target/action pair retained below
        // for the whole guide lifetime.
        let target: Retained<Actions> = unsafe { msg_send![super(target), init] };
        for button in [&done, &cancel] {
            // SAFETY: the guide retains this target with its dismiss action.
            unsafe {
                button.setTarget(Some(&target));
                button.setAction(Some(sel!(dismissGuide:)));
            }
        }
        cancel.setKeyEquivalent(&NSString::from_str("\u{1b}"));
        let open = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str("Abrir Ajustes do macOS"),
                Some(&target),
                Some(sel!(openSettings:)),
                mtm,
            )
        };
        open.setFrame(NSRect::new(
            NSPoint::new(4.0, 28.0),
            NSSize::new(230.0, 28.0),
        ));
        view.addSubview(&open);
        alert.setAccessoryView(Some(&view));
        target.check_access(sel!(checkAccess:), None);
        // SAFETY: the target implements checkAccess: and stays alive throughout
        // the guide. The timer is attached only to this thread's AppKit run loop.
        let timer = unsafe {
            let timer = NSTimer::timerWithTimeInterval_target_selector_userInfo_repeats(
                0.5,
                &target,
                sel!(checkAccess:),
                None,
                true,
            );
            let run_loop = NSRunLoop::mainRunLoop();
            run_loop.addTimer_forMode(&timer, NSRunLoopCommonModes);
            timer
        };
        Self {
            alert,
            timer,
            _actions: target,
        }
    }

    pub(crate) fn present(&self, mtm: MainThreadMarker) {
        self.alert.layout();
        let window = self.alert.window();
        if let Some(screen) = NSScreen::mainScreen(mtm) {
            let visible = screen.visibleFrame();
            window.setFrameOrigin(NSPoint::new(
                visible.origin.x + (visible.size.width - window.frame().size.width) / 2.0,
                visible.origin.y + 20.0,
            ));
        }
        window.setHidesOnDeactivate(false);
        window.setLevel(NSFloatingWindowLevel);
        window.setMovableByWindowBackground(true);
        window.makeKeyAndOrderFront(None);
        self._actions.ivars().presented.set(true);
    }
}

impl Drop for Guide {
    fn drop(&mut self) {
        self.timer.invalidate();
        self.alert.window().orderOut(None);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drags_the_running_bundle_or_standalone_executable() {
        assert_eq!(
            drag_path(Path::new(
                "/Applications/AI Usage.app/Contents/MacOS/ai-usagebar-tray"
            )),
            Path::new("/Applications/AI Usage.app")
        );
        assert_eq!(
            drag_path(Path::new("/tmp/target/debug/ai-usagebar-tray")),
            Path::new("/tmp/target/debug/ai-usagebar-tray")
        );
        assert_eq!(
            drag_path(Path::new("/tmp/Editor.app/build/ai-usagebar-tray")),
            Path::new("/tmp/Editor.app/build/ai-usagebar-tray")
        );
    }
}
