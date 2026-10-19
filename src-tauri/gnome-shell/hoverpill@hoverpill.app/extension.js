// Shared with HoverPill (src-tauri/src/shellext.rs), which installs it to
// `~/.local/share/gnome-shell/extensions/hoverpill@hoverpill.app/` and enables
// it. GNOME loads extensions only at session start, so after the first install
// a user has to log out and back in once.
//
// The pill window runs under XWayland, and a native Wayland window (Firefox on
// Ubuntu, GTK4 apps) has no X window at all, so nothing on the X server can
// tell HoverPill that a fullscreen app is up. Inside the shell they're all
// visible, so this answers one small question over D-Bus for HoverPill:
// "is a fullscreen app up on monitor N?".

import Gio from 'gi://Gio';
import Meta from 'gi://Meta';

const XML = `
<node>
  <interface name="org.hoverpill.Notch">
    <method name="IsFullscreen">
      <arg direction="in" type="i" name="monitor"/>
      <arg direction="out" type="b" name="fullscreen"/>
    </method>
  </interface>
</node>`;

export default class Hoverpill {
    enable() {
        this._impl = Gio.DBusExportedObject.wrapJSObject(XML, this);
        this._impl.export(Gio.DBus.session, '/org/hoverpill/Notch');
        // Exporting the object doesn't own the bus name; without this, a call
        // addressed to `org.hoverpill.Notch` never reaches the shell.
        this._ownedName = Gio.bus_own_name(
            Gio.BusType.SESSION, 'org.hoverpill.Notch', Gio.BusNameOwnerFlags.NONE,
            null, null, null,
        );
    }

    disable() {
        if (this._ownedName) {
            Gio.bus_unown_name(this._ownedName);
            this._ownedName = 0;
        }
        this._impl.unexport();
        this._impl = null;
    }

    /**
     * True when a fullscreen app covers monitor `monitor` on the active
     * workspace.
     *
     * The primary check is the same one GNOME itself uses when switching
     * workspaces: a normal, non-hidden window with the fullscreen flag on that
     * monitor. The frame-rect comparison is a fallback for windows that cover
     * the monitor entirely without the flag; a merely maximised window always
     * leaves the top bar area uncovered by design, so it can never pass.
     * Windows on other workspaces are simply not in the list.
     */
    IsFullscreen(monitor) {
        if (monitor < 0 || monitor >= global.display.get_n_monitors()) {
            return false;
        }
        const monitorRect = global.display.get_monitor_geometry(monitor);
        const workspace = global.workspace_manager.get_active_workspace();
        for (const window of workspace.list_windows()) {
            if (window.is_hidden()) {
                continue;
            }
            if (window.get_window_type() !== Meta.WindowType.NORMAL) {
                continue;
            }
            if (window.is_fullscreen() && window.get_monitor() === monitor) {
                return true;
            }
            const frame = window.get_frame_rect();
            if (frame.x <= monitorRect.x && frame.y <= monitorRect.y
                && frame.x + frame.width >= monitorRect.x + monitorRect.width
                && frame.y + frame.height >= monitorRect.y + monitorRect.height) {
                return true;
            }
        }
        return false;
    }
}
