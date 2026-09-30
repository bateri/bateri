# The DMG's layout: dmgbuild's settings file (Python). `make package` runs it
# with `-D app=… -D icon=…`; the paths come from there, the numbers from here.
#
# The layout uses the same numbers as the arrow and title drawn by
# `tools/dmg_background.py` (WINDOW, APP, APPS): if one changes the other must
# change too.
import os.path

app = defines["app"]  # noqa: F821 — dmgbuild injects it

# Compressed, read-only; ULFO (lzfse) has opened since macOS 10.11, the floor
# is 14. The filesystem is HFS+, not APFS: Finder's icon layout (.DS_Store) and
# the background are kept in both formats but HFS+ images are smaller and all
# older tools recognise it.
format = "ULFO"
filesystem = "HFS+"
size = None

files = [app]
symlinks = {"Applications": "/Applications"}
# The volume's icon in Finder is the application's.
icon = defines["icon"]  # noqa: F821

background = os.path.join(defines["here"], "background.png")  # noqa: F821

# Window: toolbar, sidebar, path and status bars are off — there is a single
# job and the window must show nothing else.
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
window_rect = ((200, 160), (640, 400))
default_view = "icon-view"
show_icon_preview = False
arrange_by = None
icon_size = 128
text_size = 13
label_pos = "bottom"
icon_locations = {
    os.path.basename(app): (170, 210),
    "Applications": (470, 210),
}
