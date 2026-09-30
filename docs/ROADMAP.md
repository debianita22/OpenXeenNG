# Roadmap

Each milestone ends with something playable or inspectable on all three
frontends, and with tests that pin the ported behaviour to the reference.

## M0 — Foundation ✅

Formats (read + write), frame-stepped core, sprite renderer, mixer, asset
browser, desktop/libretro/browser frontends, `xeentool`, ScummVM oracle.

## M1 — Walking around

* Map loading with the eight surrounding maps (`Map::load`, `mazeLookup`).
* Indoor and outdoor 3D view: draw lists, walls, surfaces, sky, ground,
  objects, monsters in their idle frames (`interface_scene.cpp`).
* Movement, turning, collisions, map changes at the edges, stepped-on and
  seen tiles, automap.
* Palette fades, the game font with its control codes, windows.
* Adlib music: OPL emulator + port of the music driver; sound effects.
* Acceptance: walk Vertigo and Castleview, frame-compare the 3D view with
  ScummVM at the same positions.

## M2 — The world reacts

* Event script interpreter (all 60 opcodes), texts, signs, NPC dialogs.
* Locations: shops, temples, training, bank, tavern, guilds.
* Doors, traps, teleporters, mirrors, time of day.
* Acceptance: script-by-script comparison of state changes with ScummVM.

## M3 — The party

* Characters, roster, party creation, items and enchantments, conditions,
  spells (76), resting, experience and levels.
* Save and load (original `.wox` format plus OpenXeen's own).

## M4 — Combat

* Monster movement and AI, combat rounds, spells and ranged attacks in
  combat, loot.

## M5 — Complete game

* Intro, cutscenes and endings of Clouds, Darkside and World of Xeen.
* Crossing between the two sides; Swords of Xeen.
* Acceptance: both games completable; savegame compatibility checks.

## M6 — Improvements (all optional, off by default)

The list OpenXeen was started for, plus what the new platforms need:

| Switch | Effect |
|---|---|
| Wider 3D view | extra side columns extrapolated from the original draw positions |
| Explained items | item statistics shown in the inventory |
| Stores | larger, better stocked store inventories |
| Late-game gold | rebalanced treasure after the midgame |
| Respawn | monsters return over time |
| Danger bat | the bat's animation reflects the danger level ahead |
| Fair loot | fixed contents for chests, barrels and potions, so reloading does not pay |
| Save rules | configurable save restrictions |
| Seamless world | no loading pause between neighbouring maps |
| Controller UI | every menu reachable with a gamepad; on-screen hints |
| Filters | integer, sharp-bilinear and CRT presentation |
| Mods | PNG overrides for sprites, loose-file overrides for any resource |

Each switch is a named setting with a test showing that, when off, behaviour
matches the original.
