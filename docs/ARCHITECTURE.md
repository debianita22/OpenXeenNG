# Architecture

## Goals

1. **Faithful**: with the default settings the game behaves like the DOS
   original. ScummVM's reimplementation of the Xeen engine
   (`engines/mm/xeen`, about 46 000 lines of C++) is the reference for game
   logic; differential tests hold the port to it.
2. **Improvable**: the enhancements OpenXeen was started for are opt-in
   switches on top of the faithful core, never forks of it.
3. **Portable**: one core for desktop, libretro (RetroArch, Lakka, handhelds)
   and the browser, with no platform code in the core.

## Crates

```
crates/formats   openxeen-formats   original data formats (read + write)
crates/engine    openxeen-engine    frame-stepped core, renderer, mixer, flows
crates/sdl       openxeen-sdl       desktop frontend      -> `openxeen`
crates/libretro  openxeen-libretro  libretro core         -> libopenxeen_libretro
crates/web       openxeen-web       WebAssembly module + www/ page
crates/tool      openxeen-tool      command line          -> `xeentool`
oracle/                             ScummVM-based differential test harness
```

Dependencies point one way: frontends → engine → formats. `formats` and
`engine` depend only on `std` (the tool uses `png`, the desktop frontend
`sdl2`), which keeps the WebAssembly build trivial.

## Data

`GameData` holds the archives in memory, built from `(file name, bytes)`
pairs, so every frontend loads files its own way (filesystem, libretro
content path, browser file picker). A resource is looked up as:

1. mod overrides (loose files),
2. the current side's archive (`XEEN.CC` or `DARK.CC`),
3. `INTRO.CC`,
4. loose files next to the archives.

CC archives store 16-bit name hashes, not names; `names::NameIndex` recovers
names from a dictionary of the names the engine uses (literal names plus
generated families such as `maze0xxx.dat` or `NNN.mon`).

## Execution model

The original engine is written in a blocking style: a dialog draws itself
and loops until a key arrives; a fade loops for N ticks. libretro and the
browser call the engine once per video frame and forbid blocking.

OpenXeen writes game flows as `async fn` and polls the top-level future once
per `Engine::run_frame`:

```rust
async fn confirm(cx: &Cx) -> bool {
    draw_dialog(cx);
    loop {
        match cx.wait_key().await {          // suspends until the next frame
            Key::Char('y') => return true,
            Key::Char('n') | Key::Escape => return false,
            _ => {}
        }
    }
}
```

* `Cx::next_frame()` suspends until the host's next frame; `wait_ticks(n)`
  waits for the engine's 20 Hz tick counter (the original's 50 ms frame
  counter), derived from host time.
* No thread, no executor library, no waker: the future is polled with
  `Waker::noop()` exactly once per frame.
* Shared state (screen, palette, input queue, mixer, RNG) sits in an
  `Rc<RefCell<Io>>`, borrowed only inside `Cx::with` closures, never across
  an `await`.

Porting a ScummVM function is then mostly mechanical: blocking waits become
`.await`s and the global `g_vm->_x` accessors become explicit context.

## Rendering

The screen is a 320×200 8-bit indexed surface, as in VGA mode 13h. The
palette is kept in the engine's 8-bit "6-bit DAC << 2" levels, which is what
the fades of the original operate on; frontends convert to true colour with
a lookup table (`DacScaling::Full` maps 63 to 255, `Shift` keeps 252 like
ScummVM).

`gfx::draw_sprite` ports ScummVM's sprite renderer: scaling masks for the
distance steps of the 3D view, horizontal flip, clipping to the 3D view or a
bottom line, 2×2 enlargement and the six effect drawers. Deliberate
differences, all covered by the oracle tests where they matter:

* out-of-surface writes are dropped instead of corrupting memory;
* 2×2 enlargement uses the destination width as stride (ScummVM hard-codes
  320, which only matters off-screen);
* drawer 5 recomputes its threshold per call (ScummVM caches the first value
  in a function-level `static`).

Frontends scale the picture: 4:3 display aspect (320×200 was shown on 4:3
monitors), integer upscale then a smooth stretch on the desktop, nearest
neighbour in the browser, the frontend's own shaders in RetroArch.

## Audio

`audio::Mixer` mixes mono sources at any rate into interleaved 16-bit stereo
at the frontend's rate (48 kHz by default), one voice per channel (effects,
speech, music), as the original cut the previous effect. Adlib music needs
an OPL2/OPL3 emulator and the port of the game's music driver (milestone 1).

## Randomness

`rng::ScummRandom` is ScummVM's `Common::RandomSource` bit for bit. All
randomness goes through `&mut dyn RandomSource`, so tests can pin sequences
and the oracle can replay the same numbers.

## Verification

| Layer | How |
|---|---|
| Parsers | unit tests on hand-built and synthetic data; malformed data must error, not panic |
| Real formats | `formats/tests/reference_samples.rs` on ScummVM's real-format sample files (`OPENXEEN_REF_DIR`) |
| Ported algorithms | `engine/tests/oracle.rs`: same random inputs to the Rust port and to ScummVM's own code (`OPENXEEN_ORACLE`) |
| Engine flows | headless runs through `Engine` with scripted input |
| Frontends | simulated libretro frontend, RetroArch `--max-frames` screenshots, SDL under Xvfb, Playwright for the browser |

The oracle extracts functions verbatim from a pinned ScummVM revision with
`awk` (`oracle/build.sh`); if upstream moves code, the extraction fails
loudly instead of testing the wrong thing.

## Findings

* ScummVM's own generated map script `mazex255.evt` (the "Isle of ScummVM"
  mirror destination, `devtools/create_mm/create_xeen/map.cpp`,
  `BENCH2_EVENTS`) declares a 14-byte record with only 12 bytes of payload:
  the "give gems" event swallows the bench's `Exit` record and the file ends
  in a 4-byte fragment. `evt::parse_events` reports it; `parse_events_lenient`
  keeps every complete record, which is what the engine can reach anyway.
