# GPUI font comparison on macOS

Run this from the Mira worktree to open two native windows side by side.

```sh
./research/gpui-fonts/compare
```

Start with the **500** row. Each window draws the same sample in Apple’s system font and bundled Inter Variable, at 13 and 11 px, on light and dark backgrounds. **Before** uses the GPUI revision in Mira’s lockfile. **After** uses the same source and dependencies with the Apple text backend patch applied. You can pass your own text as one quoted argument. The first run builds both binaries. Subsequent runs reuse them under `target/font-comparison`.

## What the patch fixes

**Apple system Medium.** On macOS 27.2.1, AppKit reports a normalized weight of 0.23 for Medium. The pinned font-kit maps 0.2 to CSS weight 500. When it cannot match a request for exactly 500, its matching algorithm prefers 400. GPUI consequently draws Regular for Medium. The patch identifies the system face by AppKit’s own Medium weight constant before matching. [font-kit weight conversion](https://github.com/zed-industries/font-kit/blob/94b0f28166665e8fd2f53ff6d268a14955c82269/src/loaders/core_text.rs#L61) · [font-kit matching](https://github.com/zed-industries/font-kit/blob/94b0f28166665e8fd2f53ff6d268a14955c82269/src/matching.rs#L91)

**Bundled Inter Variable.** GPUI’s in-memory loader creates a single font handle for the supplied file. Regular, Medium and Semibold then select the same default instance. Core Text exposes nine named weights in this Inter file. The patch gives GPUI one native handle per descriptor and retains the original font data. Registration stays inside the process. [Pinned GPUI loader](https://github.com/zed-industries/zed/blob/985bbf42e684ef5b5a9312a6abf075aeafbaf5b9/crates/gpui_apple/src/text_system.rs#L264) · [Core Text font descriptors](https://developer.apple.com/documentation/coretext/ctfontmanagercreatefontdescriptorsfromdata(_:))

## Independent check

```sh
./research/gpui-fonts/compare --check
```

The check compares each GPUI line width to a native Core Text line. The native reference requests the system weight from AppKit and sets Inter’s `wght` axis directly. It also compares GPUI’s actual 2× glyph masks with dark and light ink. For “Accounts and combat profiles” at 13 px, system Medium went from 180.718 to 185.207 px and Inter Medium from 184.114 to 186.243 px. Both corrected widths match the native reference. Regular produced identical glyph masks before and after at both sizes.

The patch applies to Zed `985bbf42` and covers named weights. The bundled Latin text face is the decompressed `files/inter-latin-wght-normal.woff2` from `@fontsource-variable/inter` 5.3.0, the same asset used by the design reference. [Backend patch](gpui-apple.patch) · [Inter license](OFL.txt)
