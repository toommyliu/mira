import os.path

app = defines["app"]
title_bar = 32

format = "UDZO"
files = [app]
symlinks = {"Applications": "/Applications"}
icon = os.path.join(app, "Contents/Resources/Mira.icns")
background = defines["background"]

window_rect = ((200, 200), (640, 400 + title_bar))
show_status_bar = False
show_tab_view = False
show_toolbar = False
show_pathbar = False
show_sidebar = False
icon_size = 128
text_size = 13
icon_locations = {os.path.basename(app): (180, 180), "Applications": (460, 180)}
