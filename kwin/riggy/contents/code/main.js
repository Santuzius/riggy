// Places riggy's window on the top edge of the bottom panel, at the right end, and keeps it above other windows, out of the taskbar, the pager and Alt+Tab, on all desktops. riggy's Wayland app ID (X11 class) is "riggy".

const APP_ID = "riggy";

function isRiggy(window) {
    return window.resourceClass === APP_ID || window.desktopFileName === APP_ID;
}

// Bottom right of the area windows may use, which ends where a bottom panel begins.
function place(window) {
    const area = workspace.clientArea(KWin.MaximizeArea, window.output, workspace.currentDesktop);
    const frame = window.frameGeometry;
    window.frameGeometry = {
        x: area.x + area.width - frame.width,
        y: area.y + area.height - frame.height,
        width: frame.width,
        height: frame.height
    };
}

function setUp(window) {
    if (!isRiggy(window)) {
        return;
    }
    window.keepAbove = true;
    window.skipTaskbar = true;
    window.skipPager = true;
    window.skipSwitcher = true;
    window.onAllDesktops = true;
    place(window);
}

workspace.windowAdded.connect(setUp);
workspace.windowList().forEach(setUp);
// Panels and screens that change move the edge riggy stands on.
workspace.virtualScreenGeometryChanged.connect(() => workspace.windowList().filter(isRiggy).forEach(place));
