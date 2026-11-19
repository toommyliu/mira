# mira

Mira is a macOS desktop automation client for AdventureQuest Worlds, built with GPUI and an embedded Ruffle player.

## Design canvas

`cargo run --locked --features storyboard` opens the GPUI design canvas. Click a window or choose it from the Windows menu to interact with its mock data. The overview shows each window's starting state. Changes you make in an inspected window last for the canvas session.

Open **Statuses** while inspecting a window to see a sample toast or try the in-place progress and error examples where available.

Scroll or Space-drag to pan. Pinch, Command-scroll, or use the zoom buttons to zoom. Command-0 fits all windows and Escape returns from an inspected window.
