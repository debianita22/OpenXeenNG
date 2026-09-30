// SPDX-License-Identifier: GPL-3.0-or-later
// OpenXeen browser frontend: loads openxeen.wasm, feeds it the files the
// player picks (they never leave the browser), runs one engine frame per
// animation frame and plays the audio it mixes.
(() => {
  "use strict";

  const W = 320;
  const H = 200;
  const $ = (id) => document.getElementById(id);
  const canvas = $("screen");
  const ctx = canvas.getContext("2d", { alpha: false });
  const image = ctx.createImageData(W, H);
  const statusEl = $("status");
  const clockEl = $("clock");
  const noteEl = $("data-note");
  const listEl = $("file-list");

  let ox = null; // wasm exports
  let running = false;
  let lastT = 0;
  let clock = 0;
  let audio = null;
  let nextAudioTime = 0;
  let lastStatus = "";

  // Engine key codes (see crates/web/src/lib.rs, decode_key).
  const NAMED = {
    ArrowUp: 1, ArrowDown: 2, ArrowLeft: 3, ArrowRight: 4, Enter: 5, Escape: 6,
    " ": 7, Tab: 8, Backspace: 9, PageUp: 10, PageDown: 11, Home: 12, End: 13,
  };
  function keyCode(e) {
    if (e.key in NAMED) return NAMED[e.key];
    const f = /^F([1-9]|1[0-2])$/.exec(e.key);
    if (f) return 100 + Number(f[1]);
    if (e.key.length === 1) return 0x10000 + e.key.toLowerCase().codePointAt(0);
    return 0;
  }

  // Standard gamepad layout -> engine keys (right button confirms).
  const PAD = { 0: 6, 1: 5, 2: 8, 3: 7, 4: 10, 5: 11, 8: 8, 9: 5, 12: 1, 13: 2, 14: 3, 15: 4 };
  const padDown = new Map();

  function heap() {
    return new Uint8Array(ox.memory.buffer);
  }

  function copyIn(bytes) {
    const p = ox.ox_alloc(bytes.length);
    heap().set(bytes, p); // re-read the buffer: allocation may grow memory
    return p;
  }

  function addFile(name, bytes) {
    const nb = new TextEncoder().encode(name);
    const np = copyIn(nb);
    const dp = copyIn(bytes);
    ox.ox_add_file(np, nb.length, dp, bytes.length);
  }

  function audioRate() {
    return audio ? Math.round(audio.sampleRate) : 48000;
  }

  function start(kind) {
    if (kind === "synthetic") ox.ox_start_synthetic(audioRate());
    else ox.ox_start(audioRate());
    clock = 0;
    lastT = 0;
    if (!running) {
      running = true;
      requestAnimationFrame(frame);
    }
  }

  function setStatus(text) {
    if (text !== lastStatus) {
      statusEl.textContent = text;
      lastStatus = text;
    }
  }

  function readStatus() {
    const lenPtr = ox.ox_alloc(4);
    const p = ox.ox_status(lenPtr);
    const len = new Uint32Array(ox.memory.buffer, lenPtr, 1)[0];
    const text = new TextDecoder().decode(new Uint8Array(ox.memory.buffer, p, len));
    ox.ox_free(lenPtr, 4);
    return text;
  }

  function frame(t) {
    if (!running) return;
    const dtMs = lastT ? Math.min(t - lastT, 250) : 1000 / 60;
    lastT = t;
    pollGamepads();
    const p = ox.ox_frame(Math.round(dtMs * 1000));
    image.data.set(new Uint8Array(ox.memory.buffer, p, W * H * 4));
    ctx.putImageData(image, 0, 0);
    pumpAudio();
    clock += dtMs / 1000;
    clockEl.textContent = clock.toFixed(1) + " s";
    setStatus(readStatus());
    if (ox.ox_quit_requested()) {
      running = false;
      setStatus("Stopped. Pick files, Sample data or Demo to start again.");
      return;
    }
    requestAnimationFrame(frame);
  }

  function pumpAudio() {
    if (!audio || audio.state !== "running") return;
    const rate = audio.sampleRate;
    const now = audio.currentTime;
    if (nextAudioTime < now) nextAudioTime = now + 0.02;
    const need = now + 0.1 - nextAudioTime;
    const frames = Math.floor(need * rate);
    if (frames < 256) return;
    const p = ox.ox_audio(frames);
    const pcm = new Int16Array(ox.memory.buffer, p, frames * 2);
    const buf = audio.createBuffer(2, frames, rate);
    const l = buf.getChannelData(0);
    const r = buf.getChannelData(1);
    for (let i = 0; i < frames; i++) {
      l[i] = pcm[2 * i] / 32768;
      r[i] = pcm[2 * i + 1] / 32768;
    }
    const src = audio.createBufferSource();
    src.buffer = buf;
    src.connect(audio.destination);
    src.start(nextAudioTime);
    nextAudioTime += frames / rate;
  }

  function pollGamepads() {
    let pads;
    try {
      pads = navigator.getGamepads ? navigator.getGamepads() : [];
    } catch {
      return; // blocked by the embedding page's permissions
    }
    for (const pad of pads) {
      if (!pad) continue;
      for (const [idx, code] of Object.entries(PAD)) {
        const b = pad.buttons[idx];
        const down = !!(b && b.pressed);
        const key = pad.index + ":" + idx;
        if (down !== !!padDown.get(key)) {
          padDown.set(key, down);
          ox.ox_key(code, down ? 1 : 0);
        }
      }
    }
  }

  function onKey(e, down) {
    if (!running || document.activeElement !== canvas) return;
    const code = keyCode(e);
    if (!code) return;
    e.preventDefault();
    ox.ox_key(code, down ? 1 : 0);
  }

  function pointer(e, kind) {
    if (!running) return;
    const r = canvas.getBoundingClientRect();
    const x = Math.floor(((e.clientX - r.left) * W) / r.width);
    const y = Math.floor(((e.clientY - r.top) * H) / r.height);
    ox.ox_pointer(kind, x, y);
  }

  function showFiles(files) {
    listEl.replaceChildren(
      ...files.map((f) => {
        const li = document.createElement("li");
        const n = document.createElement("span");
        const s = document.createElement("span");
        n.textContent = f.name.toUpperCase();
        s.textContent = (f.size / 1048576).toFixed(1) + " MB";
        li.append(n, s);
        return li;
      })
    );
  }

  async function loadFiles(fileList) {
    const files = Array.from(fileList).filter((f) => /\.(cc|cur)$/i.test(f.name));
    if (!files.length) {
      noteEl.textContent = "Those files are not game archives. Pick XEEN.CC, DARK.CC and INTRO.CC.";
      noteEl.classList.add("error");
      return;
    }
    noteEl.classList.remove("error");
    noteEl.textContent = "Reading " + files.length + " file(s)…";
    for (const f of files) addFile(f.name, new Uint8Array(await f.arrayBuffer()));
    showFiles(files);
    start("files");
    noteEl.textContent = "Loaded. Click the screen, then use the arrow keys and Enter.";
    canvas.focus();
  }

  function wire() {
    $("files").addEventListener("change", (e) => loadFiles(e.target.files));
    $("demo").addEventListener("click", () => {
      showFiles([]);
      noteEl.classList.remove("error");
      noteEl.textContent = "Demo running: no game data loaded.";
      start("demo");
      canvas.focus();
    });
    $("sample").addEventListener("click", () => {
      showFiles([{ name: "xeen.cc (sample made by OpenXeen)", size: 80000 }]);
      noteEl.classList.remove("error");
      noteEl.textContent = "Sample archive loaded: every viewer works on it.";
      start("synthetic");
      canvas.focus();
    });
    $("sound").addEventListener("click", async (e) => {
      try {
        if (!audio) {
          audio = new (window.AudioContext || window.webkitAudioContext)();
          nextAudioTime = 0;
        }
        if (audio.state === "suspended") await audio.resume();
        e.target.textContent = "Sound is on";
        e.target.disabled = true;
      } catch {
        e.target.textContent = "Sound unavailable";
        e.target.disabled = true;
      }
      canvas.focus();
    });
    $("fullscreen").addEventListener("click", () => {
      const box = $("screen-box");
      const req = box.requestFullscreen || box.webkitRequestFullscreen;
      if (!req) return;
      Promise.resolve(req.call(box)).then(() => canvas.focus(), () => {});
    });
    canvas.addEventListener("pointerdown", (e) => {
      canvas.focus();
      pointer(e, e.button === 2 ? 2 : 1);
    });
    canvas.addEventListener("pointermove", (e) => pointer(e, 0));
    canvas.addEventListener("pointerup", (e) => pointer(e, 3));
    canvas.addEventListener("contextmenu", (e) => e.preventDefault());
    window.addEventListener("keydown", (e) => onKey(e, true));
    window.addEventListener("keyup", (e) => onKey(e, false));

    const hint = $("drop-hint");
    let depth = 0;
    document.addEventListener("dragenter", (e) => {
      e.preventDefault();
      depth++;
      hint.hidden = false;
    });
    document.addEventListener("dragleave", () => {
      depth = Math.max(0, depth - 1);
      if (!depth) hint.hidden = true;
    });
    document.addEventListener("dragover", (e) => e.preventDefault());
    document.addEventListener("drop", (e) => {
      e.preventDefault();
      depth = 0;
      hint.hidden = true;
      if (e.dataTransfer && e.dataTransfer.files.length) loadFiles(e.dataTransfer.files);
    });
  }

  async function boot() {
    try {
      const res = await fetch("openxeen.wasm");
      if (!res.ok) throw new Error("openxeen.wasm: HTTP " + res.status);
      const { instance } = await WebAssembly.instantiate(await res.arrayBuffer(), {});
      ox = instance.exports;
    } catch (err) {
      setStatus("Could not start the engine.");
      noteEl.textContent =
        "This browser refused to run the engine (" + err.message + "). Try a current Chrome, Firefox or Safari.";
      noteEl.classList.add("error");
      return;
    }
    wire();
    start("demo");
  }

  boot();
})();
