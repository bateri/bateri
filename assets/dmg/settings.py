# DMG'nin düzeni: dmgbuild'in ayar dosyası (Python). `make paket` onu
# `-D app=… -D icon=…` ile koşturuyor; yollar oradan, sayılar buradan.
#
# Yerleşim `tools/dmg_background.py`'nin çizdiği ok ve başlıkla aynı sayılar
# (WINDOW, APP, APPS): biri değişirse öteki de değişmeli.
import os.path

app = defines["app"]  # noqa: F821 — dmgbuild enjekte ediyor

# Sıkıştırılmış, salt okunur; ULFO (lzfse) macOS 10.11'den beri açılıyor,
# taban 14. Dosya sistemi APFS değil HFS+: Finder'ın simge düzeni (.DS_Store)
# ve arka plan her iki biçimde de tutuluyor ama HFS+ imajları daha küçük ve
# eski araçların tamamı onu tanıyor.
format = "ULFO"
filesystem = "HFS+"
size = None

files = [app]
symlinks = {"Applications": "/Applications"}
# Birimin Finder'daki simgesi uygulamanınki.
icon = defines["icon"]  # noqa: F821

background = os.path.join(defines["here"], "background.png")  # noqa: F821

# Pencere: araç çubuğu, kenar çubuğu, yol ve durum çubukları kapalı — tek iş
# var ve pencere ondan başka bir şey göstermemeli.
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
